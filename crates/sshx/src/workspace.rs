//! Runtime workspace tasks that run alongside an sshx session.
//!
//! # Source analyzer
//!
//! Reads `.sshx-analysis.json` from the workspace root (built by
//! `sshx analyze`) and streams its contents to the server over gRPC.
//! The file is watched for changes; when the user re-runs `sshx analyze`
//! the browser receives a fresh snapshot automatically.
//!
//! If no analysis file exists yet, an in-memory structural analysis
//! (no AI descriptions) is built on-the-fly and sent as a fallback.
//!
//! # Claude tracker
//!
//! Tails the most recent Claude Code JSONL transcript and forwards events
//! to the server as `ClientUpdate::ClaudeEvent`.

#![allow(missing_docs)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use sshx_core::proto::client_update::ClientMessage;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tracing::{debug, info, warn};

use crate::analyze::{AnalysisDb, AnalysisEntry, ComponentGraph, ANALYSIS_FILENAME};

// ---------------------------------------------------------------------------
// Wire types  (must match WsSourceFile / WsClaudeEvent on the server)
// ---------------------------------------------------------------------------

/// Source-file record sent over the gRPC `SourceMetadata` message.
/// Mirrors `WsSourceFile` in `sshx-server`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    pub path: String,
    pub kind: String,
    pub local_imports: Vec<String>,
    pub libraries: Vec<String>,
    pub exports: Vec<String>,
    pub imported_by: Vec<String>,
    pub description: String,
    pub line_count: u32,
    pub last_modified: String,
    pub content: String,
    #[serde(default)]
    pub image_path: String,
    #[serde(default)]
    pub widget_w: u32,
    #[serde(default)]
    pub widget_h: u32,
}

impl From<&AnalysisEntry> for SourceFile {
    fn from(e: &AnalysisEntry) -> Self {
        SourceFile {
            path: e.path.clone(),
            kind: e.kind.clone(),
            local_imports: e.local_imports.clone(),
            libraries: e.libraries.clone(),
            exports: e.exports.clone(),
            imported_by: e.imported_by.clone(),
            description: e.description.clone(),
            line_count: e.line_count,
            last_modified: e.last_modified.clone(),
            content: e.content.clone(),
            image_path: e.image_path.clone(),
            widget_w: e.widget_w,
            widget_h: e.widget_h,
        }
    }
}

/// Claude Code event sent over the gRPC `ClaudeEvent` message.
/// Mirrors `WsClaudeEvent` in `sshx-server`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeEvent {
    pub kind: String,
    pub tool: Option<String>,
    pub content: String,
    pub timestamp: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
    /// UNIX timestamp (seconds) of the transcript file's last modification.
    /// Only present on synthetic `"transcript"` events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_mtime: Option<u64>,
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Spawn the source-file streaming task.
///
/// Reads `.sshx-analysis.json` from `workspace` and sends it to the server.
/// Watches the file for changes so the UI updates when `sshx analyze` is
/// re-run.  Falls back to an in-memory structural scan if the file is absent.
pub fn spawn_source_analyzer(workspace: PathBuf, tx: mpsc::Sender<ClientMessage>) {
    tokio::spawn(async move {
        if let Err(e) = run_source_analyzer(workspace, tx).await {
            warn!("source analyzer exited: {e:#}");
        }
    });
}

/// Spawn a task that calls the `claude` CLI to fill in AI descriptions for the
/// given file paths, then writes the result back to `.sshx-analysis.json`.
///
/// The existing file-watcher in [`spawn_source_analyzer`] will detect the
/// change and automatically re-broadcast the updated metadata to all browsers.
pub fn spawn_describe_files(paths: Vec<String>) {
    tokio::spawn(async move {
        if let Err(e) = run_describe_files(paths).await {
            warn!("describe-files task exited: {e:#}");
        }
    });
}

/// Spawn a task that persists updated file metadata (image path, description,
/// widget dimensions) into `.sshx-analysis.json`.
pub fn spawn_update_file_metadata(
    path: String,
    image_path: String,
    description: String,
    widget_w: u32,
    widget_h: u32,
) {
    tokio::spawn(async move {
        if let Err(e) =
            run_update_file_metadata(path, image_path, description, widget_w, widget_h).await
        {
            warn!("update-file-metadata task exited: {e:#}");
        }
    });
}

