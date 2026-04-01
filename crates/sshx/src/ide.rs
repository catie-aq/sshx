//! Manages the OpenVSCode Server process on the client machine,
//! including a local sync HTTP server for the sshx-collab extension.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::State as AxumState;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};
use sshx_core::proto::client_update::ClientMessage;
use sshx_core::proto::IdeStateUpdate;
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info, warn};

/// IDE state mirroring the server-side WsIdeState.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeState {
    /// Ordered list of open file paths (tab order).
    pub open_files: Vec<String>,
    /// Currently focused/active file path.
    pub active_file: Option<String>,
    /// Cursor positions: (path, line, col).
    pub cursors: Vec<(String, u32, u32)>,
    /// Selections: (path, startLine, startCol, endLine, endCol).
    pub selections: Vec<(String, u32, u32, u32, u32)>,
    /// Visible ranges: (path, startLine, endLine).
    pub visible_ranges: Vec<(String, u32, u32)>,
    /// Whether the sidebar is visible.
    pub sidebar_visible: bool,
    /// Active sidebar view: "explorer", "search", "git", etc.
    pub sidebar_view: Option<String>,
    /// Whether the bottom panel is visible.
    pub panel_visible: bool,
    /// Workspace folder name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_folder: Option<String>,
    /// Workspace folder path on the host machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
    /// IDE instance / widget ID (0 = legacy).
    #[serde(default)]
    pub ide_id: u32,
}

/// Remote IDE state update sent to the extension via SSE.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteIdeEvent {
    /// Event kind: "state" or "lock".
    pub kind: String,
    /// JSON payload (IdeState or EditLock info).
    pub data: serde_json::Value,
    /// IDE instance ID that produced this event.
    #[serde(default)]
    pub source_ide_id: u32,
}

/// Shared state for the sync endpoint.
struct SyncState {
    /// Channel to send local IDE state to gRPC.
    output_tx: mpsc::Sender<ClientMessage>,
    /// Broadcast channel for remote IDE events from the server.
    remote_tx: broadcast::Sender<RemoteIdeEvent>,
}

/// Handle to the sync server, used by the controller.
pub struct IdeSyncHandle {
    /// Port the sync server is listening on.
    pub port: u16,
    /// Sender for remote events (controller feeds these from gRPC).
    pub remote_tx: broadcast::Sender<RemoteIdeEvent>,
}

/// Spawn the IDE sync HTTP server on a free port.
///
/// Returns a handle with the port and a broadcast sender for pushing
/// remote IDE events from the gRPC stream.
pub fn spawn_sync_server(
    output_tx: mpsc::Sender<ClientMessage>,
) -> Result<IdeSyncHandle> {
    let (remote_tx, _) = broadcast::channel(64);
    let port = find_free_port().context("failed to find a free port for IDE sync")?;

    let state = Arc::new(SyncState {
        output_tx,
        remote_tx: remote_tx.clone(),
    });

    let app = axum::Router::new()
        .route("/state", post(post_state))
        .route("/events", get(get_events))
        .route("/lock", post(post_lock))
        .route("/unlock", post(post_unlock))
        .with_state(state);

    let listener_addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
    tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(listener_addr).await {
            Ok(l) => l,
            Err(e) => {
                error!("IDE sync server failed to bind: {e}");
                return;
            }
        };
        info!(port, "IDE sync server listening");
        if let Err(e) = axum::serve(listener, app).await {
            error!("IDE sync server error: {e}");
        }
    });

    Ok(IdeSyncHandle { port, remote_tx })
}

/// POST /state — receive extension state, forward as gRPC ClientMessage.
async fn post_state(
    AxumState(state): AxumState<Arc<SyncState>>,
    Json(ide_state): Json<IdeState>,
) -> impl IntoResponse {
    let ide_id = ide_state.ide_id;
    let json = match serde_json::to_vec(&ide_state) {
        Ok(j) => j,
        Err(e) => {
            warn!("failed to serialize IDE state: {e}");
            return axum::http::StatusCode::BAD_REQUEST;
        }
    };
    let msg = ClientMessage::IdeState(IdeStateUpdate {
        json: json.into(),
        ide_id,
    });
    state.output_tx.try_send(msg).ok();
    axum::http::StatusCode::OK
}

