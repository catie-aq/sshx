//! Network gRPC client allowing server control of terminals.

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::pin;

use anyhow::{Context, Result};
use sshx_core::proto::{
    client_update::ClientMessage, server_update::ServerMessage,
    sshx_service_client::SshxServiceClient, ClientUpdate, CloseRequest, NewShell, OpenRequest,
};
use sshx_core::{rand_alphanumeric, Sid};
use tokio::sync::mpsc;
use tokio::task;
use tokio::time::{self, Duration, Instant, MissedTickBehavior};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tonic::transport::Channel;
use tracing::{debug, error, info, warn};

use crate::encrypt::Encrypt;
use crate::ide::{self, IdeManager, IdeSyncHandle};
use crate::runner::{Runner, ShellData};
use crate::tunnel::HttpTunnel;
use crate::workspace::{spawn_context_snapshot, spawn_describe_files, spawn_save_image_file, spawn_update_file_metadata, spawn_update_widget_name};

/// Interval for sending empty heartbeat messages to the server.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

/// Interval to automatically reestablish connections.
const RECONNECT_INTERVAL: Duration = Duration::from_secs(60);

/// Default directory name for session state persistence.
pub const SESSION_DIR: &str = ".sshx";

/// Filename for the session state JSON.
const SESSION_FILE: &str = "session.json";

/// Handles a single session's communication with the remote server.
pub struct Controller {
    origin: String,
    runner: Runner,
    encrypt: Encrypt,
    encryption_key: String,

    name: String,
    token: String,
    url: String,
    write_url: Option<String>,

    /// Path to the session state directory, or None to disable persistence.
    session_dir: Option<PathBuf>,

    /// Channels with backpressure routing messages to each shell task.
    shells_tx: HashMap<Sid, mpsc::Sender<ShellData>>,
    /// Channel shared with tasks to allow them to output client messages.
    output_tx: mpsc::Sender<ClientMessage>,
    /// Owned receiving end of the `output_tx` channel.
    output_rx: mpsc::Receiver<ClientMessage>,

    /// Path to the openvscode-server binary (None = IDE feature disabled).
    openvscode_bin: Option<PathBuf>,
    /// Default workspace directory for new IDE instances.
    workspace_dir: Option<PathBuf>,
    /// HTTP tunnels per IDE instance ID (lazily created).
    http_tunnels: HashMap<u32, HttpTunnel>,
    /// IDE managers per IDE instance ID (lazily created).
    ide_managers: HashMap<u32, IdeManager>,
    /// IDE sync server handle for the sshx-collab extension.
    ide_sync: Option<IdeSyncHandle>,
}