async fn run_update_file_metadata(
    path: String,
    image_path: String,
    description: String,
    widget_w: u32,
    widget_h: u32,
) -> Result<()> {
    let root = std::env::current_dir()?;
    let out_path = root.join(ANALYSIS_FILENAME);
    let mut db = match crate::analyze::AnalysisDb::load(&out_path)? {
        Some(db) => db,
        None => crate::analyze::AnalysisDb::build(&root)?,
    };
    if let Some(entry) = db.files.get_mut(&path) {
        if !image_path.is_empty() {
            entry.image_path = image_path;
        }
        if !description.is_empty() {
            entry.description = description;
        }
        if widget_w > 0 {
            entry.widget_w = widget_w;
        }
        if widget_h > 0 {
            entry.widget_h = widget_h;
        }
    } else {
        warn!(path = %path, "UpdateFileMetadata: file path not found in analysis DB");
    }
    db.save(&out_path)?;
    info!(path = %path, "file metadata updated in {}", out_path.display());
    Ok(())
}

/// Spawn a task that saves image bytes received from the server into `~/.sshx/images/`.
pub fn spawn_save_image_file(name: String, data: Vec<u8>) {
    tokio::spawn(async move {
        if let Err(e) = run_save_image_file(name, data).await {
            warn!("save-image-file task exited: {e:#}");
        }
    });
}

async fn run_save_image_file(name: String, data: Vec<u8>) -> Result<()> {
    let dir = dirs::home_dir()
        .context("no home directory")?
        .join(".sshx")
        .join("images");
    tokio::fs::create_dir_all(&dir).await?;
    tokio::fs::write(dir.join(&name), &data).await?;
    info!("image saved → ~/.sshx/images/{name}");
    Ok(())
}

/// Name of the widget-names persistence file in the workspace root.
const WIDGET_NAMES_FILENAME: &str = ".sshx-widgets.json";

/// Spawn a task that persists a widget name to `.sshx-widgets.json`.
pub fn spawn_update_widget_name(instance_id: String, name: String) {
    tokio::spawn(async move {
        if let Err(e) = run_update_widget_name(instance_id, name).await {
            warn!("update-widget-name task exited: {e:#}");
        }
    });
}

async fn run_update_widget_name(instance_id: String, name: String) -> Result<()> {
    let root = std::env::current_dir()?;
    let path = root.join(WIDGET_NAMES_FILENAME);
    let mut map: std::collections::HashMap<String, String> = if path.exists() {
        let raw = tokio::fs::read(&path).await?;
        serde_json::from_slice(&raw).unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };
    if name.is_empty() {
        map.remove(&instance_id);
    } else {
        map.insert(instance_id, name);
    }
    let json = serde_json::to_vec_pretty(&map)?;
    tokio::fs::write(&path, json).await?;
    Ok(())
}

/// Send persisted widget names to the server on startup.
pub async fn send_widget_names(tx: &mpsc::Sender<ClientMessage>) {
    let root = match std::env::current_dir() {
        Ok(r) => r,
        Err(_) => return,
    };
    let path = root.join(WIDGET_NAMES_FILENAME);
    if !path.exists() {
        return;
    }
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(_) => return,
    };
    // Validate it's a proper map before sending.
    if serde_json::from_slice::<std::collections::HashMap<String, String>>(&bytes).is_ok() {
        tx.send(ClientMessage::WidgetNames(bytes.into())).await.ok();
    }
}

/// Spawn the Claude Code JSONL tracker task.
pub fn spawn_claude_tracker(tx: mpsc::Sender<ClientMessage>) {
    tokio::spawn(async move {
        if let Err(e) = run_claude_tracker(tx).await {
            warn!("claude tracker exited: {e:#}");
        }
    });
}

/// Spawn a background task that polls for a running `claude` process every 3 s
/// and notifies the browser when it appears or dies.
pub fn spawn_claude_pid_tracker(tx: mpsc::Sender<ClientMessage>) {
    tokio::spawn(async move {
        if let Err(e) = run_claude_pid_tracker(tx).await {
            warn!("claude PID tracker exited: {e:#}");
        }
    });
}

