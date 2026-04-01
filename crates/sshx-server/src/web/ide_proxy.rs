//! HTTP proxy for tunneling IDE (OpenVSCode Server) traffic through gRPC.
//!
//! Routes under `/ide/s/{name}/{*rest}` are intercepted here, serialized as
//! `HttpTunnelRequest` gRPC messages, sent to the CLI client, and the response
//! is reassembled from `HttpTunnelResponse` chunks.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRequest, Path, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use futures_util::SinkExt;
use sshx_core::proto::server_update::ServerMessage;
use sshx_core::proto::{HttpTunnelRequest, WsTunnelFrame};
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

use crate::session::Session;
use crate::ServerState;

/// Headers that must not be forwarded because hyper manages them.
const SKIP_HEADERS: &[&str] = &[
    "transfer-encoding",
    "connection",
    "keep-alive",
    "content-length",
];

/// Build the IDE proxy router.
pub fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        // New multi-IDE routes: /ide/s/{name}/{ide_id}/...
        .route("/s/{name}/{ide_id}/{*rest}", any(handle_ide_any))
        .route("/s/{name}/{ide_id}/", any(handle_ide_any_root))
        .route("/s/{name}/{ide_id}", any(handle_ide_any_root))
}

/// Check if a request is a WebSocket upgrade.
fn is_websocket_upgrade(headers: &HeaderMap) -> bool {
    headers
        .get("upgrade")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
}

/// Unified handler for `/ide/s/{name}/{ide_id}/{*rest}` — detects WS upgrades.
async fn handle_ide_any(
    Path((name, ide_id, rest)): Path<(String, u32, String)>,
    State(state): State<Arc<ServerState>>,
    request: axum::extract::Request,
) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let headers = request.headers().clone();

    if is_websocket_upgrade(&headers) {
        match WebSocketUpgrade::from_request(request, &state).await {
            Ok(ws) => return do_ws_upgrade(name, ide_id, rest, &uri, headers, ws, state).await,
            Err(e) => return e.into_response(),
        }
    }
    let body = request.into_body();
    do_http_tunnel(name, ide_id, rest, method, uri, headers, state, body).await
}

/// Unified handler for `/ide/s/{name}/{ide_id}` and `/ide/s/{name}/{ide_id}/` — detects WS upgrades.
async fn handle_ide_any_root(
    Path((name, ide_id)): Path<(String, u32)>,
    State(state): State<Arc<ServerState>>,
    request: axum::extract::Request,
) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let headers = request.headers().clone();

    if is_websocket_upgrade(&headers) {
        match WebSocketUpgrade::from_request(request, &state).await {
            Ok(ws) => return do_ws_upgrade(name, ide_id, String::new(), &uri, headers, ws, state).await,
            Err(e) => return e.into_response(),
        }
    }
    let body = request.into_body();
    do_http_tunnel(name, ide_id, String::new(), method, uri, headers, state, body).await
}

/// Start a WebSocket tunnel for an IDE WS upgrade request.
async fn do_ws_upgrade(
    name: String,
    ide_id: u32,
    rest: String,
    uri: &Uri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    state: Arc<ServerState>,
) -> Response {
    let session = match state.frontend_connect(&name).await {
        Ok(Ok(s)) => s,
        _ => return (StatusCode::NOT_FOUND, "Session not found").into_response(),
    };
    if !session.ide_available() {
        return (StatusCode::SERVICE_UNAVAILABLE, "IDE not available").into_response();
    }

    let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    let path = if rest.is_empty() {
        format!("/ide/s/{name}/{ide_id}/{query}")
    } else {
        format!("/ide/s/{name}/{ide_id}/{rest}{query}")
    };
    let mut header_map = HashMap::new();
    for (key, value) in headers.iter() {
        if let Ok(v) = value.to_str() {
            header_map.insert(key.as_str().to_string(), v.to_string());
        }
    }
    let tunnel_id = session.next_tunnel_id();

    debug!(%tunnel_id, %path, "WS upgrade detected on IDE path");

    ws.on_upgrade(move |socket| handle_ws_tunnel(session, tunnel_id, ide_id, path, header_map, socket))
}

