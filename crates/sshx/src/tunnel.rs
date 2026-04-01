//! HTTP tunnel: forwards HTTP requests received via gRPC to a local server.

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use sshx_core::proto::{
    client_update::ClientMessage, HttpTunnelRequest, HttpTunnelResponse, WsTunnelFrame,
};
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

/// Maximum chunk size for HTTP response bodies (64 KB).
const CHUNK_SIZE: usize = 64 * 1024;

/// Manages HTTP tunnels from gRPC to a local HTTP server.
pub struct HttpTunnel {
    /// The local server address to forward requests to (e.g. "http://127.0.0.1:9000").
    local_addr: String,
    /// HTTP client for making local requests.
    client: reqwest::Client,
    /// Active WebSocket tunnels keyed by tunnel_id.
    ws_tunnels: Arc<tokio::sync::Mutex<HashMap<u32, WsTunnelHandle>>>,
}

/// Handle for an active WebSocket tunnel.
struct WsTunnelHandle {
    /// Sender to forward frames from gRPC to the local WebSocket.
    tx: mpsc::Sender<WsTunnelFrame>,
}

impl HttpTunnel {
    /// Create a new HTTP tunnel forwarding to the given local address.
    pub fn new(local_addr: String) -> Self {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .expect("failed to build reqwest client");
        Self {
            local_addr,
            client,
            ws_tunnels: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Update the local address (e.g. when the IDE port changes).
    pub fn set_local_addr(&mut self, addr: String) {
        self.local_addr = addr;
    }

    /// Create a lightweight clone suitable for handling a single request in a spawned task.
    pub fn clone_for_request(&self) -> HttpTunnel {
        HttpTunnel {
            local_addr: self.local_addr.clone(),
            client: self.client.clone(),
            ws_tunnels: Arc::clone(&self.ws_tunnels),
        }
    }

    /// Handle an incoming HTTP tunnel request and send response chunks back via output_tx.
    pub async fn handle_http_request(
        &self,
        req: HttpTunnelRequest,
        output_tx: &mpsc::Sender<ClientMessage>,
    ) {
        let tunnel_id = req.tunnel_id;

        if req.websocket_upgrade {
            self.handle_ws_upgrade(req, output_tx).await;
            return;
        }

        let url = format!("{}{}", self.local_addr, req.path);
        debug!(%tunnel_id, method = %req.method, %url, "tunneling HTTP request");

        let method = match req.method.as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            "PATCH" => reqwest::Method::PATCH,
            "HEAD" => reqwest::Method::HEAD,
            "OPTIONS" => reqwest::Method::OPTIONS,
            other => {
                warn!(%tunnel_id, "unsupported HTTP method: {other}");
                Self::send_error_response(output_tx, tunnel_id, 405, "Method Not Allowed").await;
                return;
            }
        };

        let mut request_builder = self.client.request(method, &url);

        // Forward headers (skip host, it will be set by reqwest).
        for (key, value) in &req.headers {
            if key.to_lowercase() != "host" {
                request_builder = request_builder.header(key.as_str(), value.as_str());
            }
        }

        if !req.body.is_empty() {
            request_builder = request_builder.body(req.body.to_vec());
        }

        match request_builder.send().await {
            Ok(response) => {
                let status_code = response.status().as_u16() as u32;
                let mut headers = HashMap::new();
                for (key, value) in response.headers() {
                    if let Ok(v) = value.to_str() {
                        headers.insert(key.as_str().to_string(), v.to_string());
                    }
                }

                // Add cache headers for static assets.
                if !headers.contains_key("cache-control") {
                    let path = req.path.as_str();
                    if path.contains("/static/")
                        || path.ends_with(".js")
                        || path.ends_with(".css")
                        || path.ends_with(".woff2")
                        || path.ends_with(".woff")
                        || path.ends_with(".ttf")
                    {
                        headers.insert(
                            "cache-control".to_string(),
                            "public, max-age=31536000, immutable".to_string(),
                        );
                    }
                }

                match response.bytes().await {
                    Ok(body) => {
                        // Chunk the response body.
                        if body.is_empty() {
                            let resp = HttpTunnelResponse {
                                tunnel_id,
                                status_code,
                                headers,
                                body_chunk: Bytes::new(),
                                done: true,
                                websocket_accepted: false,
                            };
                            output_tx
                                .send(ClientMessage::HttpTunnelResponse(resp))
                                .await
                                .ok();
                        } else {
                            let chunks: Vec<&[u8]> = body.chunks(CHUNK_SIZE).collect();
                            let total = chunks.len();
                            for (i, chunk) in chunks.into_iter().enumerate() {
                                let is_last = i == total - 1;
                                let resp = HttpTunnelResponse {
                                    tunnel_id,
                                    status_code,
                                    // Only send headers with the first chunk.
                                    headers: if i == 0 {
                                        headers.clone()
                                    } else {
                                        HashMap::new()
                                    },
                                    body_chunk: Bytes::copy_from_slice(chunk),
                                    done: is_last,
                                    websocket_accepted: false,
                                };
                                output_tx
                                    .send(ClientMessage::HttpTunnelResponse(resp))
                                    .await
                                    .ok();
                            }
                        }
                    }
                    Err(e) => {
                        error!(%tunnel_id, "failed to read response body: {e}");
                        Self::send_error_response(output_tx, tunnel_id, 502, "Bad Gateway").await;
                    }
                }
            }
            Err(e) => {
                error!(%tunnel_id, "failed to send request to local server: {e}");
                Self::send_error_response(output_tx, tunnel_id, 502, "Bad Gateway").await;
            }
        }
    }

    /// Handle a WebSocket upgrade tunnel request.
    async fn handle_ws_upgrade(
        &self,
        req: HttpTunnelRequest,
        output_tx: &mpsc::Sender<ClientMessage>,
    ) {
        let tunnel_id = req.tunnel_id;
        let url = format!("{}{}", self.local_addr.replace("http://", "ws://"), req.path);
        debug!(%tunnel_id, %url, "tunneling WebSocket upgrade");

        // Build a WS request with proper handshake headers.
        // tungstenite does NOT add them when given a pre-built Request.
        let ws_key = tokio_tungstenite::tungstenite::handshake::client::generate_key();
        let host = url
            .strip_prefix("ws://")
            .unwrap_or(&url)
            .split('/')
            .next()
            .unwrap_or("localhost");
        let mut ws_request = tokio_tungstenite::tungstenite::http::Request::builder()
            .uri(&url)
            .method("GET")
            .header("Host", host)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", &ws_key);

        for (key, value) in &req.headers {
            let lower = key.to_lowercase();
            // Skip hop-by-hop and WebSocket handshake headers — we set our own above.
            if lower == "host"
                || lower == "upgrade"
                || lower == "connection"
                || lower.starts_with("sec-websocket-")
            {
                continue;
            }
            ws_request = ws_request.header(key.as_str(), value.as_str());
        }

        let ws_request = match ws_request.body(()) {
            Ok(r) => r,
            Err(e) => {
                error!(%tunnel_id, "failed to build WS request: {e}");
                Self::send_error_response(output_tx, tunnel_id, 500, "Internal Server Error")
                    .await;
                return;
            }
        };

        debug!(%tunnel_id, %url, "connecting WS to local server");
        match tokio_tungstenite::connect_async(ws_request).await {
            Ok((ws_stream, resp)) => {
                debug!(%tunnel_id, status = %resp.status(), "local WS connected");
                // Signal that the WebSocket was accepted.
                let resp = HttpTunnelResponse {
                    tunnel_id,
                    status_code: 101,
                    headers: HashMap::new(),
                    body_chunk: Bytes::new(),
                    done: true,
                    websocket_accepted: true,
                };
                output_tx
                    .send(ClientMessage::HttpTunnelResponse(resp))
                    .await
                    .ok();

                // Spawn bidirectional relay.
                let (ws_write, ws_read) = futures_util::StreamExt::split(ws_stream);
                let (frame_tx, frame_rx) = mpsc::channel::<WsTunnelFrame>(64);

                // Register the tunnel.
                self.ws_tunnels
                    .lock()
                    .await
                    .insert(tunnel_id, WsTunnelHandle { tx: frame_tx });

                let ws_tunnels = Arc::clone(&self.ws_tunnels);
                let output_tx_clone = output_tx.clone();
                tokio::spawn(async move {
                    Self::ws_relay(tunnel_id, ws_read, ws_write, frame_rx, &output_tx_clone).await;
                    ws_tunnels.lock().await.remove(&tunnel_id);
                    debug!(%tunnel_id, "WS tunnel closed");
                });
            }
            Err(e) => {
                error!(%tunnel_id, "failed to connect WebSocket: {e}");
                Self::send_error_response(output_tx, tunnel_id, 502, "Bad Gateway").await;
            }
        }
    }

    /// Bidirectional relay between the local WebSocket and gRPC frames.
    async fn ws_relay(
        tunnel_id: u32,
        mut ws_read: futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
        mut ws_write: futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            tokio_tungstenite::tungstenite::Message,
        >,
        mut frame_rx: mpsc::Receiver<WsTunnelFrame>,
        output_tx: &mpsc::Sender<ClientMessage>,
    ) {
        use futures_util::SinkExt;
        use tokio_tungstenite::tungstenite::Message as WsMsg;

        loop {
            tokio::select! {
                // Local WS → gRPC
                msg = tokio_stream::StreamExt::next(&mut ws_read) => {
                    match msg {
                        Some(Ok(WsMsg::Text(text))) => {
                            let frame = WsTunnelFrame {
                                tunnel_id,
                                data: text.as_bytes().to_vec().into(),
                                is_text: true,
                                close: false,
                            };
                            output_tx.send(ClientMessage::WsTunnelFrame(frame)).await.ok();
                        }
                        Some(Ok(WsMsg::Binary(data))) => {
                            let frame = WsTunnelFrame {
                                tunnel_id,
                                data: data.into(),
                                is_text: false,
                                close: false,
                            };
                            output_tx.send(ClientMessage::WsTunnelFrame(frame)).await.ok();
                        }
                        Some(Ok(WsMsg::Close(_))) | None => {
                            let frame = WsTunnelFrame {
                                tunnel_id,
                                data: Bytes::new(),
                                is_text: false,
                                close: true,
                            };
                            output_tx.send(ClientMessage::WsTunnelFrame(frame)).await.ok();
                            break;
                        }
                        Some(Ok(WsMsg::Ping(data))) => {
                            ws_write.send(WsMsg::Pong(data)).await.ok();
                        }
                        Some(Ok(_)) => {} // Pong, Frame
                        Some(Err(e)) => {
                            warn!(%tunnel_id, "local WS read error: {e}");
                            break;
                        }
                    }
                }
                // gRPC → local WS
                frame = frame_rx.recv() => {
                    match frame {
                        Some(f) if f.close => {
                            ws_write.send(WsMsg::Close(None)).await.ok();
                            break;
                        }
                        Some(f) => {
                            let msg = if f.is_text {
                                WsMsg::Text(String::from_utf8_lossy(&f.data).into_owned().into())
                            } else {
                                WsMsg::Binary(f.data.to_vec().into())
                            };
                            if ws_write.send(msg).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
        }
    }

    /// Forward a WebSocket frame from gRPC to the local WebSocket.
    pub async fn handle_ws_frame(&self, frame: WsTunnelFrame) {
        let tunnel_id = frame.tunnel_id;
        let tunnels = self.ws_tunnels.lock().await;
        if let Some(handle) = tunnels.get(&tunnel_id) {
            handle.tx.send(frame).await.ok();
        } else {
            debug!(%tunnel_id, "WS tunnel not found, frame dropped");
        }
    }

    /// Send a simple error response.
    async fn send_error_response(
        output_tx: &mpsc::Sender<ClientMessage>,
        tunnel_id: u32,
        status_code: u32,
        message: &str,
    ) {
        let resp = HttpTunnelResponse {
            tunnel_id,
            status_code,
            headers: HashMap::new(),
            body_chunk: Bytes::copy_from_slice(message.as_bytes()),
            done: true,
            websocket_accepted: false,
        };
        output_tx
            .send(ClientMessage::HttpTunnelResponse(resp))
            .await
            .ok();
    }
}
