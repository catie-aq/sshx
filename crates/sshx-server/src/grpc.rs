//! Defines gRPC routes and application request logic.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use base64::prelude::{Engine as _, BASE64_STANDARD};
use hmac::Mac;
use sshx_core::proto::{
    browser_service_server::BrowserService, browser_update::BrowserMessage,
    client_update::ClientMessage, server_update::ServerMessage, sshx_service_server::SshxService,
    BrowserCommand, BrowserJoinRequest, BrowserJoinResponse, BrowserLeaveRequest,
    BrowserLeaveResponse, BrowserUpdate, ClientUpdate, CloseRequest, CloseResponse, OpenRequest,
    OpenResponse, ServerUpdate,
};
use sshx_core::{rand_alphanumeric, Sid, Vid};
use tokio::sync::mpsc;
use tokio::time::{self, MissedTickBehavior};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tonic::{Request, Response, Status, Streaming};
use tracing::{error, info, warn};

use crate::session::{Metadata, Session};
use crate::web::protocol::{WsClaudeEvent, WsComponentGraph, WsSourceFile, WsVideoStream};
use crate::ServerState;

/// Interval for synchronizing sequence numbers with the client.
pub const SYNC_INTERVAL: Duration = Duration::from_secs(5);

/// Interval for measuring client latency.
pub const PING_INTERVAL: Duration = Duration::from_secs(2);

/// Server that handles gRPC requests from the sshx command-line client.
#[derive(Clone)]
pub struct GrpcServer(Arc<ServerState>);

impl GrpcServer {
    /// Construct a new [`GrpcServer`] instance with associated state.
    pub fn new(state: Arc<ServerState>) -> Self {
        Self(state)
    }
}

type RR<T> = Result<Response<T>, Status>;

#[tonic::async_trait]
impl SshxService for GrpcServer {
    type ChannelStream = ReceiverStream<Result<ServerUpdate, Status>>;

    async fn open(&self, request: Request<OpenRequest>) -> RR<OpenResponse> {
        let request = request.into_inner();
        let origin = self.0.override_origin().unwrap_or(request.origin);
        if origin.is_empty() {
            return Err(Status::invalid_argument("origin is empty"));
        }
        let name = rand_alphanumeric(10);
        info!(%name, "creating new session");

        match self.0.lookup(&name) {
            Some(_) => return Err(Status::already_exists("generated duplicate ID")),
            None => {
                let metadata = Metadata {
                    encrypted_zeros: request.encrypted_zeros,
                    name: request.name,
                    write_password_hash: request.write_password_hash,
                };
                self.0.insert(&name, Arc::new(Session::new(metadata)));
            }
        };
        let token = self.0.mac().chain_update(&name).finalize();
        let url = format!("{origin}/s/{name}");
        Ok(Response::new(OpenResponse {
            name,
            token: BASE64_STANDARD.encode(token.into_bytes()),
            url,
        }))
    }