/// Tunnel a normal HTTP request through gRPC to the CLI.
async fn do_http_tunnel(
    name: String,
    ide_id: u32,
    rest: String,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    state: Arc<ServerState>,
    body: Body,
) -> Response {
    let session = match state.frontend_connect(&name).await {
        Ok(Ok(s)) => s,
        _ => return (StatusCode::NOT_FOUND, "Session not found").into_response(),
    };
    if !session.ide_available() {
        return (StatusCode::SERVICE_UNAVAILABLE, "IDE not available").into_response();
    }

    let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    let path = if rest.is_empty() {
        format!("/ide/s/{name}/{ide_id}/{query}")
    } else {
        format!("/ide/s/{name}/{ide_id}/{rest}{query}")
    };

    let mut header_map = HashMap::new();
    for (key, value) in headers.iter() {
        if let Ok(v) = value.to_str() {
            header_map.insert(key.as_str().to_string(), v.to_string());
        }
    }

    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            error!("failed to read request body: {e}");
            return (StatusCode::BAD_REQUEST, "Failed to read body").into_response();
        }
    };

    let tunnel_id = session.next_tunnel_id();

    let req = HttpTunnelRequest {
        tunnel_id,
        method: method.as_str().to_string(),
        path,
        headers: header_map,
        body: body_bytes,
        websocket_upgrade: false,
        ide_id,
    };

    let (resp_tx, mut resp_rx) = mpsc::unbounded_channel();
    session.register_tunnel_response(tunnel_id, resp_tx);

    if session
        .update_tx()
        .try_send(ServerMessage::HttpTunnelRequest(req))
        .is_err()
    {
        session.remove_tunnel_response(tunnel_id);
        return (StatusCode::BAD_GATEWAY, "Failed to send to CLI").into_response();
    }

    // Collect response chunks with a 30-second timeout.
    let mut status_code = StatusCode::BAD_GATEWAY;
    let mut response_headers = HeaderMap::new();
    let mut body_parts: Vec<bytes::Bytes> = Vec::new();
    let mut got_response = false;
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(30);

    loop {
        let chunk = tokio::select! {
            c = resp_rx.recv() => c,
            _ = tokio::time::sleep_until(deadline) => {
                warn!(%tunnel_id, "tunnel response timeout");
                break;
            }
        };
        match chunk {
            Some(resp) => {
                if !got_response {
                    got_response = true;
                    status_code = StatusCode::from_u16(resp.status_code as u16)
                        .unwrap_or(StatusCode::BAD_GATEWAY);
                    for (key, value) in &resp.headers {
                        if let (Ok(k), Ok(v)) = (
                            axum::http::header::HeaderName::from_bytes(key.as_bytes()),
                            axum::http::header::HeaderValue::from_str(value),
                        ) {
                            response_headers.insert(k, v);
                        }
                    }
                }
                if !resp.body_chunk.is_empty() {
                    body_parts.push(resp.body_chunk);
                }
                if resp.done {
                    break;
                }
            }
            None => break,
        }
    }

    session.remove_tunnel_response(tunnel_id);

    if !got_response {
        return (StatusCode::BAD_GATEWAY, "No response from CLI").into_response();
    }

    let full_body: Vec<u8> = body_parts.iter().flat_map(|b| b.iter().copied()).collect();

    // Build the response, filtering out framing headers that hyper manages itself.
    let mut response = Response::builder().status(status_code);
    for (key, value) in response_headers.iter() {
        if !SKIP_HEADERS.contains(&key.as_str()) {
            response = response.header(key, value);
        }
    }
    response
        .body(Body::from(full_body))
        .unwrap_or_else(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, "Response build error").into_response()
        })
}

/// Handle a WebSocket tunnel: relay frames between browser WS and gRPC tunnel.
async fn handle_ws_tunnel(
    session: Arc<Session>,
    tunnel_id: u32,
    ide_id: u32,
    path: String,
    headers: HashMap<String, String>,
    mut socket: WebSocket,
) {
    debug!(%tunnel_id, %path, "starting WS tunnel");

    let req = HttpTunnelRequest {
        tunnel_id,
        method: "GET".to_string(),
        path,
        headers,
        body: bytes::Bytes::new(),
        websocket_upgrade: true,
        ide_id,
    };

    let (resp_tx, mut resp_rx) = mpsc::unbounded_channel();
    session.register_tunnel_response(tunnel_id, resp_tx);

    if session
        .update_tx()
        .try_send(ServerMessage::HttpTunnelRequest(req))
        .is_err()
    {
        session.remove_tunnel_response(tunnel_id);
        warn!(%tunnel_id, "failed to send WS upgrade request to CLI");
        return;
    }

    // Wait for the CLI to accept the WS upgrade.
    let accepted = tokio::time::timeout(tokio::time::Duration::from_secs(10), async {
        while let Some(resp) = resp_rx.recv().await {
            if resp.websocket_accepted {
                return true;
            }
            if resp.done {
                return false;
            }
        }
        false
    })
    .await
    .unwrap_or(false);

    session.remove_tunnel_response(tunnel_id);

    if !accepted {
        warn!(%tunnel_id, "CLI rejected WS upgrade");
        return;
    }

    // Register the WS tunnel for frame relay.
    let (frame_tx, mut frame_rx) = mpsc::unbounded_channel::<WsTunnelFrame>();
    session.register_ws_tunnel(tunnel_id, frame_tx);

    loop {
        tokio::select! {
            // Browser → CLI
            msg = socket.recv() => {
                match msg {
                    Some(Ok(WsMessage::Text(text))) => {
                        let frame = WsTunnelFrame {
                            tunnel_id,
                            data: text.as_bytes().to_vec().into(),
                            is_text: true,
                            close: false,
                        };
                        session.update_tx().try_send(ServerMessage::WsTunnelFrame(frame)).ok();
                    }
                    Some(Ok(WsMessage::Binary(data))) => {
                        let frame = WsTunnelFrame {
                            tunnel_id,
                            data: data.into(),
                            is_text: false,
                            close: false,
                        };
                        session.update_tx().try_send(ServerMessage::WsTunnelFrame(frame)).ok();
                    }
                    Some(Ok(WsMessage::Close(_))) | None => {
                        let frame = WsTunnelFrame {
                            tunnel_id,
                            data: bytes::Bytes::new(),
                            is_text: false,
                            close: true,
                        };
                        session.update_tx().try_send(ServerMessage::WsTunnelFrame(frame)).ok();
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        warn!(%tunnel_id, "browser WS error: {e}");
                        break;
                    }
                }
            }
            // CLI → Browser
            frame = frame_rx.recv() => {
                match frame {
                    Some(f) if f.close => {
                        socket.close().await.ok();
                        break;
                    }
                    Some(f) => {
                        let msg = if f.is_text {
                            WsMessage::Text(String::from_utf8_lossy(&f.data).into_owned().into())
                        } else {
                            WsMessage::Binary(f.data.to_vec().into())
                        };
                        if socket.send(msg).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    session.remove_ws_tunnel(tunnel_id);
    debug!(%tunnel_id, "WS tunnel closed");
}
