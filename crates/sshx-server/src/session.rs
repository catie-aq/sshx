//! Core logic for sshx sessions, independent of message transport.

use std::collections::{HashMap, VecDeque};
use std::ops::DerefMut;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use parking_lot::{Mutex, RwLock, RwLockWriteGuard};
use sshx_core::{
    proto::{
        browser_command::BrowserCommand as BrowserCmd, server_update::ServerMessage, BrowserCommand,
        InputEvent, SequenceNumbers, VideoFrame,
    },
    Did, IdCounter, Nid, Sid, Slid, Tid, Uid, Vid, Wid,
};
use tokio::sync::{broadcast, mpsc, watch, Notify};
use tonic::Status;
use tokio::time::Instant;
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream, WatchStream};
use tokio_stream::Stream;
use tracing::{debug, warn};

use crate::utils::Shutdown;
use crate::web::protocol::{WsClaudeEvent, WsDrawing, WsEditLock, WsIceCandidate, WsIdeState, WsNote, WsServer, WsSlide, WsSourceFile, WsTextBlock, WsUser, WsVideoStream, WsWidget, WsWidgetKind, WsWinsize};

pub mod snapshot;

/// Store a rolling buffer with at most this quantity of output, per shell.
const SHELL_STORED_BYTES: u64 = 1 << 21; // 2 MiB

/// Static metadata for this session.
#[derive(Debug, Clone)]
pub struct Metadata {
    /// Used to validate that clients have the correct encryption key.
    pub encrypted_zeros: Bytes,

    /// Name of the session (human-readable).
    pub name: String,

    /// Password for write access to the session.
    pub write_password_hash: Option<Bytes>,
}

/// In-memory state for a single sshx session.
#[derive(Debug)]
pub struct Session {
    /// Static metadata for this session.
    metadata: Metadata,

    /// In-memory state for the session.
    shells: RwLock<HashMap<Sid, State>>,

    /// Metadata for currently connected users.
    users: RwLock<HashMap<Uid, WsUser>>,

    /// Atomic counter to get new, unique IDs.
    counter: IdCounter,

    /// Timestamp of the last backend client message from an active connection.
    last_accessed: Mutex<Instant>,

    /// Watch channel source for the ordered list of open shells and sizes.
    source: watch::Sender<Vec<(Sid, WsWinsize)>>,

    /// Broadcasts updates to all WebSocket clients.
    ///
    /// Every update inside this channel must be of idempotent form, since
    /// messages may arrive before or after any snapshot of the current session
    /// state. Duplicated events should remain consistent.
    broadcast: broadcast::Sender<WsServer>,

    /// Sender end of a channel that buffers messages for the client.
    update_tx: async_channel::Sender<ServerMessage>,

    /// Receiver end of a channel that buffers messages for the client.
    update_rx: async_channel::Receiver<ServerMessage>,

    /// In-memory state for sticky notes.
    notes: RwLock<HashMap<Nid, WsNote>>,

    /// Watch channel source for the ordered list of notes.
    notes_source: watch::Sender<Vec<(Nid, WsNote)>>,

    /// Cached source file metadata sent by the CLI workspace analyzer.
    /// Tuple: (rootName, rootPath, files).
    source_files: RwLock<(String, String, Vec<WsSourceFile>)>,

    /// Watch channel source for source file snapshots; clients get the latest on connect.
    source_files_source: watch::Sender<(String, String, Vec<WsSourceFile>)>,

    /// In-memory widget state.
    widgets: RwLock<HashMap<Wid, WsWidget>>,

    /// Watch channel source for the widget list; clients get a snapshot on connect.
    widget_source: watch::Sender<Vec<(Wid, WsWidget)>>,

    /// Human-readable names for shell windows, set by browser clients.
    shell_names: RwLock<HashMap<Sid, String>>,

    /// Ring buffer of the last 200 Claude events for browser reconnect replay.
    claude_events: Mutex<VecDeque<WsClaudeEvent>>,

    /// Active video streams (screen shares + offscreen browser streams).
    video_streams: RwLock<HashMap<Vid, WsVideoStream>>,

    /// Watch channel source for video stream snapshots; clients get a snapshot on connect.
    video_streams_source: watch::Sender<Vec<(Vid, WsVideoStream)>>,

    /// Current controller of each offscreen browser stream (None = uncontrolled).
    browser_controllers: RwLock<HashMap<Vid, Option<Uid>>>,

    /// Command senders to connected sshx-browser gRPC streams (keyed by vid).
    browser_cmd_senders: Mutex<HashMap<Vid, mpsc::Sender<Result<BrowserCommand, Status>>>>,

    /// Per-vid subscribers receiving live VP8 frames from sshx-browser.
    browser_frame_subscribers: Mutex<HashMap<Vid, Vec<mpsc::Sender<VideoFrame>>>>,

    /// Per-vid GOP buffer: keyframe + subsequent deltas, for new-viewer catch-up.
    browser_frame_buffers: Mutex<HashMap<Vid, Vec<VideoFrame>>>,

    /// In-memory state for text blocks.
    text_blocks: RwLock<HashMap<Tid, WsTextBlock>>,

    /// Watch channel source for the ordered list of text blocks.
    text_blocks_source: watch::Sender<Vec<(Tid, WsTextBlock)>>,

    /// In-memory state for drawing strokes.
    drawings: RwLock<HashMap<Did, WsDrawing>>,

    /// Watch channel source for the ordered list of drawings.
    drawings_source: watch::Sender<Vec<(Did, WsDrawing)>>,

    /// In-memory state for slides (slideshow mode).
    slides: RwLock<HashMap<Slid, WsSlide>>,

    /// Triggered from metadata events when an immediate snapshot is needed.
    sync_notify: Notify,

    /// Set when this session has been closed and removed.
    shutdown: Shutdown,

    /// Next tunnel ID for HTTP tunnel requests.
    next_tunnel_id: std::sync::atomic::AtomicU32,

    /// Pending HTTP tunnel responses: tunnel_id → sender for assembled response chunks.
    tunnel_responses: dashmap::DashMap<u32, mpsc::UnboundedSender<sshx_core::proto::HttpTunnelResponse>>,

    /// Active WebSocket tunnels: tunnel_id → sender for frames from CLI to browser.
    ws_tunnels: dashmap::DashMap<u32, mpsc::UnboundedSender<sshx_core::proto::WsTunnelFrame>>,

    /// Whether the CLI has IDE support available.
    ide_available: std::sync::atomic::AtomicBool,

    /// Per-widget IDE editor state (open files, cursors, selections), keyed by Wid.
    ide_states: RwLock<HashMap<Wid, WsIdeState>>,

