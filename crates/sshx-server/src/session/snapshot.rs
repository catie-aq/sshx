//! Snapshot and restore sessions from serialized state.

use std::collections::{BTreeMap, HashMap};

use anyhow::{ensure, Context, Result};
use bytes::Bytes;
use prost::Message;
use serde::{Deserialize, Serialize};
use sshx_core::{
    proto::{SerializedSession, SerializedShell},
    AllCounterValues, Did, Nid, Sid, Slid, Tid, Uid, Wid,
};

use super::{Metadata, Session, State};
use crate::web::protocol::{WsDrawing, WsNote, WsSlide, WsTextBlock, WsWidget, WsWinsize};
use sshx_core::proto::{server_update::ServerMessage, NewShell};

/// Persist at most this many bytes of output in storage, per shell.
const SHELL_SNAPSHOT_BYTES: u64 = 1 << 15; // 32 KiB

const MAX_SNAPSHOT_SIZE: usize = 1 << 22; // 4 MiB

/// A shell's terminal data for JSON persistence.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ShellSnapshot {
    /// Shell position and size.
    pub winsize: WsWinsize,
    /// Base64-encoded terminal data chunks (encrypted).
    pub data: Vec<String>,
    /// Sequence number.
    pub seqnum: u64,
    /// Number of pruned data chunks before data[0].
    pub chunk_offset: u64,
    /// Number of bytes in pruned data chunks.
    pub byte_offset: u64,
    /// Whether the shell is closed.
    pub closed: bool,
}

/// JSON-serializable full session snapshot for `.sshx/session.json`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct JsonSessionSnapshot {
    /// All notes.
    pub notes: HashMap<u32, WsNote>,
    /// All text blocks.
    pub text_blocks: HashMap<u32, WsTextBlock>,
    /// All drawing strokes.
    pub drawings: HashMap<u32, WsDrawing>,
    /// All slides.
    pub slides: HashMap<u32, WsSlide>,
    /// All widgets.
    pub widgets: HashMap<u32, WsWidget>,
    /// All shells with terminal data and position.
    pub shells: HashMap<u32, ShellSnapshot>,
    /// Human-readable shell names.
    pub shell_names: HashMap<u32, String>,
    /// Counter state for ID generation.
    pub counters: AllCounterValues,
}

impl Session {
    /// Snapshot the session, returning a compressed representation (protobuf).
    pub fn snapshot(&self) -> Result<Vec<u8>> {
        let ids = self.counter.get_current_values();
        let winsizes: BTreeMap<Sid, WsWinsize> = self.source.borrow().iter().cloned().collect();
        let message = SerializedSession {
            encrypted_zeros: self.metadata().encrypted_zeros.clone(),
            shells: self
                .shells
                .read()
                .iter()
                .map(|(sid, shell)| {
                    // Prune off data until its total length is at most `SHELL_SNAPSHOT_BYTES`.
                    let mut prefix = 0;
                    let mut chunk_offset = shell.chunk_offset;
                    let mut byte_offset = shell.byte_offset;

                    for i in 0..shell.data.len() {
                        if shell.seqnum - byte_offset > SHELL_SNAPSHOT_BYTES {
                            prefix += 1;
                            chunk_offset += 1;
                            byte_offset += shell.data[i].len() as u64;
                        } else {
                            break;
                        }
                    }

                    let winsize = winsizes.get(sid).cloned().unwrap_or_default();
                    let shell = SerializedShell {
                        seqnum: shell.seqnum,
                        data: shell.data[prefix..].to_vec(),
                        chunk_offset,
                        byte_offset,
                        closed: shell.closed,
                        winsize_x: winsize.x,
                        winsize_y: winsize.y,
                        winsize_rows: winsize.rows.into(),
                        winsize_cols: winsize.cols.into(),
                    };
                    (sid.0, shell)
                })
                .collect(),
            next_sid: ids.0 .0,
            next_uid: ids.1 .0,
            name: self.metadata().name.clone(),
            write_password_hash: self.metadata().write_password_hash.clone(),
        };
        let data = message.encode_to_vec();
        ensure!(data.len() < MAX_SNAPSHOT_SIZE, "snapshot too large");
        Ok(zstd::bulk::compress(&data, 3)?)
    }