// ---------------------------------------------------------------------------
// Source analyzer
// ---------------------------------------------------------------------------

/// Convert an `AnalysisDb` to the wire-format payload and send.
async fn send_db(db: &AnalysisDb, tx: &mpsc::Sender<ClientMessage>) -> Result<()> {
    let root = std::path::PathBuf::from(&db.workspace_root)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string();
    let root_path = db.workspace_root.clone();
    let files: Vec<SourceFile> = db.files.values().map(SourceFile::from).collect();
    let json = serde_json::to_vec(&serde_json::json!({ "root": root, "rootPath": root_path, "files": files }))?;
    let compressed = zstd::encode_all(&*json, 3)?;
    tx.send(ClientMessage::SourceMetadata(compressed.into()))
        .await
        .ok();

    // Also send the component graph derived from the same database.
    send_graph(&db.to_graph(), tx).await?;
    Ok(())
}

/// Serialize and send a [`ComponentGraph`] to the server.
async fn send_graph(graph: &ComponentGraph, tx: &mpsc::Sender<ClientMessage>) -> Result<()> {
    let json = serde_json::to_vec(graph)?;
    let compressed = zstd::encode_all(&*json, 3)?;
    tx.send(ClientMessage::ComponentGraph(compressed.into()))
        .await
        .ok();
    Ok(())
}

/// Load from the analysis file, or build a fallback in-memory scan.
fn load_or_build(root: &Path) -> AnalysisDb {
    let path = root.join(ANALYSIS_FILENAME);
    match AnalysisDb::load(&path) {
        Ok(Some(db)) => {
            info!(
                files = db.file_count(),
                described = db.described_count(),
                "loaded workspace analysis from {}",
                path.display()
            );
            db
        }
        Ok(None) => {
            info!(
                "no {} found — running structural scan (no AI descriptions). \
                 Run `sshx analyze` to generate full metadata.",
                ANALYSIS_FILENAME
            );
            AnalysisDb::build(root).unwrap_or_else(|e| {
                warn!("inline workspace scan failed: {e:#}");
                AnalysisDb {
                    version: 1,
                    workspace_root: root.to_string_lossy().into_owned(),
                    analyzed_at: String::new(),
                    files: Default::default(),
                }
            })
        }
        Err(e) => {
            warn!("failed to read {}: {e:#}", path.display());
            AnalysisDb {
                version: 1,
                workspace_root: root.to_string_lossy().into_owned(),
                analyzed_at: String::new(),
                files: Default::default(),
            }
        }
    }
}

async fn run_source_analyzer(root: PathBuf, tx: mpsc::Sender<ClientMessage>) -> Result<()> {
    // Initial send.
    send_db(&load_or_build(&root), &tx).await?;

    // Watch the analysis file only — re-send whenever `sshx analyze` rewrites it.
    let analysis_path = root.join(ANALYSIS_FILENAME);
    let (notify_tx, mut notify_rx) = mpsc::channel::<()>(4);

    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = notify_tx.try_send(());
            }
        },
        notify::Config::default(),
    )?;

    // It's fine if the file doesn't exist yet; we'll start watching when it
    // appears (notify will error silently if the path is absent).
    let watch_dir = analysis_path.parent().unwrap_or(&root);
    watcher.watch(watch_dir, RecursiveMode::NonRecursive)?;

    loop {
        notify_rx.recv().await;
        while notify_rx.try_recv().is_ok() {} // drain bursts
        sleep(Duration::from_millis(200)).await;

        // Only react if the analysis file itself changed.
        if !analysis_path.exists() {
            continue;
        }
        send_db(&load_or_build(&root), &tx).await?;
    }
}

// ---------------------------------------------------------------------------
// AI description runner
// ---------------------------------------------------------------------------

/// Batch size for calling the `claude` CLI.
const DESCRIBE_BATCH_SIZE: usize = 20;

