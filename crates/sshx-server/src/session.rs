//! Core logic for sshx sessions, independent of message transport.

use std::collections::{HashMap, VecDeque};
use std::ops::DerefMut;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use parking_lot::{Mutex, RwLock, RwLockWriteGuard};
use sshx_core::{
    proto::{server_update::ServerMessage, SequenceNumbers},
    IdCounter, Nid, Sid, Uid, Wid,
};
use tokio::sync::{broadcast, watch, Notify};
use tokio::time::Instant;
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream, WatchStream};
use tokio_stream::Stream;
use tracing::{debug, warn};

use crate::utils::Shutdown;
use crate::web::protocol::{WsClaudeEvent, WsComponentGraph, WsNote, WsServer, WsSourceFile, WsUser, WsWidget, WsWinsize};

mod snapshot;

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
    /// The String is the workspace root basename (e.g. "my-project").
    source_files: RwLock<(String, Vec<WsSourceFile>)>,

    /// Watch channel source for source file snapshots; clients get the latest on connect.
    source_files_source: watch::Sender<(String, Vec<WsSourceFile>)>,

    /// In-memory widget state.
    widgets: RwLock<HashMap<Wid, WsWidget>>,

    /// Watch channel source for the widget list; clients get a snapshot on connect.
    widget_source: watch::Sender<Vec<(Wid, WsWidget)>>,

    /// Human-readable names for shell windows, set by browser clients.
    shell_names: RwLock<HashMap<Sid, String>>,

    /// Ring buffer of the last 200 Claude events for browser reconnect replay.
    claude_events: Mutex<VecDeque<WsClaudeEvent>>,

    /// Latest component graph sent by the CLI workspace analyzer.
    component_graph: RwLock<Option<WsComponentGraph>>,

    /// Triggered from metadata events when an immediate snapshot is needed.
    sync_notify: Notify,

    /// Set when this session has been closed and removed.
    shutdown: Shutdown,
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
            source_files: RwLock::new((String::new(), Vec::new())),
            source_files_source: watch::channel((String::new(), Vec::new())).0,
            widgets: RwLock::new(HashMap::new()),
            widget_source: watch::channel(Vec::new()).0,
            shell_names: RwLock::new(HashMap::new()),
            claude_events: Mutex::new(VecDeque::new()),
            component_graph: RwLock::new(None),
            sync_notify: Notify::new(),
            shutdown: Shutdown::new(),
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

    /// Replace the stored source file metadata and notify all WebSocket clients.
    pub fn update_source_files(&self, root: String, files: Vec<WsSourceFile>) {
        *self.source_files.write() = (root.clone(), files.clone());
        self.source_files_source.send_modify(|s| *s = (root, files));
    }

    /// Return a snapshot of the current source file metadata.
    pub fn list_source_files(&self) -> (String, Vec<WsSourceFile>) {
        self.source_files.read().clone()
    }

    /// Store a new component graph received from the CLI workspace analyzer.
    ///
    /// Broadcasts `WsServer::ComponentGraph` to all connected WebSocket clients.
    pub fn update_component_graph(&self, graph: WsComponentGraph) {
        *self.component_graph.write() = Some(graph.clone());
        let _ = self.broadcast.send(WsServer::ComponentGraph(graph));
    }

    /// Return the current component graph, or `None` if not yet received.
    pub fn get_component_graph(&self) -> Option<WsComponentGraph> {
        self.component_graph.read().clone()
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
        for file in &mut sf_lock.1 {
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

    /// Subscribe to source file updates (watch stream, delivers latest on connect).
    pub fn subscribe_source_files(&self) -> impl Stream<Item = (String, Vec<WsSourceFile>)> + Unpin {
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
}