    /// Current edit lock for collaborative IDE editing.
    edit_lock: RwLock<WsEditLock>,
}

/// Internal state for each shell.
#[derive(Default, Debug)]
struct State {
    /// Sequence number, indicating how many bytes have been received.
    seqnum: u64,

    /// Terminal data chunks.
    data: Vec<Bytes>,

    /// Number of pruned data chunks before `data[0]`.
    chunk_offset: u64,

    /// Number of bytes in pruned data chunks.
    byte_offset: u64,

    /// Set when this shell is terminated.
    closed: bool,

    /// Updated when any of the above fields change.
    notify: Arc<Notify>,
}

impl Session {
    /// Construct a new session.
    pub fn new(metadata: Metadata) -> Self {
        let now = Instant::now();
        let (update_tx, update_rx) = async_channel::bounded(256);
        Session {
            metadata,
            shells: RwLock::new(HashMap::new()),
            users: RwLock::new(HashMap::new()),
            counter: IdCounter::default(),
            last_accessed: Mutex::new(now),
            source: watch::channel(Vec::new()).0,
            broadcast: broadcast::channel(64).0,
            update_tx,
            update_rx,
            notes: RwLock::new(HashMap::new()),
            notes_source: watch::channel(Vec::new()).0,
            source_files: RwLock::new((String::new(), String::new(), Vec::new())),
            source_files_source: watch::channel((String::new(), String::new(), Vec::new())).0,
            widgets: RwLock::new(HashMap::new()),
            widget_source: watch::channel(Vec::new()).0,
            shell_names: RwLock::new(HashMap::new()),
            claude_events: Mutex::new(VecDeque::new()),
            video_streams: RwLock::new(HashMap::new()),
            video_streams_source: watch::channel(Vec::new()).0,
            browser_controllers: RwLock::new(HashMap::new()),
            browser_cmd_senders: Mutex::new(HashMap::new()),
            browser_frame_subscribers: Mutex::new(HashMap::new()),
            browser_frame_buffers: Mutex::new(HashMap::new()),
            text_blocks: RwLock::new(HashMap::new()),
            text_blocks_source: watch::channel(Vec::new()).0,
            drawings: RwLock::new(HashMap::new()),
            drawings_source: watch::channel(Vec::new()).0,
            slides: RwLock::new(HashMap::new()),
            sync_notify: Notify::new(),
            shutdown: Shutdown::new(),
            next_tunnel_id: std::sync::atomic::AtomicU32::new(1),
            tunnel_responses: dashmap::DashMap::new(),
            ws_tunnels: dashmap::DashMap::new(),
            ide_available: std::sync::atomic::AtomicBool::new(false),
            ide_states: RwLock::new(HashMap::new()),
            edit_lock: RwLock::new(WsEditLock::default()),
        }
    }

    /// Returns the metadata for this session.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Gives access to the ID counter for obtaining new IDs.
    pub fn counter(&self) -> &IdCounter {
        &self.counter
    }

    /// Return the sequence numbers for current shells.
    pub fn sequence_numbers(&self) -> SequenceNumbers {
        let shells = self.shells.read();
        let mut map = HashMap::with_capacity(shells.len());
        for (key, value) in &*shells {
            if !value.closed {
                map.insert(key.0, value.seqnum);
            }
        }
        SequenceNumbers { map }
    }

    /// Receive a notification on broadcasted message events.
    pub fn subscribe_broadcast(
        &self,
    ) -> impl Stream<Item = Result<WsServer, BroadcastStreamRecvError>> + Unpin {
        BroadcastStream::new(self.broadcast.subscribe())
    }

    /// Return the current set of active (non-closed) shells.
    pub fn list_shells(&self) -> Vec<(Sid, WsWinsize)> {
        self.source.borrow().clone()
    }

    /// Receive a notification every time the set of shells is changed.
    pub fn subscribe_shells(&self) -> impl Stream<Item = Vec<(Sid, WsWinsize)>> + Unpin {
        WatchStream::new(self.source.subscribe())
    }