    /// Restore the session from a previous compressed snapshot (protobuf).
    pub fn restore(data: &[u8]) -> Result<Self> {
        let data = zstd::bulk::decompress(data, MAX_SNAPSHOT_SIZE)?;
        let message = SerializedSession::decode(&*data)?;

        let metadata = Metadata {
            encrypted_zeros: message.encrypted_zeros,
            name: message.name,
            write_password_hash: message.write_password_hash,
        };

        let session = Self::new(metadata);
        let mut shells = session.shells.write();
        let mut winsizes = Vec::new();
        for (sid, shell) in message.shells {
            winsizes.push((
                Sid(sid),
                WsWinsize {
                    x: shell.winsize_x,
                    y: shell.winsize_y,
                    rows: shell.winsize_rows.try_into().context("rows overflow")?,
                    cols: shell.winsize_cols.try_into().context("cols overflow")?,
                },
            ));
            let shell = State {
                seqnum: shell.seqnum,
                data: shell.data,
                chunk_offset: shell.chunk_offset,
                byte_offset: shell.byte_offset,
                closed: shell.closed,
                notify: Default::default(),
            };
            shells.insert(Sid(sid), shell);
        }
        drop(shells);
        session.source.send_replace(winsizes);
        session
            .counter
            .set_current_values(Sid(message.next_sid), Uid(message.next_uid));

        Ok(session)
    }

    /// Create a JSON snapshot of the full canvas state for CLI persistence.
    pub fn json_snapshot(&self) -> JsonSessionSnapshot {
        use base64::prelude::{Engine as _, BASE64_STANDARD};

        let winsizes: BTreeMap<Sid, WsWinsize> = self.source.borrow().iter().cloned().collect();

        let shells: HashMap<u32, ShellSnapshot> = self
            .shells
            .read()
            .iter()
            .map(|(sid, shell)| {
                // Prune data to fit within SHELL_SNAPSHOT_BYTES.
                let mut prefix = 0;
                let mut chunk_offset = shell.chunk_offset;
                let mut byte_offset = shell.byte_offset;
                for i in 0..shell.data.len() {
                    if shell.seqnum - byte_offset > SHELL_SNAPSHOT_BYTES {
                        prefix += 1;
                        chunk_offset += 1;
                        byte_offset += shell.data[i].len() as u64;
                    } else {
                        break;
                    }
                }
                let data: Vec<String> = shell.data[prefix..]
                    .iter()
                    .map(|b| BASE64_STANDARD.encode(b))
                    .collect();
                let winsize = winsizes.get(sid).cloned().unwrap_or_default();
                (
                    sid.0,
                    ShellSnapshot {
                        winsize,
                        data,
                        seqnum: shell.seqnum,
                        chunk_offset,
                        byte_offset,
                        closed: shell.closed,
                    },
                )
            })
            .collect();

        let notes: HashMap<u32, WsNote> = self
            .notes
            .read()
            .iter()
            .map(|(k, v)| (k.0, v.clone()))
            .collect();

        let text_blocks: HashMap<u32, WsTextBlock> = self
            .text_blocks
            .read()
            .iter()
            .map(|(k, v)| (k.0, v.clone()))
            .collect();

        let drawings: HashMap<u32, WsDrawing> = self
            .drawings
            .read()
            .iter()
            .map(|(k, v)| (k.0, v.clone()))
            .collect();

        let slides: HashMap<u32, WsSlide> = self
            .slides
            .read()
            .iter()
            .map(|(k, v)| (k.0, v.clone()))
            .collect();

        let widgets: HashMap<u32, WsWidget> = self
            .widgets
            .read()
            .iter()
            .map(|(k, v)| (k.0, v.clone()))
            .collect();

        let shell_names: HashMap<u32, String> = self
            .shell_names
            .read()
            .iter()
            .map(|(k, v)| (k.0, v.clone()))
            .collect();

        let counters = self.counter.get_all_values();

        JsonSessionSnapshot {
            notes,
            text_blocks,
            drawings,
            slides,
            widgets,
            shells,
            shell_names,
            counters,
        }
    }