    async fn channel(&self, request: Request<Streaming<ClientUpdate>>) -> RR<Self::ChannelStream> {
        let mut stream = request.into_inner();
        let first_update = match stream.next().await {
            Some(result) => result?,
            None => return Err(Status::invalid_argument("missing first message")),
        };
        let session_name = match first_update.client_message {
            Some(ClientMessage::Hello(hello)) => {
                let (name, token) = hello
                    .split_once(',')
                    .ok_or_else(|| Status::invalid_argument("missing name and token"))?;
                validate_token(self.0.mac(), name, token)?;
                name.to_string()
            }
            _ => return Err(Status::invalid_argument("invalid first message")),
        };
        let session = match self.0.backend_connect(&session_name).await {
            Ok(Some(session)) => session,
            Ok(None) => {
                warn!(session = %session_name, "channel: session not found (may have expired)");
                return Err(Status::not_found("session not found"));
            }
            Err(err) => {
                error!(?err, "failed to connect to backend session");
                return Err(Status::internal(err.to_string()));
            }
        };

        // We now spawn an asynchronous task that sends updates to the client. Note that
        // when this task finishes, the sender end is dropped, so the receiver is
        // automatically closed.
        let (tx, rx) = mpsc::channel(16);
        tokio::spawn(async move {
            if let Err(err) = handle_streaming(&tx, &session, stream).await {
                warn!(?err, "connection exiting early due to an error");
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn close(&self, request: Request<CloseRequest>) -> RR<CloseResponse> {
        let request = request.into_inner();
        validate_token(self.0.mac(), &request.name, &request.token)?;
        info!("closing session {}", request.name);
        if let Err(err) = self.0.close_session(&request.name).await {
            error!(?err, "failed to close session {}", request.name);
            return Err(Status::internal(err.to_string()));
        }
        Ok(Response::new(CloseResponse {}))
    }
}

/// Validate the client token for a session.
#[allow(clippy::result_large_err)]
fn validate_token(mac: impl Mac, name: &str, token: &str) -> tonic::Result<()> {
    if let Ok(token) = BASE64_STANDARD.decode(token) {
        if mac.chain_update(name).verify_slice(&token).is_ok() {
            return Ok(());
        }
    }
    Err(Status::unauthenticated("invalid token"))
}

type ServerTx = mpsc::Sender<Result<ServerUpdate, Status>>;

/// Handle bidirectional streaming messages RPC messages.
async fn handle_streaming(
    tx: &ServerTx,
    session: &Session,
    mut stream: Streaming<ClientUpdate>,
) -> Result<(), &'static str> {
    let mut sync_interval = time::interval(SYNC_INTERVAL);
    sync_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut ping_interval = time::interval(PING_INTERVAL);
    ping_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            // Send periodic sync messages to the client.
            _ = sync_interval.tick() => {
                let msg = ServerMessage::Sync(session.sequence_numbers());
                if !send_msg(tx, msg).await {
                    return Err("failed to send sync message");
                }
            }
            // Send periodic pings to the client.
            _ = ping_interval.tick() => {
                send_msg(tx, ServerMessage::Ping(get_time_ms())).await;
            }
            // Send buffered server updates to the client.
            Ok(msg) = session.update_rx().recv() => {
                if !send_msg(tx, msg).await {
                    return Err("failed to send update message");
                }
            }
            // Handle incoming client messages.
            maybe_update = stream.next() => {
                if let Some(Ok(update)) = maybe_update {
                    if !handle_update(tx, session, update).await {
                        return Err("error responding to client update");
                    }
                } else {
                    // The client has hung up on their end.
                    return Ok(());
                }
            }
            // Exit on a session shutdown signal.
            _ = session.terminated() => {
                let msg = String::from("disconnecting because session is closed");
                send_msg(tx, ServerMessage::Error(msg)).await;
                return Ok(());
            }
        };
    }
}

/// Handles a singe update from the client. Returns `true` on success.
async fn handle_update(tx: &ServerTx, session: &Session, update: ClientUpdate) -> bool {
    session.access();
    match update.client_message {
        Some(ClientMessage::Hello(_)) => {
            return send_err(tx, "unexpected hello".into()).await;
        }
        Some(ClientMessage::Data(data)) => {
            if let Err(err) = session.add_data(Sid(data.id), data.data, data.seq) {
                return send_err(tx, format!("add data: {:?}", err)).await;
            }
        }
        Some(ClientMessage::CreatedShell(new_shell)) => {
            let id = Sid(new_shell.id);
            let center = (new_shell.x, new_shell.y);
            if let Err(err) = session.add_shell(id, center) {
                return send_err(tx, format!("add shell: {:?}", err)).await;
            }
        }
        Some(ClientMessage::ClosedShell(id)) => {
            if let Err(err) = session.close_shell(Sid(id)) {
                return send_err(tx, format!("close shell: {:?}", err)).await;
            }
        }
        Some(ClientMessage::SourceMetadata(bytes)) => {
            #[derive(serde::Deserialize)]
            struct Payload {
                root: Option<String>,
                files: Vec<WsSourceFile>,
            }
            match zstd::decode_all(&*bytes)
                .map_err(|e| format!("zstd decode: {e}"))
                .and_then(|raw| serde_json::from_slice::<Payload>(&raw)
                    .map_err(|e| format!("json parse: {e}")))
            {
                Ok(p) => session.update_source_files(p.root.unwrap_or_default(), p.files),
                Err(e) => warn!("failed to decode source metadata: {e}"),
            }
        }
        Some(ClientMessage::ClaudeEvent(bytes)) => {
            match serde_json::from_slice::<WsClaudeEvent>(&bytes) {
                Ok(event) => session.send_claude_event(event),
                Err(e) => warn!("failed to decode claude event: {e}"),
            }
        }
        Some(ClientMessage::ComponentGraph(bytes)) => {
            match zstd::decode_all(&*bytes)
                .map_err(|e| format!("zstd decode: {e}"))
                .and_then(|raw| serde_json::from_slice::<WsComponentGraph>(&raw)
                    .map_err(|e| format!("json parse: {e}")))
            {
                Ok(graph) => session.update_component_graph(graph),
                Err(e) => warn!("failed to decode component graph: {e}"),
            }
        }
        Some(ClientMessage::WidgetNames(bytes)) => {
            match serde_json::from_slice::<std::collections::HashMap<String, String>>(&bytes) {
                Ok(names) => session.apply_widget_names(&names),
                Err(e) => warn!("failed to decode widget names: {e}"),
            }
        }
        Some(ClientMessage::Pong(ts)) => {
            let latency = get_time_ms().saturating_sub(ts);
            session.send_latency_measurement(latency);
        }
        Some(ClientMessage::Error(err)) => {
            // TODO: Propagate these errors to listeners on the web interface?
            error!(?err, "error received from client");
        }
        None => (), // Heartbeat message, ignored.
    }
    true
}

/// Attempt to send a server message to the client.
async fn send_msg(tx: &ServerTx, message: ServerMessage) -> bool {
    let update = Ok(ServerUpdate {
        server_message: Some(message),
    });
    tx.send(update).await.is_ok()
}

/// Attempt to send an error string to the client.
async fn send_err(tx: &ServerTx, err: String) -> bool {
    send_msg(tx, ServerMessage::Error(err)).await
}

/// Server that handles gRPC requests from sshx-browser clients.
#[derive(Clone)]
pub struct BrowserGrpcServer(Arc<ServerState>);

impl BrowserGrpcServer {
    /// Construct a new [`BrowserGrpcServer`] instance.
    pub fn new(state: Arc<ServerState>) -> Self {
        Self(state)
    }
}

#[tonic::async_trait]
impl BrowserService for BrowserGrpcServer {
    async fn join(&self, request: Request<BrowserJoinRequest>) -> RR<BrowserJoinResponse> {
        let req = request.into_inner();
        validate_token(self.0.mac(), &req.session_name, &req.token)?;

        let session = match self.0.frontend_connect(&req.session_name).await {
            Ok(Ok(s)) => s,
            Ok(Err(_)) => return Err(Status::not_found("session not found")),
            Err(e) => return Err(Status::internal(e.to_string())),
        };

        let vid = session.counter().next_vid();
        let stream_info = WsVideoStream {
            owner_uid: None,
            label: "Browser".to_string(),
            is_browser: true,
            x: 100,
            y: 100,
            w: 640,
            h: 400,
        };
        session.add_video_stream(vid, stream_info);
        session.set_browser_controller(vid, None);

        info!(%vid, session = %req.session_name, "browser stream joined");
        Ok(Response::new(BrowserJoinResponse { vid: vid.0 }))
    }