/// Call the `claude` CLI to generate descriptions for `paths`, then write them
/// back into `.sshx-analysis.json`.  The file-watcher will pick up the change.
async fn run_describe_files(mut paths: Vec<String>) -> Result<()> {
    let root = std::env::current_dir()?;
    let out_path = root.join(crate::analyze::ANALYSIS_FILENAME);

    // Load or build the current db.
    let mut db = match crate::analyze::AnalysisDb::load(&out_path)? {
        Some(db) => db,
        None => crate::analyze::AnalysisDb::build(&root)?,
    };

    // If the caller passed an empty list, describe all files that have no
    // description yet.
    if paths.is_empty() {
        paths = db
            .files
            .values()
            .filter(|e| e.description.is_empty())
            .map(|e| e.path.clone())
            .collect();
    }

    if paths.is_empty() {
        info!("all files already have descriptions, nothing to do");
        return Ok(());
    }

    info!(count = paths.len(), "requesting AI descriptions");

    for chunk in paths.chunks(DESCRIBE_BATCH_SIZE) {
        let batch: Vec<serde_json::Value> = chunk
            .iter()
            .filter_map(|p| db.files.get(p))
            .map(|e| {
                serde_json::json!({
                    "path": e.path,
                    "kind": e.kind,
                    "lineCount": e.line_count,
                    "exports": e.exports,
                })
            })
            .collect();

        if batch.is_empty() {
            continue;
        }

        let prompt = format!(
            "Return a JSON object mapping each file path to a one-sentence description \
             (≤120 chars). Only output the JSON object, nothing else. Files:\n{}",
            serde_json::to_string_pretty(&batch).unwrap_or_default()
        );

        let output = tokio::process::Command::new("claude")
            .args(["--print", &prompt])
            .output()
            .await;

        let raw = match output {
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).into_owned()
            }
            Ok(out) => {
                warn!(
                    "claude exited with status {}: {}",
                    out.status,
                    String::from_utf8_lossy(&out.stderr).trim()
                );
                continue;
            }
            Err(e) => {
                warn!("failed to run claude CLI: {e}");
                continue;
            }
        };

        // Extract the first `{ … }` block from stdout.
        if let Some(start) = raw.find('{') {
            if let Some(end) = raw.rfind('}') {
                let json_str = &raw[start..=end];
                match serde_json::from_str::<std::collections::HashMap<String, String>>(json_str) {
                    Ok(map) => {
                        for (path, desc) in map {
                            if let Some(entry) = db.files.get_mut(&path) {
                                entry.description = desc.chars().take(120).collect();
                            }
                        }
                    }
                    Err(e) => warn!("failed to parse claude descriptions JSON: {e}"),
                }
            }
        }
    }

    db.save(&out_path)?;
    info!("descriptions written to {}", out_path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Claude Code tracker
// ---------------------------------------------------------------------------


#[derive(Deserialize, Debug)]
struct JournalEntry {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    #[serde(rename = "isMeta")]
    is_meta: Option<bool>,
    message: Option<serde_json::Value>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    timestamp: Option<String>,
}

fn parse_journal_entry(entry: &JournalEntry) -> Option<ClaudeEvent> {
    if entry.is_meta == Some(true) {
        return None;
    }
    if matches!(
        entry.entry_type.as_deref(),
        Some("file-history-snapshot") | Some("progress") | None
    ) {
        return None;
    }

    let msg = entry.message.as_ref()?;
    let role = msg.get("role")?.as_str()?;
    let content = msg.get("content")?;
    let timestamp = entry.timestamp.clone().unwrap_or_default();
    let session_id = entry.session_id.clone().unwrap_or_default();

    let input_tokens = msg
        .get("usage")
        .and_then(|u| u.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let output_tokens = msg
        .get("usage")
        .and_then(|u| u.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    match role {
        "assistant" => {
            let blocks = match content {
                serde_json::Value::Array(arr) => arr.clone(),
                serde_json::Value::String(s) => {
                    return Some(ClaudeEvent {
                        kind: "assistant_message".into(),
                        tool: None,
                        content: s.chars().take(500).collect(),
                        timestamp,
                        session_id,
                        input_tokens,
                        output_tokens,
                        file_mtime: None,
                    });
                }
                _ => return None,
            };
            for block in &blocks {
                let block_type = block.get("type")?.as_str()?;
                match block_type {
                    "tool_use" => {
                        let tool = block.get("name")?.as_str()?.to_string();
                        let input = block
                            .get("input")
                            .map(|v| v.to_string().chars().take(500).collect::<String>())
                            .unwrap_or_default();
                        return Some(ClaudeEvent {
                            kind: "tool_use".into(),
                            tool: Some(tool),
                            content: input,
                            timestamp,
                            session_id,
                            input_tokens,
                            output_tokens,
                            file_mtime: None,
                        });
                    }
                    "text" => {
                        let text = block.get("text")?.as_str()?;
                        return Some(ClaudeEvent {
                            kind: "assistant_message".into(),
                            tool: None,
                            content: text.chars().take(500).collect(),
                            timestamp,
                            session_id,
                            input_tokens,
                            output_tokens,
                            file_mtime: None,
                        });
                    }
                    _ => {}
                }
            }
            None
        }
        "user" => match content {
            serde_json::Value::Array(arr) => {
                for block in arr {
                    if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                        let result_content = block
                            .get("content")
                            .map(|v| v.to_string().chars().take(500).collect::<String>())
                            .unwrap_or_default();
                        return Some(ClaudeEvent {
                            kind: "tool_result".into(),
                            tool: None,
                            content: result_content,
                            timestamp,
                            session_id,
                            input_tokens: None,
                            output_tokens: None,
                            file_mtime: None,
                        });
                    }
                }
                None
            }
            serde_json::Value::String(s) => {
                // Detect /exit slash command → emit a synthetic session_end event.
                if s.contains("<command-name>/exit</command-name>") {
                    return Some(ClaudeEvent {
                        kind: "session_end".into(),
                        tool: None,
                        content: String::new(),
                        timestamp,
                        session_id,
                        input_tokens: None,
                        output_tokens: None,
                        file_mtime: None,
                    });
                }
                Some(ClaudeEvent {
                    kind: "user_message".into(),
                    tool: None,
                    content: s.chars().take(500).collect(),
                    timestamp,
                    session_id,
                    input_tokens: None,
                    output_tokens: None,
                    file_mtime: None,
                })
            }
            _ => None,
        },
        _ => None,
    }
}

/// Tail a Claude Code JSONL transcript file and forward events over `tx`.
///
/// Announces the transcript path as a synthetic `"transcript"` event, then
/// watches the file for new lines and parses each one.  Exported for testing.
pub async fn tail_transcript(
    transcript: PathBuf,
    tx: mpsc::Sender<ClientMessage>,
) -> Result<()> {
    let path_str = transcript.to_string_lossy().into_owned();
    // Use the transcript filename stem (UUID) as the session identifier so the
    // browser can bucket events from different Claude sessions separately.
    let file_session_id = transcript
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let file_mtime = std::fs::metadata(&transcript)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    info!("watching Claude transcript: {path_str}");
    if let Ok(json) = serde_json::to_vec(&ClaudeEvent {
        kind: "transcript".into(),
        tool: None,
        content: path_str,
        timestamp: String::new(),
        session_id: file_session_id,
        input_tokens: None,
        output_tokens: None,
        file_mtime,
    }) {
        tx.send(ClientMessage::ClaudeEvent(json.into())).await.ok();
    }

    // Replay the last 200 lines of existing content so reconnecting browsers
    // see recent history immediately.
    {
        use std::io::Read;
        if let Ok(mut file) = std::fs::File::open(&transcript) {
            let mut content = String::new();
            file.read_to_string(&mut content).ok();
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(200);
            for line in &lines[start..] {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(entry) = serde_json::from_str::<JournalEntry>(line) {
                    if let Some(event) = parse_journal_entry(&entry) {
                        if let Ok(json) = serde_json::to_vec(&event) {
                            tx.send(ClientMessage::ClaudeEvent(json.into())).await.ok();
                        }
                    }
                }
            }
        }
    }

    let mut byte_offset = std::fs::metadata(&transcript)
        .map(|m| m.len())
        .unwrap_or(0);

    let (notify_tx, mut notify_rx) = mpsc::channel::<()>(4);
    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = notify_tx.try_send(());
            }
        },
        notify::Config::default(),
    )?;
    watcher.watch(&transcript, RecursiveMode::NonRecursive)?;

    loop {
        let received = tokio::time::timeout(Duration::from_secs(30), notify_rx.recv()).await;

        // If the timeout fired with no notification, loop back to check again.
        if received.is_err() {
            continue;
        }

        let new_bytes = {
            use std::io::{Read, Seek, SeekFrom};
            let mut file = match std::fs::File::open(&transcript) {
                Ok(f) => f,
                Err(_) => return Ok(()), // file deleted — let outer loop re-discover
            };
            let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
            if file_len <= byte_offset {
                continue;
            }
            file.seek(SeekFrom::Start(byte_offset)).ok();
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).ok();
            byte_offset = file_len;
            buf
        };

        for line in String::from_utf8_lossy(&new_bytes).lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<JournalEntry>(line) {
                Ok(entry) => {
                    if let Some(event) = parse_journal_entry(&entry) {
                        if let Ok(json) = serde_json::to_vec(&event) {
                            tx.send(ClientMessage::ClaudeEvent(json.into())).await.ok();
                        }
                    }
                }
                Err(e) => debug!("failed to parse journal line: {e}"),
            }
        }
    }
}