impl Controller {
    /// Construct a new controller, connecting to the remote server.
    ///
    /// If `session_dir` is `Some`, the controller will load/save session state
    /// from `<session_dir>/session.json`. Pass `None` to disable persistence.
    ///
    /// If `openvscode_bin` is `Some`, the controller can start an OpenVSCode
    /// Server and tunnel its HTTP traffic through gRPC.
    pub async fn new(
        origin: &str,
        name: &str,
        runner: Runner,
        enable_readers: bool,
        session_dir: Option<PathBuf>,
        openvscode_bin: Option<PathBuf>,
        workspace_dir: Option<PathBuf>,
    ) -> Result<Self> {
        debug!(%origin, "connecting to server");
        let encryption_key = rand_alphanumeric(14); // 83.3 bits of entropy

        let kdf_task = {
            let encryption_key = encryption_key.clone();
            task::spawn_blocking(move || Encrypt::new(&encryption_key))
        };

        let (write_password, kdf_write_password_task) = if enable_readers {
            let write_password = rand_alphanumeric(14); // 83.3 bits of entropy
            let task = {
                let write_password = write_password.clone();
                task::spawn_blocking(move || Encrypt::new(&write_password))
            };
            (Some(write_password), Some(task))
        } else {
            (None, None)
        };

        let mut client = Self::connect(origin).await?;
        let encrypt = kdf_task.await?;
        let write_password_hash = if let Some(task) = kdf_write_password_task {
            Some(task.await?.zeros().into())
        } else {
            None
        };

        // Load existing session snapshot for restore.
        let restore_snapshot = if let Some(ref dir) = session_dir {
            let session_file = dir.join(SESSION_FILE);
            if session_file.exists() {
                match tokio::fs::read(&session_file).await {
                    Ok(data) => {
                        info!("restoring session from {}", session_file.display());
                        Some(data)
                    }
                    Err(e) => {
                        warn!("failed to read session file: {e}");
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        let req = OpenRequest {
            origin: origin.into(),
            encrypted_zeros: encrypt.zeros().into(),
            name: name.into(),
            write_password_hash,
            restore_snapshot: restore_snapshot.map(Into::into),
        };
        let mut resp = client.open(req).await?.into_inner();
        resp.url = resp.url + "#" + &encryption_key;

        let write_url = if let Some(write_password) = write_password {
            Some(resp.url.clone() + "," + &write_password)
        } else {
            None
        };

        let (output_tx, output_rx) = mpsc::channel(64);

        // Spawn IDE sync server if IDE binary is configured.
        let ide_sync = if openvscode_bin.is_some() {
            match ide::spawn_sync_server(output_tx.clone()) {
                Ok(handle) => Some(handle),
                Err(e) => {
                    warn!("failed to start IDE sync server: {e}");
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            origin: origin.into(),
            runner,
            encrypt,
            encryption_key,
            name: resp.name,
            token: resp.token,
            url: resp.url,
            write_url,
            session_dir,
            shells_tx: HashMap::new(),
            output_tx,
            output_rx,
            openvscode_bin,
            workspace_dir,
            http_tunnels: HashMap::new(),
            ide_managers: HashMap::new(),
            ide_sync,
        })
    }

    /// Create a new gRPC client to the HTTP(S) origin.
    ///
    /// This is used on reconnection to the server, since some replicas may be
    /// gracefully shutting down, which means connected clients need to start a
    /// new TCP handshake.
    async fn connect(origin: &str) -> Result<SshxServiceClient<Channel>, tonic::transport::Error> {
        SshxServiceClient::connect(String::from(origin)).await
    }

    /// Returns the name of the session.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the HMAC token for this session (used to authenticate sshx-browser).
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Returns the URL of the session.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the write URL of the session, if it exists.
    pub fn write_url(&self) -> Option<&str> {
        self.write_url.as_deref()
    }

    /// Returns the encryption key for this session, hidden from the server.
    pub fn encryption_key(&self) -> &str {
        &self.encryption_key
    }

    /// Run the controller forever, listening for requests from the server.
    pub async fn run(&mut self) -> ! {
        let mut last_retry = Instant::now();
        let mut retries = 0;
        loop {
            if let Err(err) = self.try_channel().await {
                if last_retry.elapsed() >= Duration::from_secs(10) {
                    retries = 0;
                }
                let secs = 2_u64.pow(retries.min(4));
                error!(%err, "disconnected, retrying in {secs}s...");
                time::sleep(Duration::from_secs(secs)).await;
                retries += 1;
            }
            last_retry = Instant::now();
        }
    }

    /// Helper function used by `run()` that can return errors.
    async fn try_channel(&mut self) -> Result<()> {
        let (tx, rx) = mpsc::channel(16);

        let hello = if self.openvscode_bin.is_some() {
            ClientMessage::Hello(format!("{},{},ide", self.name, self.token))
        } else {
            ClientMessage::Hello(format!("{},{}", self.name, self.token))
        };
        send_msg(&tx, hello).await?;

        let mut client = Self::connect(&self.origin).await?;
        let resp = client.channel(ReceiverStream::new(rx)).await?;
        let mut messages = resp.into_inner(); // A stream of server messages.

        let mut interval = time::interval(HEARTBEAT_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut reconnect = pin!(time::sleep(RECONNECT_INTERVAL));
        loop {
            let message = tokio::select! {
                _ = interval.tick() => {
                    tx.send(ClientUpdate::default()).await?;
                    continue;
                }
                msg = self.output_rx.recv() => {
                    let msg = msg.context("unreachable: output_tx was closed?")?;
                    send_msg(&tx, msg).await?;
                    continue;
                }
                item = messages.next() => {
                    item.context("server closed connection")??
                        .server_message
                        .context("server message is missing")?
                }
                _ = &mut reconnect => {
                    return Ok(()); // Reconnect to the server.
                }
            };

            match message {
                ServerMessage::Input(input) => {
                    let data = self.encrypt.segment(0x200000000, input.offset, &input.data);
                    if let Some(sender) = self.shells_tx.get(&Sid(input.id)) {
                        // This line applies backpressure if the shell task is overloaded.
                        sender.send(ShellData::Data(data)).await.ok();
                    } else {
                        warn!(%input.id, "received data for non-existing shell");
                    }
                }
                ServerMessage::CreateShell(new_shell) => {
                    let id = Sid(new_shell.id);
                    let center = (new_shell.x, new_shell.y);
                    if !self.shells_tx.contains_key(&id) {
                        self.spawn_shell_task(id, center);
                    } else {
                        warn!(%id, "server asked to create duplicate shell");
                    }
                }
                ServerMessage::CloseShell(id) => {
                    // Closes the channel when it is dropped, notifying the task to shut down.
                    self.shells_tx.remove(&Sid(id));
                    send_msg(&tx, ClientMessage::ClosedShell(id)).await?;
                }
                ServerMessage::Sync(seqnums) => {
                    for (id, seq) in seqnums.map {
                        if let Some(sender) = self.shells_tx.get(&Sid(id)) {
                            sender.send(ShellData::Sync(seq)).await.ok();
                        } else {
                            warn!(%id, "received sequence number for non-existing shell");
                            send_msg(&tx, ClientMessage::ClosedShell(id)).await?;
                        }
                    }
                }
                ServerMessage::Resize(msg) => {
                    if let Some(sender) = self.shells_tx.get(&Sid(msg.id)) {
                        sender.send(ShellData::Size(msg.rows, msg.cols)).await.ok();
                    } else {
                        warn!(%msg.id, "received resize for non-existing shell");
                    }
                }
                ServerMessage::Ping(ts) => {
                    // Echo back the timestamp, for stateless latency measurement.
                    send_msg(&tx, ClientMessage::Pong(ts)).await?;
                }
                ServerMessage::DescribeFiles(req) => {
                    spawn_describe_files(req.paths);
                }
                ServerMessage::UpdateFileMetadata(meta) => {
                    spawn_update_file_metadata(
                        meta.path,
                        meta.image_path,
                        meta.description,
                        meta.widget_w,
                        meta.widget_h,
                    );
                }
                ServerMessage::SetWidgetName(req) => {
                    spawn_update_widget_name(req.instance_id, req.name);
                }
                ServerMessage::ImageFile(img) => {
                    spawn_save_image_file(img.name, img.data.to_vec());
                }
                ServerMessage::SessionSnapshot(bytes) => {
                    self.save_session_snapshot(&bytes).await;
                }
                ServerMessage::Error(err) => {
                    error!(?err, "error received from server");
                }
                ServerMessage::BrowserInput(_) => {
                    // sshx-browser handles this, not the sshx CLI client.
                }
                ServerMessage::IdeState(update) => {
                    // Relay remote IDE state to the sync server's SSE clients.
                    if let Some(ref sync) = self.ide_sync {
                        if let Ok(state) = serde_json::from_slice::<serde_json::Value>(&update.json) {
                            let event = ide::RemoteIdeEvent {
                                kind: "state".into(),
                                data: state,
                                source_ide_id: update.ide_id,
                            };
                            sync.remote_tx.send(event).ok();
                        }
                    }
                }
                ServerMessage::EditLockStatus(json) => {
                    // Relay edit lock status to the extension via SSE.
                    if let Some(ref sync) = self.ide_sync {
                        if let Ok(lock) = serde_json::from_slice::<serde_json::Value>(&json) {
                            let event = ide::RemoteIdeEvent {
                                kind: "lock".into(),
                                data: lock,
                                source_ide_id: 0, // edit locks are global, not per-IDE
                            };
                            sync.remote_tx.send(event).ok();
                        }
                    }
                }
                ServerMessage::HttpTunnelRequest(req) => {
                    self.handle_tunnel_request(req).await;
                }
                ServerMessage::WsTunnelFrame(frame) => {
                    // Route to the correct IDE tunnel by trying all active tunnels.
                    // The one that owns this tunnel_id will forward it; others silently drop it.
                    for tunnel in self.http_tunnels.values() {
                        tunnel.handle_ws_frame(frame.clone()).await;
                    }
                }
                ServerMessage::ContextSnapshotRequest(_) => {
                    spawn_context_snapshot(self.output_tx.clone());
                }
            }
        }
    }

    /// Save a session snapshot to `<session_dir>/session.json`.
    async fn save_session_snapshot(&self, data: &[u8]) {
        let dir = match &self.session_dir {
            Some(d) => d,
            None => return, // Persistence disabled.
        };
        if let Err(e) = tokio::fs::create_dir_all(dir).await {
            warn!("failed to create session dir: {e}");
            return;
        }
        let path = dir.join(SESSION_FILE);
        if let Err(e) = tokio::fs::write(&path, data).await {
            warn!("failed to write session file: {e}");
        } else {
            debug!("saved session snapshot to {}", path.display());
        }
    }

    /// Entry point to start a new terminal task on the client.
    fn spawn_shell_task(&mut self, id: Sid, center: (i32, i32)) {
        let (shell_tx, shell_rx) = mpsc::channel(16);
        let opt = self.shells_tx.insert(id, shell_tx);
        debug_assert!(opt.is_none(), "shell ID cannot be in existing tasks");

        let runner = self.runner.clone();
        let encrypt = self.encrypt.clone();
        let output_tx = self.output_tx.clone();
        tokio::spawn(async move {
            debug!(%id, "spawning new shell");
            let new_shell = NewShell {
                id: id.0,
                x: center.0,
                y: center.1,
            };
            if let Err(err) = output_tx.send(ClientMessage::CreatedShell(new_shell)).await {
                error!(%id, ?err, "failed to send shell creation message");
                return;
            }
            if let Err(err) = runner.run(id, encrypt, shell_rx, output_tx.clone()).await {
                let err = ClientMessage::Error(err.to_string());
                output_tx.send(err).await.ok();
            }
            output_tx.send(ClientMessage::ClosedShell(id.0)).await.ok();
        });
    }

    /// Returns true if the IDE feature is available (binary configured).
    pub fn has_ide(&self) -> bool {
        self.openvscode_bin.is_some()
    }

    /// Handle an HTTP tunnel request: start the correct IDE instance if needed, then forward.
    async fn handle_tunnel_request(&mut self, req: sshx_core::proto::HttpTunnelRequest) {
        use sshx_core::proto::HttpTunnelResponse;

        let ide_id = req.ide_id;
        let tunnel_id = req.tunnel_id;

        // Lazy-start the IDE instance for this ide_id on first request.
        if !self.http_tunnels.contains_key(&ide_id) {
            let bin = match &self.openvscode_bin {
                Some(b) => b.clone(),
                None => {
                    warn!("received tunnel request but no IDE binary configured");
                    let resp = HttpTunnelResponse {
                        tunnel_id,
                        status_code: 503,
                        headers: Default::default(),
                        body_chunk: bytes::Bytes::from_static(b"IDE not configured"),
                        done: true,
                        websocket_accepted: false,
                    };
                    self.output_tx.send(ClientMessage::HttpTunnelResponse(resp)).await.ok();
                    return;
                }
            };
            let ws_dir = self
                .workspace_dir
                .clone()
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let mut ide = IdeManager::new(bin, ide_id, &self.name, ws_dir);
            if let Some(ref sync) = self.ide_sync {
                ide.set_sync_port(sync.port);
            }
            if let Err(e) = ide.spawn().await {
                error!("failed to start IDE instance {ide_id}: {e:#}");
                let resp = HttpTunnelResponse {
                    tunnel_id,
                    status_code: 502,
                    headers: Default::default(),
                    body_chunk: bytes::Bytes::from(format!("Failed to start IDE: {e:#}")),
                    done: true,
                    websocket_accepted: false,
                };
                self.output_tx.send(ClientMessage::HttpTunnelResponse(resp)).await.ok();
                return;
            }
            let tunnel = HttpTunnel::new(ide.local_addr());
            self.http_tunnels.insert(ide_id, tunnel);
            self.ide_managers.insert(ide_id, ide);
        }

        if let Some(tunnel) = self.http_tunnels.get(&ide_id) {
            let output_tx = self.output_tx.clone();
            let tunnel_clone = tunnel.clone_for_request();
            tokio::spawn(async move {
                tunnel_clone.handle_http_request(req, &output_tx).await;
            });
        }
    }

    /// Returns a sender that can inject `ClientMessage`s into the gRPC stream.
    pub fn output_sender(&self) -> mpsc::Sender<ClientMessage> {
        self.output_tx.clone()
    }

    /// Terminate this session gracefully.
    pub async fn close(&self) -> Result<()> {
        debug!("closing session");
        let req = CloseRequest {
            name: self.name.clone(),
            token: self.token.clone(),
        };
        let mut client = Self::connect(&self.origin).await?;
        client.close(req).await?;
        Ok(())
    }
}

/// Attempt to send a client message over an update channel.
async fn send_msg(tx: &mpsc::Sender<ClientUpdate>, message: ClientMessage) -> Result<()> {
    let update = ClientUpdate {
        client_message: Some(message),
    };
    tx.send(update)
        .await
        .context("failed to send message to server")
}