/// Query parameters for the SSE events endpoint.
#[derive(Debug, Deserialize, Default)]
struct EventsQuery {
    /// IDE instance ID; events from this ID are filtered out (self-echo suppression).
    #[serde(default)]
    ide_id: u32,
}

/// GET /events — SSE stream of remote IDE state updates.
///
/// Accepts an optional `?ide_id=N` query param. Events originating from
/// that IDE instance are filtered out so the extension doesn't see its
/// own state echoed back.
async fn get_events(
    AxumState(state): AxumState<Arc<SyncState>>,
    axum::extract::Query(query): axum::extract::Query<EventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let my_ide_id = query.ide_id;
    let mut rx = state.remote_tx.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    // Filter out self-echo: skip events from our own IDE instance.
                    if my_ide_id != 0 && event.source_ide_id == my_ide_id {
                        continue;
                    }
                    let data = serde_json::to_string(&event).unwrap_or_default();
                    yield Ok(Event::default().event(&event.kind).data(data));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// POST /lock — request edit lock (body: { "file": "path", "ideId": N }).
///
/// Forwards the lock request as a gRPC `ClientMessage` so the server can
/// manage the edit lock and broadcast it to all connected browsers.
async fn post_lock(
    AxumState(state): AxumState<Arc<SyncState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let file = body
        .get("file")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if file.is_empty() {
        return axum::http::StatusCode::BAD_REQUEST;
    }
    let ide_id = body
        .get("ideId")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let msg = ClientMessage::EditLockRequest(sshx_core::proto::EditLockRequest {
        file,
        release: false,
        ide_id,
    });
    state.output_tx.try_send(msg).ok();
    axum::http::StatusCode::OK
}

/// POST /unlock — release edit lock.
async fn post_unlock(
    AxumState(state): AxumState<Arc<SyncState>>,
) -> impl IntoResponse {
    let msg = ClientMessage::EditLockRequest(sshx_core::proto::EditLockRequest {
        file: String::new(),
        release: true,
        ide_id: 0,
    });
    state.output_tx.try_send(msg).ok();
    axum::http::StatusCode::OK
}

/// Manages a local OpenVSCode Server instance.
pub struct IdeManager {
    /// Path to the `openvscode-server` binary.
    bin_path: PathBuf,
    /// IDE instance / widget ID, passed as `SSHX_IDE_ID` env var.
    ide_id: u32,
    /// Port the server is listening on.
    port: u16,
    /// Running child process, if any.
    child: Option<Child>,
    /// Base path for the IDE (e.g. "/ide/s/{session}").
    base_path: String,
    /// Directory to open by default in the IDE.
    workspace_dir: PathBuf,
    /// VS Code user data directory (settings, state, extensions).
    user_data_dir: PathBuf,
    /// Port of the IDE sync server (set after spawn).
    sync_port: Option<u16>,
}

impl IdeManager {
    /// Create a new IDE manager with the given binary path, IDE instance ID, session name, and workspace directory.
    ///
    /// `ide_id` must match the ID encoded in the proxy URL (`/ide/s/{session}/{ide_id}/`).
    pub fn new(bin_path: PathBuf, ide_id: u32, session_name: &str, workspace_dir: PathBuf) -> Self {
        let user_data_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".sshx")
            .join("vscode-data");
        Self {
            bin_path,
            ide_id,
            port: 0,
            child: None,
            base_path: format!("/ide/s/{session_name}/{ide_id}"),
            workspace_dir,
            user_data_dir,
            sync_port: None,
        }
    }

    /// Set the sync server port (called after sync server is spawned).
    pub fn set_sync_port(&mut self, port: u16) {
        self.sync_port = Some(port);
    }

    /// Returns the local port the IDE is listening on, or 0 if not started.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Returns the local address (e.g. "http://127.0.0.1:9000").
    pub fn local_addr(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Returns true if the IDE process is currently running.
    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    /// Returns the path to the bundled extensions directory.
    fn extensions_dir(&self) -> PathBuf {
        // Look for extensions/ next to the sshx binary, or in the workspace.
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        let bundled = exe_dir.join("extensions");
        if bundled.exists() {
            return bundled;
        }
        // Fallback: check workspace root.
        let ws_ext = self.workspace_dir.join("extensions");
        if ws_ext.exists() {
            return ws_ext;
        }
        bundled
    }

    /// Spawn the OpenVSCode Server process, binding to a free port.
    ///
    /// If the server is already running, this is a no-op.
    pub async fn spawn(&mut self) -> Result<()> {
        if self.child.is_some() {
            return Ok(());
        }

        // Seed default VS Code settings if they don't exist yet.
        self.seed_default_settings();

        // Find a free port.
        let port = find_free_port().context("failed to find a free port")?;
        self.port = port;

        info!(
            port,
            bin = %self.bin_path.display(),
            base_path = %self.base_path,
            workspace = %self.workspace_dir.display(),
            "launching OpenVSCode Server"
        );

        let mut cmd = Command::new(&self.bin_path);
        cmd.arg("--port")
            .arg(port.to_string())
            .arg("--without-connection-token")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--server-base-path")
            .arg(&self.base_path)
            .arg("--default-folder")
            .arg(&self.workspace_dir)
            .arg("--user-data-dir")
            .arg(&self.user_data_dir);

        // Point to bundled extensions if available.
        let ext_dir = self.extensions_dir();
        if ext_dir.exists() {
            cmd.arg("--extensions-dir").arg(&ext_dir);
            debug!(ext_dir = %ext_dir.display(), "using bundled extensions dir");
        }

        // Pass the sync port and IDE instance ID as environment variables for the extension.
        if let Some(sync_port) = self.sync_port {
            cmd.env("SSHX_SYNC_PORT", sync_port.to_string());
        }
        cmd.env("SSHX_IDE_ID", self.ide_id.to_string());

        let child = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| {
                format!(
                    "failed to spawn openvscode-server at {}",
                    self.bin_path.display()
                )
            })?;

        debug!(pid = child.id(), "openvscode-server spawned");
        self.child = Some(child);

        // Give it a moment to start listening.
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        Ok(())
    }

    /// Ensure default VS Code settings exist.
    ///
    /// Seeds `keyboard.dispatch = "keyCode"` and `terminal.integrated.enabled = false`.
    fn seed_default_settings(&self) {
        let settings_dir = self.user_data_dir.join("User");
        let settings_file = settings_dir.join("settings.json");

        if let Err(e) = std::fs::create_dir_all(&settings_dir) {
            warn!("failed to create VS Code settings dir: {e}");
            return;
        }

        // Read existing settings (or start from an empty object).
        let mut obj: serde_json::Map<String, serde_json::Value> = if settings_file.exists() {
            match std::fs::read_to_string(&settings_file) {
                Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
                Err(e) => {
                    warn!("failed to read VS Code settings: {e}");
                    return;
                }
            }
        } else {
            serde_json::Map::new()
        };

        let mut changed = false;

        // Keyboard dispatch: use keyCode for non-standard layouts.
        if !obj.contains_key("keyboard.dispatch") {
            obj.insert(
                "keyboard.dispatch".into(),
                serde_json::Value::String("keyCode".into()),
            );
            changed = true;
        }

        // Disable integrated terminal — users use sshx canvas terminals instead.
        if !obj.contains_key("terminal.integrated.enabled") {
            obj.insert(
                "terminal.integrated.enabled".into(),
                serde_json::Value::Bool(false),
            );
            changed = true;
        }

        if changed {
            match serde_json::to_string_pretty(&obj) {
                Ok(json) => {
                    if let Err(e) = std::fs::write(&settings_file, json) {
                        warn!("failed to write VS Code settings: {e}");
                    } else {
                        debug!(
                            path = %settings_file.display(),
                            "seeded VS Code settings"
                        );
                    }
                }
                Err(e) => warn!("failed to serialize VS Code settings: {e}"),
            }
        }
    }

    /// Stop the OpenVSCode Server process gracefully.
    pub async fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            info!("stopping openvscode-server");
            // Try SIGTERM first via kill().
            if let Err(e) = child.kill().await {
                warn!("failed to kill openvscode-server: {e}");
            }
            match tokio::time::timeout(
                tokio::time::Duration::from_secs(5),
                child.wait(),
            )
            .await
            {
                Ok(Ok(status)) => debug!(%status, "openvscode-server exited"),
                Ok(Err(e)) => error!("error waiting for openvscode-server: {e}"),
                Err(_) => warn!("openvscode-server did not exit in time"),
            }
        }
    }
}

impl Drop for IdeManager {
    fn drop(&mut self) {
        // kill_on_drop is set, so the child will be killed when dropped.
    }
}

/// Find a free TCP port on localhost.
fn find_free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    Ok(port)
}