/// Return the `~/.claude/projects/<encoded-cwd>/` directory path, or `None`.
fn claude_project_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let cwd = std::env::current_dir().ok()?;
    let encoded = cwd.to_string_lossy().replace('/', "-");
    Some(home.join(".claude").join("projects").join(encoded))
}

/// Watch `project_dir` for new `.jsonl` transcripts and spawn `tail_transcript`
/// for each one discovered.  Also does an initial scan for pre-existing files.
/// Returns when the watcher's internal channel closes (unexpected).
///
/// Exported as `pub(crate)` so tests can call it directly with a `TempDir`.
pub(crate) async fn watch_transcripts_in_dir(
    project_dir: &Path,
    spawned: &mut HashSet<PathBuf>,
    tx: mpsc::Sender<ClientMessage>,
) -> Result<()> {
    // Helper: spawn tail_transcript for a path if not already spawned.
    let mut spawn_tail = |path: PathBuf| {
        if spawned.insert(path.clone()) {
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Err(e) = tail_transcript(path, tx).await {
                    warn!("claude transcript tail exited: {e:#}");
                }
            });
        }
    };

    // Initial scan: pick up any pre-existing .jsonl files.
    let entries: Vec<PathBuf> = std::fs::read_dir(project_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .map(|e| e.path())
        .collect();
    for path in entries {
        spawn_tail(path);
    }

    // Set up a directory watcher so new files are picked up immediately.
    let (dir_tx, mut dir_rx) = mpsc::channel::<PathBuf>(16);
    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                if matches!(event.kind, notify::EventKind::Create(_)) {
                    for path in event.paths {
                        if path.extension().and_then(|x| x.to_str()) == Some("jsonl") {
                            let _ = dir_tx.try_send(path);
                        }
                    }
                }
            }
        },
        notify::Config::default(),
    )?;
    watcher.watch(project_dir, RecursiveMode::NonRecursive)?;

    info!("watching Claude project dir for new transcripts: {project_dir:?}");

    // Process new-file events until the channel closes (watcher dropped).
    loop {
        match tokio::time::timeout(Duration::from_secs(60), dir_rx.recv()).await {
            Ok(Some(path)) => {
                info!("new Claude transcript detected: {path:?}");
                spawn_tail(path);
            }
            Ok(None) => return Ok(()), // sender dropped — caller should restart
            Err(_) => {}               // timeout — loop back and wait again
        }
    }
}