    /// Restore canvas state from a JSON snapshot (notes, text blocks, drawings, slides, widgets, shell names).
    /// Shell terminal data is also restored so the CLI can re-populate shells.
    pub fn restore_from_json(&self, snap: &JsonSessionSnapshot) {
        use base64::prelude::{Engine as _, BASE64_STANDARD};

        // Restore notes.
        {
            let mut notes = self.notes.write();
            for (&id, note) in &snap.notes {
                notes.insert(Nid(id), note.clone());
            }
        }
        self.notes_source.send_modify(|s| {
            for (&id, note) in &snap.notes {
                s.push((Nid(id), note.clone()));
            }
        });

        // Restore text blocks.
        {
            let mut text_blocks = self.text_blocks.write();
            for (&id, block) in &snap.text_blocks {
                text_blocks.insert(Tid(id), block.clone());
            }
        }
        self.text_blocks_source.send_modify(|s| {
            for (&id, block) in &snap.text_blocks {
                s.push((Tid(id), block.clone()));
            }
        });

        // Restore drawings.
        {
            let mut drawings = self.drawings.write();
            for (&id, drawing) in &snap.drawings {
                drawings.insert(Did(id), drawing.clone());
            }
        }
        self.drawings_source.send_modify(|s| {
            for (&id, drawing) in &snap.drawings {
                s.push((Did(id), drawing.clone()));
            }
        });

        // Restore slides.
        {
            let mut slides = self.slides.write();
            for (&id, slide) in &snap.slides {
                slides.insert(Slid(id), slide.clone());
            }
        }

        // Restore all widgets, including IDE editor widgets (which will reappear
        // when loading a previous workspace snapshot).
        let restorable_widgets: Vec<(u32, WsWidget)> = snap
            .widgets
            .iter()
            .map(|(&id, w)| (id, w.clone()))
            .collect();
        {
            let mut widgets = self.widgets.write();
            for (id, widget) in &restorable_widgets {
                widgets.insert(Wid(*id), widget.clone());
            }
        }
        self.widget_source.send_modify(|s| {
            for (id, widget) in &restorable_widgets {
                s.push((Wid(*id), widget.clone()));
            }
        });

        // Restore shell names.
        {
            let mut shell_names = self.shell_names.write();
            for (&id, name) in &snap.shell_names {
                shell_names.insert(Sid(id), name.clone());
            }
        }

        // Restore counters first, so new shell IDs don't collide.
        self.counter.set_all_values(&snap.counters);

        // Restore shell data: closed shells are kept as-is for history;
        // non-closed shells are NOT restored (no backing PTY exists).
        // Instead, we queue CreateShell messages so the CLI spawns fresh
        // terminals at the same positions.
        {
            let mut shells = self.shells.write();
            let mut shells_to_recreate: Vec<(i32, i32)> = Vec::new();
            for (&id, shell_snap) in &snap.shells {
                if !shell_snap.closed {
                    // Remember position for re-creation, skip restoring the dead shell.
                    shells_to_recreate.push((shell_snap.winsize.x, shell_snap.winsize.y));
                    continue;
                }
                // Restore closed shells to the HashMap for history only.
                // Do NOT add them to `source` — they have no backing PTY
                // and would cause broken "Remote Terminal" windows in the browser.
                let data: Vec<Bytes> = shell_snap
                    .data
                    .iter()
                    .filter_map(|b64| BASE64_STANDARD.decode(b64).ok().map(Bytes::from))
                    .collect();
                let state = State {
                    seqnum: shell_snap.seqnum,
                    data,
                    chunk_offset: shell_snap.chunk_offset,
                    byte_offset: shell_snap.byte_offset,
                    closed: shell_snap.closed,
                    notify: Default::default(),
                };
                shells.insert(Sid(id), state);
            }
            drop(shells);

            // Queue CreateShell messages for the CLI to spawn fresh PTYs.
            for (x, y) in shells_to_recreate {
                let id = self.counter.next_sid();
                let msg = ServerMessage::CreateShell(NewShell { id: id.0, x, y });
                self.update_tx.try_send(msg).ok();
            }
        }
    }
}