    /// Subscribe for chunks from a shell, until it is closed.
    pub fn subscribe_chunks(
        &self,
        id: Sid,
        mut chunknum: u64,
    ) -> impl Stream<Item = (u64, Vec<Bytes>)> + '_ {
        async_stream::stream! {
            while !self.shutdown.is_terminated() {
                // We absolutely cannot hold `shells` across an await point,
                // since that would cause deadlocks.
                let (seqnum, chunks, notified) = {
                    let shells = self.shells.read();
                    let shell = match shells.get(&id) {
                        Some(shell) if !shell.closed => shell,
                        _ => return,
                    };
                    let notify = Arc::clone(&shell.notify);
                    let notified = async move { notify.notified().await };
                    let mut seqnum = shell.byte_offset;
                    let mut chunks = Vec::new();
                    let current_chunks = shell.chunk_offset + shell.data.len() as u64;
                    if chunknum < current_chunks {
                        let start = chunknum.saturating_sub(shell.chunk_offset) as usize;
                        seqnum += shell.data[..start].iter().map(|x| x.len() as u64).sum::<u64>();
                        chunks = shell.data[start..].to_vec();
                        chunknum = current_chunks;
                    }
                    (seqnum, chunks, notified)
                };

                if !chunks.is_empty() {
                    yield (seqnum, chunks);
                }
                tokio::select! {
                    _ = notified => (),
                    _ = self.terminated() => return,
                }
            }
        }
    }

    /// Add a new shell to the session.
    pub fn add_shell(&self, id: Sid, center: (i32, i32)) -> Result<()> {
        use std::collections::hash_map::Entry::*;
        let _guard = match self.shells.write().entry(id) {
            Occupied(_) => bail!("shell already exists with id={id}"),
            Vacant(v) => v.insert(State::default()),
        };
        self.source.send_modify(|source| {
            let winsize = WsWinsize {
                x: center.0,
                y: center.1,
                ..Default::default()
            };
            source.push((id, winsize));
        });
        self.sync_now();
        Ok(())
    }

    /// Terminates an existing shell.
    pub fn close_shell(&self, id: Sid) -> Result<()> {
        match self.shells.write().get_mut(&id) {
            Some(shell) if !shell.closed => {
                shell.closed = true;
                shell.notify.notify_waiters();
            }
            Some(_) => return Ok(()),
            None => bail!("cannot close shell with id={id}, does not exist"),
        }
        self.source.send_modify(|source| {
            source.retain(|&(x, _)| x != id);
        });
        self.sync_now();
        Ok(())
    }

    fn get_shell_mut(&self, id: Sid) -> Result<impl DerefMut<Target = State> + '_> {
        let shells = self.shells.write();
        match shells.get(&id) {
            Some(shell) if !shell.closed => {
                Ok(RwLockWriteGuard::map(shells, |s| s.get_mut(&id).unwrap()))
            }
            Some(_) => bail!("cannot update shell with id={id}, already closed"),
            None => bail!("cannot update shell with id={id}, does not exist"),
        }
    }

    /// Change the size of a terminal, notifying clients if necessary.
    pub fn move_shell(&self, id: Sid, winsize: Option<WsWinsize>) -> Result<()> {
        let _guard = self.get_shell_mut(id)?; // Ensures mutual exclusion.
        self.source.send_modify(|source| {
            if let Some(idx) = source.iter().position(|&(sid, _)| sid == id) {
                let (_, oldsize) = source.remove(idx);
                source.push((id, winsize.unwrap_or(oldsize)));
            }
        });
        Ok(())
    }

    /// Receive new data into the session.
    pub fn add_data(&self, id: Sid, data: Bytes, seq: u64) -> Result<()> {
        let mut shell = self.get_shell_mut(id)?;

        if seq <= shell.seqnum && seq + data.len() as u64 > shell.seqnum {
            let start = shell.seqnum - seq;
            let segment = data.slice(start as usize..);
            debug!(%id, bytes = segment.len(), "adding data to shell");
            shell.seqnum += segment.len() as u64;
            shell.data.push(segment);

            // Prune old chunks if we've exceeded the maximum stored bytes.
            let mut stored_bytes = shell.seqnum - shell.byte_offset;
            if stored_bytes > SHELL_STORED_BYTES {
                let mut offset = 0;
                while offset < shell.data.len() && stored_bytes > SHELL_STORED_BYTES {
                    let bytes = shell.data[offset].len() as u64;
                    stored_bytes -= bytes;
                    shell.chunk_offset += 1;
                    shell.byte_offset += bytes;
                    offset += 1;
                }
                shell.data.drain(..offset);
            }

            shell.notify.notify_waiters();
        }

        Ok(())
    }

    /// List all the users in the session.
    pub fn list_users(&self) -> Vec<(Uid, WsUser)> {
        self.users
            .read()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }

    /// Update a user in place by ID, applying a callback to the object.
    pub fn update_user(&self, id: Uid, f: impl FnOnce(&mut WsUser)) -> Result<()> {
        let updated_user = {
            let mut users = self.users.write();
            let user = users.get_mut(&id).context("user not found")?;
            f(user);
            user.clone()
        };
        self.broadcast
            .send(WsServer::UserDiff(id, Some(updated_user)))
            .ok();
        Ok(())
    }

    /// Add a new user, and return a guard that removes the user when dropped.
    pub fn user_scope(&self, id: Uid, can_write: bool) -> Result<impl Drop + '_> {
        use std::collections::hash_map::Entry::*;

        #[must_use]
        struct UserGuard<'a>(&'a Session, Uid);
        impl Drop for UserGuard<'_> {
            fn drop(&mut self) {
                self.0.remove_user(self.1);
            }
        }

        match self.users.write().entry(id) {
            Occupied(_) => bail!("user already exists with id={id}"),
            Vacant(v) => {
                let user = WsUser {
                    name: format!("User {id}"),
                    cursor: None,
                    focus: None,
                    can_write,
                };
                v.insert(user.clone());
                self.broadcast.send(WsServer::UserDiff(id, Some(user))).ok();
                Ok(UserGuard(self, id))
            }
        }
    }

    /// Remove an existing user.
    fn remove_user(&self, id: Uid) {
        if self.users.write().remove(&id).is_none() {
            warn!(%id, "invariant violation: removed user that does not exist");
        }
        self.broadcast.send(WsServer::UserDiff(id, None)).ok();
        // Clean up edit lock on browser user disconnect.
        self.force_release_edit_lock(id);
        // Note: IDE states are keyed by Wid (not Uid), cleaned up when CLI disconnects.
    }

    /// Check if a user has write permission in the session.
    pub fn check_write_permission(&self, user_id: Uid) -> Result<()> {
        let users = self.users.read();
        let user = users.get(&user_id).context("user not found")?;
        if !user.can_write {
            bail!("No write permission");
        }
        Ok(())
    }

    /// Send a chat message into the room.
    pub fn send_chat(&self, id: Uid, msg: &str) -> Result<()> {
        // Populate the message with the current name in case it's not known later.
        let name = {
            let users = self.users.read();
            users.get(&id).context("user not found")?.name.clone()
        };
        self.broadcast
            .send(WsServer::Hear(id, name, msg.into()))
            .ok();
        Ok(())
    }

    /// Send a measurement of the shell latency.
    pub fn send_latency_measurement(&self, latency: u64) {
        self.broadcast.send(WsServer::ShellLatency(latency)).ok();
    }

    /// Receive a notification every time the set of notes is changed.
    pub fn subscribe_notes(&self) -> impl Stream<Item = Vec<(Nid, WsNote)>> + Unpin {
        WatchStream::new(self.notes_source.subscribe())
    }

    /// List all sticky notes in the session.
    pub fn list_notes(&self) -> Vec<(Nid, WsNote)> {
        self.notes
            .read()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }

    /// Add a new sticky note at the given canvas position.
    pub fn add_note(&self, id: Nid, x: i32, y: i32) -> Result<()> {
        use std::collections::hash_map::Entry::*;
        let note = WsNote {
            x,
            y,
            text: String::new(),
            color: "yellow".into(),
            pinned: false,
            w: 0,
            h: 0,
            font: String::new(),
        };
        match self.notes.write().entry(id) {
            Occupied(_) => bail!("note already exists with id={id}"),
            Vacant(v) => {
                v.insert(note.clone());
            }
        }
        self.notes_source.send_modify(|s| s.push((id, note)));
        self.sync_now();
        Ok(())
    }

    /// Update all fields of an existing sticky note.
    pub fn update_note(&self, id: Nid, note: WsNote) -> Result<()> {
        {
            let mut notes = self.notes.write();
            *notes.get_mut(&id).context("note not found")? = note.clone();
        }
        self.notes_source.send_modify(|s| {
            if let Some(idx) = s.iter().position(|&(nid, _)| nid == id) {
                s[idx].1 = note;
            }
        });
        self.sync_now();
        Ok(())
    }

    /// Delete a sticky note by ID.
    pub fn delete_note(&self, id: Nid) -> Result<()> {
        match self.notes.write().remove(&id) {
            Some(_) => {
                self.notes_source
                    .send_modify(|s| s.retain(|&(nid, _)| nid != id));
                self.sync_now();
                Ok(())
            }
            None => bail!("note with id={id} does not exist"),
        }
    }

    /// Subscribe to text block changes.
    pub fn subscribe_text_blocks(&self) -> impl Stream<Item = Vec<(Tid, WsTextBlock)>> + Unpin {
        WatchStream::new(self.text_blocks_source.subscribe())
    }

    /// List all text blocks in the session.
    pub fn list_text_blocks(&self) -> Vec<(Tid, WsTextBlock)> {
        self.text_blocks
            .read()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }

    /// Add a new text block at the given canvas position.
    pub fn add_text_block(&self, id: Tid, x: i32, y: i32) -> Result<()> {
        use std::collections::hash_map::Entry::*;
        let block = WsTextBlock {
            x,
            y,
            content: String::new(),
            font_size: "md".into(),
            color: "#ffffff".into(),
            align: "left".into(),
            font: String::new(),
        };
        match self.text_blocks.write().entry(id) {
            Occupied(_) => bail!("text block already exists with id={id}"),
            Vacant(v) => {
                v.insert(block.clone());
            }
        }
        self.text_blocks_source.send_modify(|s| s.push((id, block)));
        self.sync_now();
        Ok(())
    }

    /// Update all fields of an existing text block.
    pub fn update_text_block(&self, id: Tid, block: WsTextBlock) -> Result<()> {
        {
            let mut text_blocks = self.text_blocks.write();
            *text_blocks.get_mut(&id).context("text block not found")? = block.clone();
        }
        self.text_blocks_source.send_modify(|s| {
            if let Some(idx) = s.iter().position(|&(tid, _)| tid == id) {
                s[idx].1 = block;
            }
        });
        self.sync_now();
        Ok(())
    }

    /// Delete a text block by ID.
    pub fn delete_text_block(&self, id: Tid) -> Result<()> {
        match self.text_blocks.write().remove(&id) {
            Some(_) => {
                self.text_blocks_source
                    .send_modify(|s| s.retain(|&(tid, _)| tid != id));
                self.sync_now();
                Ok(())
            }
            None => bail!("text block with id={id} does not exist"),
        }
    }

    // --- Drawing methods ---

    /// Receive a notification every time the set of drawings is changed.
    pub fn subscribe_drawings(&self) -> impl Stream<Item = Vec<(Did, WsDrawing)>> + Unpin {
        WatchStream::new(self.drawings_source.subscribe())
    }

    /// Add a new drawing stroke.
    pub fn add_drawing(&self, id: Did, drawing: WsDrawing) -> Result<()> {
        self.drawings.write().insert(id, drawing.clone());
        self.drawings_source.send_modify(|s| s.push((id, drawing)));
        self.sync_now();
        Ok(())
    }

    /// Delete a drawing stroke.
    pub fn delete_drawing(&self, id: Did) -> Result<()> {
        match self.drawings.write().remove(&id) {
            Some(_) => {
                self.drawings_source
                    .send_modify(|s| s.retain(|&(did, _)| did != id));
                self.sync_now();
                Ok(())
            }
            None => bail!("drawing with id={id} does not exist"),
        }
    }

    /// Return a snapshot of all drawings.
    pub fn list_drawings(&self) -> Vec<(Did, WsDrawing)> {
        self.drawings.read().iter().map(|(&k, v)| (k, v.clone())).collect()
    }

    // --- Slide methods ---

    /// Add a new slide.
    pub fn add_slide(&self, id: Slid, slide: WsSlide) -> Result<()> {
        self.slides.write().insert(id, slide.clone());
        self.broadcast.send(WsServer::SlideDiff(id, Some(slide))).ok();
        self.sync_now();
        Ok(())
    }

    /// Update an existing slide.
    pub fn update_slide(&self, id: Slid, slide: WsSlide) -> Result<()> {
        {
            let mut slides = self.slides.write();
            *slides.get_mut(&id).context("slide not found")? = slide.clone();
        }
        self.broadcast.send(WsServer::SlideDiff(id, Some(slide))).ok();
        self.sync_now();
        Ok(())
    }

    /// Delete a slide.
    pub fn delete_slide(&self, id: Slid) -> Result<()> {
        match self.slides.write().remove(&id) {
            Some(_) => {
                self.broadcast.send(WsServer::SlideDiff(id, None)).ok();
                self.sync_now();
                Ok(())
            }
            None => bail!("slide with id={id} does not exist"),
        }
    }

    /// Reorder slides.
    pub fn reorder_slides(&self, orders: Vec<(Slid, u32)>) -> Result<()> {
        let mut slides = self.slides.write();
        for &(ref id, order) in &orders {
            if let Some(s) = slides.get_mut(id) {
                s.order = order;
            }
        }
        // Broadcast all changed slides
        for &(id, _) in &orders {
            if let Some(s) = slides.get(&id) {
                let slide: WsSlide = s.clone();
                self.broadcast.send(WsServer::SlideDiff(id, Some(slide))).ok();
            }
        }
        drop(slides);
        self.sync_now();
        Ok(())
    }

    /// Return a snapshot of all slides.
    pub fn list_slides(&self) -> Vec<(Slid, WsSlide)> {
        self.slides.read().iter().map(|(&k, v): (&Slid, &WsSlide)| (k, v.clone())).collect()
    }

    /// Replace the stored source file metadata and notify all WebSocket clients.
    pub fn update_source_files(&self, root: String, root_path: String, files: Vec<WsSourceFile>) {
        *self.source_files.write() = (root.clone(), root_path.clone(), files.clone());
        self.source_files_source.send_modify(|s| *s = (root, root_path, files));
    }

    /// Return a snapshot of the current source file metadata.
    pub fn list_source_files(&self) -> (String, String, Vec<WsSourceFile>) {
        self.source_files.read().clone()
    }

    /// Subscribe to video stream list updates (watch stream, delivers latest on connect).
    pub fn subscribe_video_streams(&self) -> impl Stream<Item = Vec<(Vid, WsVideoStream)>> + Unpin {
        WatchStream::new(self.video_streams_source.subscribe())
    }

    /// Return a snapshot of all active video streams.
    pub fn list_video_streams(&self) -> Vec<(Vid, WsVideoStream)> {
        self.video_streams
            .read()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }

    /// Add a new video stream and broadcast the addition.
    pub fn add_video_stream(&self, id: Vid, stream: WsVideoStream) {
        self.video_streams.write().insert(id, stream.clone());
        self.video_streams_source
            .send_modify(|s| s.push((id, stream.clone())));
        self.broadcast
            .send(WsServer::VideoStreamDiff(id, Some(stream)))
            .ok();
    }

    /// Update the canvas position of a video stream and broadcast the change.
    pub fn move_video_stream(&self, id: Vid, x: i32, y: i32) {
        {
            let mut streams = self.video_streams.write();
            if let Some(s) = streams.get_mut(&id) {
                s.x = x;
                s.y = y;
            }
        }
        self.video_streams_source.send_modify(|s| {
            if let Some(entry) = s.iter_mut().find(|(v, _)| *v == id) {
                entry.1.x = x;
                entry.1.y = y;
            }
        });
        let updated = self.video_streams.read().get(&id).cloned();
        if let Some(stream) = updated {
            self.broadcast.send(WsServer::VideoStreamDiff(id, Some(stream))).ok();
        }
    }

    /// Update the size of a video stream widget and broadcast the change.
    pub fn resize_video_stream(&self, id: Vid, w: u32, h: u32) {
        {
            let mut streams = self.video_streams.write();
            if let Some(s) = streams.get_mut(&id) {
                s.w = w;
                s.h = h;
            }
        }
        self.video_streams_source.send_modify(|s| {
            if let Some(entry) = s.iter_mut().find(|(v, _)| *v == id) {
                entry.1.w = w;
                entry.1.h = h;
            }
        });
        let updated = self.video_streams.read().get(&id).cloned();
        if let Some(stream) = updated {
            self.broadcast.send(WsServer::VideoStreamDiff(id, Some(stream))).ok();
        }
    }

    /// Remove a video stream by ID and broadcast the removal.
    pub fn remove_video_stream(&self, id: Vid) {
        self.video_streams.write().remove(&id);
        self.video_streams_source
            .send_modify(|s| s.retain(|(vid, _)| *vid != id));
        self.broadcast
            .send(WsServer::VideoStreamDiff(id, None))
            .ok();
        // Clean up browser controller and frame relay state.
        self.browser_controllers.write().remove(&id);
        self.browser_frame_subscribers.lock().remove(&id);
        self.browser_frame_buffers.lock().remove(&id);
    }

    /// Get the current controller of an offscreen browser stream.
    pub fn get_browser_controller(&self, vid: Vid) -> Option<Option<Uid>> {
        self.browser_controllers.read().get(&vid).copied()
    }

    /// Set the controller of an offscreen browser stream and broadcast the change.
    pub fn set_browser_controller(&self, vid: Vid, controller: Option<Uid>) {
        self.browser_controllers.write().insert(vid, controller);
        self.broadcast
            .send(WsServer::BrowserControlStatus(vid, controller))
            .ok();
    }

    /// Relay a WebRTC signaling message (offer, answer, or ICE) to a target user.
    pub fn relay_rtc_offer(&self, vid: Vid, target_uid: Uid, sdp: String) {
        self.broadcast
            .send(WsServer::RtcOffer(vid, target_uid, sdp))
            .ok();
    }

    /// Relay a WebRTC answer to a target user, including the sender's UID for multi-viewer routing.
    pub fn relay_rtc_answer(&self, vid: Vid, target_uid: Uid, sender_uid: Uid, sdp: String) {
        self.broadcast
            .send(WsServer::RtcAnswer(vid, target_uid, sender_uid, sdp))
            .ok();
    }

    /// Relay a WebRTC ICE candidate to a target user, including the sender's UID for multi-viewer routing.
    pub fn relay_rtc_ice(&self, vid: Vid, target_uid: Uid, sender_uid: Uid, candidate: WsIceCandidate) {
        self.broadcast
            .send(WsServer::RtcIce(vid, target_uid, sender_uid, candidate))
            .ok();
    }

    /// Broadcast a component-highlight request to all connected browsers.
    pub fn broadcast_highlight(&self, name: String) {
        self.broadcast
            .send(WsServer::HighlightComponent(name))
            .ok();
    }

    /// Register a command sender for an sshx-browser gRPC stream.
    pub fn register_browser_cmd_sender(
        &self,
        vid: Vid,
        tx: mpsc::Sender<Result<BrowserCommand, Status>>,
    ) {
        self.browser_cmd_senders.lock().insert(vid, tx);
    }

    /// Unregister the command sender for an sshx-browser gRPC stream.
    pub fn unregister_browser_cmd_sender(&self, vid: Vid) {
        self.browser_cmd_senders.lock().remove(&vid);
    }

    /// Ingest a video frame from sshx-browser and relay it to watching WebSocket clients.
    ///
    /// Maintains a per-vid GOP buffer (keyframe + subsequent deltas) so that new viewers
    /// can catch up immediately when they subscribe.
    pub fn store_browser_frame(&self, vid: Vid, frame: VideoFrame) {
        // Relay to all live subscribers.
        {
            let mut subs = self.browser_frame_subscribers.lock();
            if let Some(txs) = subs.get_mut(&vid) {
                txs.retain(|tx| tx.try_send(frame.clone()).is_ok());
            }
        }
        // Update GOP buffer: reset on keyframe, append otherwise.
        let mut bufs = self.browser_frame_buffers.lock();
        let buf = bufs.entry(vid).or_default();
        if frame.keyframe {
            buf.clear();
        }
        buf.push(frame);
        // Cap at 300 frames to prevent unbounded growth.
        if buf.len() > 300 {
            buf.drain(..100);
        }
    }

    /// Subscribe to live VP8 frames for a browser video stream.
    ///
    /// Returns the current GOP buffer (for decoder catch-up) and a receiver
    /// that delivers live frames going forward.
    pub fn subscribe_browser_frames(&self, vid: Vid) -> (Vec<VideoFrame>, mpsc::Receiver<VideoFrame>) {
        let (tx, rx) = mpsc::channel(256);
        // Register subscriber before reading the backlog to avoid missing frames.
        self.browser_frame_subscribers.lock().entry(vid).or_default().push(tx);
        let backlog = self.browser_frame_buffers.lock().get(&vid).cloned().unwrap_or_default();
        (backlog, rx)
    }

    /// Forward a mouse/keyboard input event to the sshx-browser controlling the given stream.
    pub fn send_browser_input(&self, vid: Vid, json: String) {
        let cmd = BrowserCommand {
            browser_command: Some(BrowserCmd::Input(InputEvent { json })),
        };
        if let Some(tx) = self.browser_cmd_senders.lock().get(&vid).cloned() {
            if let Err(e) = tx.try_send(Ok(cmd)) {
                warn!(%vid, "failed to send browser input: {e}");
            }
        }
    }

    /// Apply a partial metadata update to a single source file in memory.
    ///
    /// Updates the matching entry (by path) and notifies WebSocket clients.
    /// Returns the updated fields as a proto message to forward to the CLI.
    pub fn update_source_file_metadata(
        &self,
        path: &str,
        update: &crate::web::protocol::WsFileMetadataUpdate,
    ) -> sshx_core::proto::UpdateFileMetadata {
        let mut sf_lock = self.source_files.write();
        let mut changed = false;
        for file in &mut sf_lock.2 {
            if file.path == path {
                if let Some(ref ip) = update.image_path {
                    file.image_path = ip.clone();
                    changed = true;
                }
                if let Some(ref desc) = update.description {
                    file.description = desc.clone();
                    changed = true;
                }
                if let Some(w) = update.widget_w {
                    file.widget_w = w;
                    changed = true;
                }
                if let Some(h) = update.widget_h {
                    file.widget_h = h;
                    changed = true;
                }
                break;
            }
        }
        if changed {
            let updated = sf_lock.clone();
            drop(sf_lock);
            self.source_files_source.send_modify(|s| *s = updated);
        }
        sshx_core::proto::UpdateFileMetadata {
            path: path.to_string(),
            image_path: update.image_path.clone().unwrap_or_default(),
            description: update.description.clone().unwrap_or_default(),
            widget_w: update.widget_w.unwrap_or(0),
            widget_h: update.widget_h.unwrap_or(0),
        }
    }

    /// Return the current `image_path` for a source file by path, if any.
    pub fn get_source_file_image_path(&self, path: &str) -> Option<String> {
        let sf_lock = self.source_files.read();
        sf_lock.2.iter().find(|f| f.path == path).and_then(|f| {
            if f.image_path.is_empty() { None } else { Some(f.image_path.clone()) }
        })
    }

    /// Subscribe to source file updates (watch stream, delivers latest on connect).
    pub fn subscribe_source_files(&self) -> impl Stream<Item = (String, String, Vec<WsSourceFile>)> + Unpin {
        WatchStream::new(self.source_files_source.subscribe())
    }

    /// Broadcast a Claude Code event to all connected WebSocket clients,
    /// and store it in the ring buffer for later replay to reconnecting browsers.
    pub fn send_claude_event(&self, event: WsClaudeEvent) {
        {
            let mut buf = self.claude_events.lock();
            if buf.len() >= 200 {
                buf.pop_front();
            }
            buf.push_back(event.clone());
        }
        self.broadcast.send(WsServer::ClaudeEvent(event)).ok();
    }

    /// Return a snapshot of all buffered Claude events for replay on connect.
    pub fn list_claude_events(&self) -> Vec<WsClaudeEvent> {
        self.claude_events.lock().iter().cloned().collect()
    }

    /// Request the CLI to capture and return a context snapshot.
    ///
    /// Enqueues a `ContextSnapshotRequest` gRPC message to the CLI.
    pub fn request_context_snapshot(&self) {
        self.update_tx.try_send(ServerMessage::ContextSnapshotRequest(bytes::Bytes::new())).ok();
    }

    /// Broadcast a received context snapshot to all connected WebSocket clients.
    pub fn deliver_context_snapshot(&self, ansi: String) {
        self.broadcast.send(WsServer::ContextSnapshot(ansi)).ok();
    }

    /// Subscribe to canvas widget updates (watch stream, delivers latest on connect).
    pub fn subscribe_widgets(&self) -> impl Stream<Item = Vec<(Wid, WsWidget)>> + Unpin {
        WatchStream::new(self.widget_source.subscribe())
    }

    /// Return a snapshot of the current widget list.
    pub fn list_widgets(&self) -> Vec<(Wid, WsWidget)> {
        self.widgets
            .read()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }

    /// Add a new canvas widget.
    pub fn add_widget(&self, id: Wid, widget: WsWidget) -> Result<()> {
        use std::collections::hash_map::Entry::*;
        match self.widgets.write().entry(id) {
            Occupied(_) => bail!("widget already exists with id={id}"),
            Vacant(v) => {
                v.insert(widget.clone());
            }
        }
        self.widget_source.send_modify(|s| s.push((id, widget.clone())));
        self.broadcast.send(WsServer::WidgetDiff(id, Some(widget))).ok();
        Ok(())
    }

    /// Move an existing canvas widget to a new position.
    pub fn move_widget(&self, id: Wid, x: i32, y: i32) -> Result<()> {
        {
            let mut widgets = self.widgets.write();
            let w = widgets.get_mut(&id).context("widget not found")?;
            w.x = x;
            w.y = y;
        }
        self.widget_source.send_modify(|s| {
            if let Some(entry) = s.iter_mut().find(|(wid, _)| *wid == id) {
                entry.1.x = x;
                entry.1.y = y;
            }
        });
        let updated = self.widgets.read().get(&id).cloned();
        if let Some(w) = updated {
            self.broadcast.send(WsServer::WidgetDiff(id, Some(w))).ok();
        }
        Ok(())
    }

    /// Resize an existing canvas widget.
    pub fn resize_widget(&self, id: Wid, w: u32, h: u32) -> Result<()> {
        {
            let mut widgets = self.widgets.write();
            let widget = widgets.get_mut(&id).context("widget not found")?;
            widget.w = w;
            widget.h = h;
        }
        self.widget_source.send_modify(|s| {
            if let Some(entry) = s.iter_mut().find(|(wid, _)| *wid == id) {
                entry.1.w = w;
                entry.1.h = h;
            }
        });
        let updated = self.widgets.read().get(&id).cloned();
        if let Some(widget) = updated {
            self.broadcast.send(WsServer::WidgetDiff(id, Some(widget))).ok();
        }
        Ok(())
    }

    /// Set the collapsed state of a canvas widget and broadcast the change.
    pub fn set_widget_collapsed(&self, id: Wid, collapsed: bool) -> Result<()> {
        {
            let mut widgets = self.widgets.write();
            let widget = widgets.get_mut(&id).context("widget not found")?;
            widget.collapsed = collapsed;
        }
        self.widget_source.send_modify(|s| {
            if let Some(entry) = s.iter_mut().find(|(wid, _)| *wid == id) {
                entry.1.collapsed = collapsed;
            }
        });
        let updated = self.widgets.read().get(&id).cloned();
        if let Some(widget) = updated {
            self.broadcast.send(WsServer::WidgetDiff(id, Some(widget))).ok();
        }
        Ok(())
    }

    /// Set a user-defined name on a canvas widget.
    pub fn set_widget_name(&self, id: Wid, name: String) -> Result<()> {
        {
            let mut widgets = self.widgets.write();
            let widget = widgets.get_mut(&id).context("widget not found")?;
            widget.name = if name.is_empty() { None } else { Some(name.clone()) };
        }
        self.widget_source.send_modify(|s| {
            if let Some(entry) = s.iter_mut().find(|(wid, _)| *wid == id) {
                entry.1.name = if name.is_empty() { None } else { Some(name.clone()) };
            }
        });
        let updated = self.widgets.read().get(&id).cloned();
        if let Some(widget) = updated {
            self.broadcast.send(WsServer::WidgetDiff(id, Some(widget))).ok();
        }
        Ok(())
    }

    /// Replace the `kind` field of an existing widget and broadcast the update.
    pub fn update_widget_kind(&self, id: Wid, kind: crate::web::protocol::WsWidgetKind) -> Result<()> {
        {
            let mut widgets = self.widgets.write();
            let widget = widgets.get_mut(&id).context("widget not found")?;
            widget.kind = kind.clone();
        }
        self.widget_source.send_modify(|s| {
            if let Some(entry) = s.iter_mut().find(|(wid, _)| *wid == id) {
                entry.1.kind = kind;
            }
        });
        let updated = self.widgets.read().get(&id).cloned();
        if let Some(widget) = updated {
            self.broadcast.send(WsServer::WidgetDiff(id, Some(widget))).ok();
        }
        Ok(())
    }

    /// Update an image widget's URL and name atomically.
    pub fn update_image_widget(&self, id: Wid, new_url: String, new_name: String) -> Result<()> {
        {
            let mut widgets = self.widgets.write();
            let widget = widgets.get_mut(&id).context("widget not found")?;
            if let WsWidgetKind::Image { ref mut url, .. } = widget.kind {
                *url = new_url.clone();
            }
            widget.name = if new_name.is_empty() { None } else { Some(new_name.clone()) };
        }
        self.widget_source.send_modify(|s| {
            if let Some(entry) = s.iter_mut().find(|(wid, _)| *wid == id) {
                if let WsWidgetKind::Image { ref mut url, .. } = entry.1.kind {
                    *url = new_url;
                }
                entry.1.name = if new_name.is_empty() { None } else { Some(new_name) };
            }
        });
        let updated = self.widgets.read().get(&id).cloned();
        if let Some(widget) = updated {
            self.broadcast.send(WsServer::WidgetDiff(id, Some(widget))).ok();
        }
        Ok(())
    }

    /// Return true if a claudeFeed widget with the given instance_id already exists.
    pub fn claude_feed_exists(&self, instance_id: &str) -> bool {
        use crate::web::protocol::WsWidgetKind;
        self.widgets.read().values().any(|w| {
            matches!(&w.kind, WsWidgetKind::ClaudeFeed { instance_id: iid } if iid == instance_id)
        })
    }

    /// Return the Claude session instance_id for a claudeFeed widget, if any.
    pub fn get_widget_instance_id(&self, id: Wid) -> Option<String> {
        use crate::web::protocol::WsWidgetKind;
        self.widgets.read().get(&id).and_then(|w| {
            if let WsWidgetKind::ClaudeFeed { instance_id } = &w.kind {
                Some(instance_id.clone())
            } else {
                None
            }
        })
    }

    /// Apply a bulk map of instanceId → name from CLI startup state.
    /// Sets the name on any claudeFeed widget whose instanceId is in the map.
    pub fn apply_widget_names(&self, names: &std::collections::HashMap<String, String>) {
        use crate::web::protocol::WsWidgetKind;
        let ids: Vec<(Wid, String)> = {
            let widgets = self.widgets.read();
            widgets
                .iter()
                .filter_map(|(wid, w)| {
                    if let WsWidgetKind::ClaudeFeed { instance_id } = &w.kind {
                        names.get(instance_id.as_str()).map(|n| (*wid, n.clone()))
                    } else {
                        None
                    }
                })
                .collect()
        };
        for (wid, name) in ids {
            self.set_widget_name(wid, name).ok();
        }
    }

    /// Remove a canvas widget by ID.
    pub fn remove_widget(&self, id: Wid) -> Result<()> {
        match self.widgets.write().remove(&id) {
            Some(_) => {
                self.widget_source.send_modify(|s| s.retain(|(wid, _)| *wid != id));
                self.broadcast.send(WsServer::WidgetDiff(id, None)).ok();
                Ok(())
            }
            None => bail!("widget with id={id} does not exist"),
        }
    }

    /// Return a snapshot of all shell names.
    pub fn list_shell_names(&self) -> Vec<(Sid, String)> {
        self.shell_names.read().iter().map(|(k, v)| (*k, v.clone())).collect()
    }

    /// Set a human-readable name for a shell window and broadcast the change.
    pub fn set_shell_name(&self, id: Sid, name: String) {
        self.shell_names.write().insert(id, name.clone());
        self.broadcast.send(WsServer::ShellNameDiff(id, name)).ok();
    }

    /// Register a backend client heartbeat, refreshing the timestamp.
    pub fn access(&self) {
        *self.last_accessed.lock() = Instant::now();
    }

    /// Returns the timestamp of the last backend client activity.
    pub fn last_accessed(&self) -> Instant {
        *self.last_accessed.lock()
    }

    /// Forward a describe-files request to the CLI client.
    pub fn request_describe_files(&self, paths: Vec<String>) {
        use sshx_core::proto::DescribeFilesRequest;
        let msg = ServerMessage::DescribeFiles(DescribeFilesRequest { paths });
        self.update_tx.try_send(msg).ok();
    }

    /// Access the sender of the client message channel for this session.
    pub fn update_tx(&self) -> &async_channel::Sender<ServerMessage> {
        &self.update_tx
    }

    /// Access the receiver of the client message channel for this session.
    pub fn update_rx(&self) -> &async_channel::Receiver<ServerMessage> {
        &self.update_rx
    }

    /// Mark the session as requiring an immediate storage sync.
    ///
    /// This is needed for consistency when creating new shells, removing old
    /// shells, or updating the ID counter. If these operations are lost in a
    /// server restart, then the snapshot that contains them would be invalid
    /// compared to the current backend client state.
    ///
    /// Note that it is not necessary to do this all the time though, since that
    /// would put too much pressure on the database. Lost terminal data is
    /// already re-synchronized periodically.
    pub fn sync_now(&self) {
        self.sync_notify.notify_one();
    }

    /// Resolves when the session has been marked for an immediate sync.
    pub async fn sync_now_wait(&self) {
        self.sync_notify.notified().await
    }

    /// Send a termination signal to exit this session.
    pub fn shutdown(&self) {
        self.shutdown.shutdown()
    }

    /// Resolves when the session has received a shutdown signal.
    pub async fn terminated(&self) {
        self.shutdown.wait().await
    }

    // ---- HTTP Tunnel ----

    /// Allocate the next unique tunnel ID.
    pub fn next_tunnel_id(&self) -> u32 {
        self.next_tunnel_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Register a response channel for a pending HTTP tunnel request.
    pub fn register_tunnel_response(
        &self,
        tunnel_id: u32,
        tx: mpsc::UnboundedSender<sshx_core::proto::HttpTunnelResponse>,
    ) {
        self.tunnel_responses.insert(tunnel_id, tx);
    }

    /// Remove a tunnel response channel.
    pub fn remove_tunnel_response(&self, tunnel_id: u32) {
        self.tunnel_responses.remove(&tunnel_id);
    }

    /// Deliver an HTTP tunnel response chunk from the CLI.
    pub fn deliver_tunnel_response(&self, resp: sshx_core::proto::HttpTunnelResponse) {
        let tunnel_id = resp.tunnel_id;
        if let Some(tx) = self.tunnel_responses.get(&tunnel_id) {
            tx.send(resp).ok();
        } else {
            debug!("tunnel response for unknown tunnel_id={tunnel_id}");
        }
    }

    /// Register a WebSocket tunnel frame channel.
    pub fn register_ws_tunnel(
        &self,
        tunnel_id: u32,
        tx: mpsc::UnboundedSender<sshx_core::proto::WsTunnelFrame>,
    ) {
        self.ws_tunnels.insert(tunnel_id, tx);
    }

    /// Remove a WebSocket tunnel.
    pub fn remove_ws_tunnel(&self, tunnel_id: u32) {
        self.ws_tunnels.remove(&tunnel_id);
    }

    /// Deliver a WebSocket tunnel frame from the CLI.
    pub fn deliver_ws_tunnel_frame(&self, frame: sshx_core::proto::WsTunnelFrame) {
        let tunnel_id = frame.tunnel_id;
        if let Some(tx) = self.ws_tunnels.get(&tunnel_id) {
            tx.send(frame).ok();
        }
    }

    /// Set whether the IDE is available on this session's CLI client.
    pub fn set_ide_available(&self, available: bool) {
        self.ide_available
            .store(available, std::sync::atomic::Ordering::Relaxed);
        self.broadcast.send(WsServer::IdeAvailable(available)).ok();
    }

    /// Check if the IDE is available.
    pub fn ide_available(&self) -> bool {
        self.ide_available
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    // --- IDE state sync methods ---

    /// Return a snapshot of all IDE states (keyed by Wid).
    pub fn list_ide_states(&self) -> Vec<(Wid, WsIdeState)> {
        self.ide_states
            .read()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }

    /// Update an IDE widget's state and broadcast the diff.
    ///
    /// Also relays the state to the CLI via gRPC so the VS Code extension
    /// can receive remote state via SSE. Includes `ide_id` so the extension
    /// can filter out its own echoes.
    pub fn update_ide_state(&self, wid: Wid, state: WsIdeState) {
        self.ide_states.write().insert(wid, state.clone());
        self.broadcast
            .send(WsServer::IdeStateDiff(wid, Some(state.clone())))
            .ok();

        // Relay IDE state to the CLI extension with the ide_id.
        if let Ok(json) = serde_json::to_vec(&state) {
            use sshx_core::proto::{server_update::ServerMessage, IdeStateUpdate};
            let msg = ServerMessage::IdeState(IdeStateUpdate {
                json: json.into(),
                ide_id: wid.0,
            });
            self.update_tx.try_send(msg).ok();
        }
    }

    /// Remove an IDE widget's state and broadcast the removal.
    pub fn remove_ide_state(&self, wid: Wid) {
        if self.ide_states.write().remove(&wid).is_some() {
            self.broadcast
                .send(WsServer::IdeStateDiff(wid, None))
                .ok();
        }
    }

    /// Clear all IDE states (called when CLI disconnects).
    pub fn clear_ide_states(&self) {
        let wids: Vec<Wid> = self.ide_states.read().keys().copied().collect();
        for wid in wids {
            self.remove_ide_state(wid);
        }
    }

    /// Return the current edit lock state.
    pub fn get_edit_lock(&self) -> WsEditLock {
        self.edit_lock.read().clone()
    }

    /// Try to acquire the edit lock for a user on a file.
    ///
    /// Returns `Ok(lock)` if granted, `Err(msg)` if held by someone else.
    /// Auto-expires stale locks.
    pub fn request_edit_lock(&self, uid: Uid, file: String) -> Result<WsEditLock> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let lock_duration_ms = 5_000; // 5 seconds

        let mut lock = self.edit_lock.write();

        // Check if the lock is held and not expired.
        if let Some(holder) = lock.holder {
            if holder != uid && lock.expires_at > now_ms {
                bail!(
                    "edit lock held by user {} on {:?} (expires in {}ms)",
                    holder,
                    lock.file,
                    lock.expires_at.saturating_sub(now_ms)
                );
            }
        }

        // Grant or renew the lock.
        lock.holder = Some(uid);
        lock.file = Some(file);
        lock.expires_at = now_ms + lock_duration_ms;
        let new_lock = lock.clone();
        drop(lock);

        self.broadcast
            .send(WsServer::EditLock(new_lock.clone()))
            .ok();
        // Also relay lock status to the CLI extension.
        self.relay_edit_lock_to_cli(&new_lock);
        Ok(new_lock)
    }

    /// Release the edit lock if held by the given user.
    pub fn release_edit_lock(&self, uid: Uid) {
        let mut lock = self.edit_lock.write();
        if lock.holder == Some(uid) {
            *lock = WsEditLock::default();
            let new_lock = lock.clone();
            drop(lock);
            self.broadcast.send(WsServer::EditLock(new_lock.clone())).ok();
            self.relay_edit_lock_to_cli(&new_lock);
        }
    }

    /// Send edit lock status to the CLI via the gRPC channel.
    fn relay_edit_lock_to_cli(&self, lock: &WsEditLock) {
        if let Ok(json) = serde_json::to_vec(lock) {
            use sshx_core::proto::server_update::ServerMessage;
            self.update_tx
                .try_send(ServerMessage::EditLockStatus(json.into()))
                .ok();
        }
    }

    /// Release the edit lock unconditionally for a disconnecting user.
    pub fn force_release_edit_lock(&self, uid: Uid) {
        self.release_edit_lock(uid);
    }
}