async fn run_claude_tracker(tx: mpsc::Sender<ClientMessage>) -> Result<()> {
    let mut spawned: HashSet<PathBuf> = HashSet::new();

    loop {
        let Some(project_dir) = claude_project_dir() else {
            debug!("cannot determine Claude project dir, retrying in 5s");
            sleep(Duration::from_secs(5)).await;
            continue;
        };

        if !project_dir.exists() {
            debug!("Claude project dir not found, retrying in 5s");
            sleep(Duration::from_secs(5)).await;
            continue;
        }

        match watch_transcripts_in_dir(&project_dir, &mut spawned, tx.clone()).await {
            Ok(()) => warn!("Claude directory watcher closed, restarting in 5s"),
            Err(e) => warn!("Claude directory watcher error: {e:#}, restarting in 5s"),
        }
        sleep(Duration::from_secs(5)).await;
    }
}

// ---------------------------------------------------------------------------
// Claude PID tracker
// ---------------------------------------------------------------------------

/// Find the PID of the most recently started `claude` process via `pgrep`.
async fn find_claude_pid() -> Option<u32> {
    let output = tokio::process::Command::new("pgrep")
        .args(["-n", "claude"])
        .output()
        .await
        .ok()?;
    if output.status.success() {
        String::from_utf8_lossy(&output.stdout).trim().parse().ok()
    } else {
        None
    }
}