    type StreamStream = ReceiverStream<Result<BrowserCommand, Status>>;

    async fn stream(
        &self,
        request: Request<tonic::Streaming<BrowserUpdate>>,
    ) -> RR<Self::StreamStream> {
        let mut inbound = request.into_inner();

        // The first message must identify the vid.
        let first = match inbound.next().await {
            Some(Ok(u)) => u,
            _ => return Err(Status::invalid_argument("expected first frame")),
        };
        let vid = match &first.browser_message {
            Some(BrowserMessage::Frame(f)) => Vid(f.vid),
            _ => return Err(Status::invalid_argument("expected first frame message")),
        };

        // Find the session that owns this vid.
        let session = match self.find_session_for_vid(vid) {
            Some(s) => s,
            None => {
                warn!(%vid, "stream: video stream not found in any session");
                return Err(Status::not_found("video stream not found"));
            }
        };

        // Channel for sending commands back to sshx-browser.
        let (cmd_tx, cmd_rx) = mpsc::channel::<Result<BrowserCommand, Status>>(32);

        // Store the frame sender in session so input events can be forwarded.
        session.register_browser_cmd_sender(vid, cmd_tx.clone());

        // Spawn a task that reads inbound frames and stores/broadcasts them.
        let session_clone = Arc::clone(&session);
        tokio::spawn(async move {
            // Process the first frame we already read.
            if let Some(BrowserMessage::Frame(frame)) = first.browser_message {
                session_clone.store_browser_frame(vid, frame);
            }
            while let Some(result) = inbound.next().await {
                match result {
                    Ok(update) => match update.browser_message {
                        Some(BrowserMessage::Frame(frame)) => {
                            session_clone.store_browser_frame(vid, frame);
                        }
                        Some(BrowserMessage::Pong(ts)) => {
                            session_clone.send_latency_measurement(ts);
                        }
                        None => {}
                    },
                    Err(e) => {
                        warn!("browser stream error: {e}");
                        break;
                    }
                }
            }
            // Stream ended — remove the video stream.
            session_clone.remove_video_stream(vid);
            session_clone.unregister_browser_cmd_sender(vid);
            info!(%vid, "browser stream disconnected");
        });

        Ok(Response::new(ReceiverStream::new(cmd_rx)))
    }

    async fn leave(&self, request: Request<BrowserLeaveRequest>) -> RR<BrowserLeaveResponse> {
        let req = request.into_inner();
        let vid = Vid(req.vid);
        if let Some(session) = self.find_session_for_vid(vid) {
            session.remove_video_stream(vid);
            session.unregister_browser_cmd_sender(vid);
        }
        Ok(Response::new(BrowserLeaveResponse {}))
    }
}

impl BrowserGrpcServer {
    fn find_session_for_vid(&self, vid: Vid) -> Option<Arc<Session>> {
        self.0.find_session_with_vid(vid)
    }
}

fn get_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system time is before the UNIX epoch")
        .as_millis() as u64
}