/// Check if a process with the given PID is still alive by sending signal 0.
#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
    use nix::sys::signal;
    use nix::unistd::Pid;
    signal::kill(Pid::from_raw(pid as i32), None).is_ok()
}

#[cfg(not(unix))]
fn pid_is_alive(_pid: u32) -> bool {
    true
}

/// Send a `claude_pid` synthetic event to the browser.
/// `status` is `"running"` or `"dead"`; `pid_str` is the PID number or `""`.
async fn send_pid_event(tx: &mpsc::Sender<ClientMessage>, pid_str: &str, status: &str) {
    if let Ok(json) = serde_json::to_vec(&ClaudeEvent {
        kind: "claude_pid".into(),
        tool: Some(status.into()),
        content: pid_str.into(),
        timestamp: String::new(),
        session_id: String::new(),
        input_tokens: None,
        output_tokens: None,
        file_mtime: None,
    }) {
        tx.send(ClientMessage::ClaudeEvent(json.into())).await.ok();
    }
}

/// Track a known `initial_pid` Claude process and emit lifecycle events over `tx`.
///
/// Immediately emits a `"running"` event for `initial_pid`, then polls every
/// 3 seconds.  When the process dies a `"dead"` event is emitted and pgrep is
/// used to find a replacement.  Exported for testing.
pub async fn track_pid_lifecycle(
    initial_pid: u32,
    tx: mpsc::Sender<ClientMessage>,
) -> Result<()> {
    send_pid_event(&tx, &initial_pid.to_string(), "running").await;
    let mut current_pid: Option<u32> = Some(initial_pid);

    loop {
        sleep(Duration::from_secs(3)).await;

        match current_pid {
            Some(pid) if pid_is_alive(pid) => {
                // Still alive, nothing to report.
            }
            Some(_dead_pid) => {
                send_pid_event(&tx, "", "dead").await;
                current_pid = None;
                if let Some(new_pid) = find_claude_pid().await {
                    current_pid = Some(new_pid);
                    send_pid_event(&tx, &new_pid.to_string(), "running").await;
                }
            }
            None => {
                if let Some(pid) = find_claude_pid().await {
                    current_pid = Some(pid);
                    send_pid_event(&tx, &pid.to_string(), "running").await;
                }
            }
        }
    }
}

async fn run_claude_pid_tracker(tx: mpsc::Sender<ClientMessage>) -> Result<()> {
    let mut current_pid: Option<u32> = None;

    loop {
        sleep(Duration::from_secs(3)).await;

        match current_pid {
            Some(pid) if pid_is_alive(pid) => {
                // Still alive, nothing to report.
            }
            Some(_dead_pid) => {
                send_pid_event(&tx, "", "dead").await;
                current_pid = None;
                if let Some(new_pid) = find_claude_pid().await {
                    current_pid = Some(new_pid);
                    send_pid_event(&tx, &new_pid.to_string(), "running").await;
                }
            }
            None => {
                if let Some(pid) = find_claude_pid().await {
                    current_pid = Some(pid);
                    send_pid_event(&tx, &pid.to_string(), "running").await;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::TempDir;
    use tokio::sync::mpsc;

    use super::*;

    /// A valid JSONL line that produces a `user_message` event.
    const USER_LINE: &str = r#"{"type":"say","message":{"role":"user","content":"test prompt"},"sessionId":"aaaabbbb-cccc-dddd-eeee-ffffffffffff","timestamp":"2024-01-01T00:00:00.000Z"}"#;

    /// A valid JSONL line that produces an `assistant_message` event.
    const ASSISTANT_LINE: &str = r#"{"type":"say","message":{"role":"assistant","content":[{"type":"text","text":"Hello world"}]},"sessionId":"aaaabbbb-cccc-dddd-eeee-ffffffffffff","timestamp":"2024-01-01T00:00:00.000Z"}"#;

    fn decode_event(msg: ClientMessage) -> ClaudeEvent {
        match msg {
            ClientMessage::ClaudeEvent(bytes) => serde_json::from_slice(&bytes).unwrap(),
            other => panic!("expected ClaudeEvent, got {other:?}"),
        }
    }

    /// Receive the next message with a 2-second timeout.
    async fn recv_event(rx: &mut mpsc::Receiver<ClientMessage>) -> ClaudeEvent {
        decode_event(
            tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("timed out waiting for event")
                .expect("channel closed"),
        )
    }

    // ------------------------------------------------------------------
    // tail_transcript: pre-existing lines replayed, new lines tailed
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_tail_transcript_sends_events() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("session.jsonl");

        // Write 2 lines before calling tail_transcript.
        {
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, "{USER_LINE}").unwrap();
            writeln!(f, "{ASSISTANT_LINE}").unwrap();
        }

        let (tx, mut rx) = mpsc::channel(32);
        let path_clone = path.clone();
        tokio::spawn(async move {
            tail_transcript(path_clone, tx).await.ok();
        });

        // First: synthetic "transcript" event with the file path.
        let event = recv_event(&mut rx).await;
        assert_eq!(event.kind, "transcript");
        assert!(
            event.content.ends_with("session.jsonl"),
            "transcript event content should be the file path, got: {}",
            event.content
        );

        // Second: user_message replayed from the pre-existing line.
        let event = recv_event(&mut rx).await;
        assert_eq!(event.kind, "user_message");
        assert_eq!(event.content, "test prompt");

        // Third: assistant_message replayed.
        let event = recv_event(&mut rx).await;
        assert_eq!(event.kind, "assistant_message");
        assert_eq!(event.content, "Hello world");

        // Append a new line and verify it's tailed live.
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            writeln!(
                f,
                "{}",
                USER_LINE.replace("test prompt", "second prompt")
            )
            .unwrap();
        }

        let event = recv_event(&mut rx).await;
        assert_eq!(event.kind, "user_message");
        assert_eq!(event.content, "second prompt");
    }

    // ------------------------------------------------------------------
    // watch_transcripts_in_dir: new .jsonl file triggers tail_transcript
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_watch_dir_detects_new_file() {
        let dir = TempDir::new().unwrap();
        let (tx, mut rx) = mpsc::channel(32);

        let dir_path = dir.path().to_path_buf();
        tokio::spawn(async move {
            let mut spawned = HashSet::new();
            watch_transcripts_in_dir(&dir_path, &mut spawned, tx)
                .await
                .ok();
        });

        // Give the watcher time to initialise.
        sleep(Duration::from_millis(100)).await;

        // Create a new .jsonl file with one valid line.
        let transcript_path = dir.path().join("abc123.jsonl");
        {
            let mut f = std::fs::File::create(&transcript_path).unwrap();
            writeln!(f, "{ASSISTANT_LINE}").unwrap();
        }

        // Collect events until we've seen both the synthetic "transcript" event
        // and the parsed "assistant_message" event, or until a 3 s deadline.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        let mut got_transcript = false;
        let mut got_event = false;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Some(msg)) => {
                    let event = decode_event(msg);
                    if event.kind == "transcript" && event.content.ends_with("abc123.jsonl") {
                        got_transcript = true;
                    } else if event.kind == "assistant_message" && event.content == "Hello world" {
                        got_event = true;
                    }
                    if got_transcript && got_event {
                        break;
                    }
                }
                _ => break,
            }
        }

        assert!(
            got_transcript,
            "expected synthetic 'transcript' event for abc123.jsonl"
        );
        assert!(
            got_event,
            "expected 'assistant_message' event from new transcript file"
        );
    }
}
