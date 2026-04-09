//! Serializable types sent and received by the web server.

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sshx_core::{Did, Nid, Sid, Slid, Tid, Uid, Vid, Wid};

/// Serde helpers that accept both CBOR integers and floats for f32 fields.
/// cbor-x (JavaScript) encodes whole-number floats (e.g. 2.0, 1.0) as CBOR
/// integers, but ciborium's default f32 deserializer rejects them.
mod flexible_f32 {
    use serde::de::{Deserializer, SeqAccess, Visitor};

    /// Newtype wrapper with a Deserialize that accepts both integers and floats.
    struct Num(f32);

    impl<'de> serde::Deserialize<'de> for Num {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = Num;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("a number (float or integer)")
                }
                fn visit_f32<E: serde::de::Error>(self, v: f32) -> Result<Num, E> {
                    Ok(Num(v))
                }
                fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Num, E> {
                    Ok(Num(v as f32))
                }
                fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Num, E> {
                    Ok(Num(v as f32))
                }
                fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Num, E> {
                    Ok(Num(v as f32))
                }
            }
            d.deserialize_any(V)
        }
    }

    pub fn scalar<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        use serde::Deserialize as _;
        Num::deserialize(d).map(|n| n.0)
    }

    pub fn vec<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<f32>, D::Error> {
        struct SeqV;
        impl<'de> Visitor<'de> for SeqV {
            type Value = Vec<f32>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a sequence of numbers")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<f32>, A::Error> {
                let mut out = Vec::new();
                while let Some(Num(v)) = seq.next_element::<Num>()? {
                    out.push(v);
                }
                Ok(out)
            }
        }
        d.deserialize_seq(SeqV)
    }
}

/// Real-time message conveying the position and size of a terminal.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsWinsize {
    /// The top-left x-coordinate of the window, offset from origin.
    pub x: i32,
    /// The top-left y-coordinate of the window, offset from origin.
    pub y: i32,
    /// The number of rows in the window.
    pub rows: u16,
    /// The number of columns in the terminal.
    pub cols: u16,
}

impl Default for WsWinsize {
    fn default() -> Self {
        WsWinsize {
            x: 0,
            y: 0,
            rows: 24,
            cols: 80,
        }
    }
}

/// Real-time message representing a sticky note on the canvas.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsNote {
    /// The x-coordinate of the note on the canvas.
    pub x: i32,
    /// The y-coordinate of the note on the canvas.
    pub y: i32,
    /// The text content of the note (HTML rich text).
    pub text: String,
    /// Color key: "yellow" | "pink" | "blue" | "green" | "purple"
    pub color: String,
    /// Whether the note is pinned (position locked).
    pub pinned: bool,
    /// Width of the note in pixels (0 = default 260px).
    #[serde(default)]
    pub w: u32,
    /// Height of the note in pixels (0 = auto).
    #[serde(default)]
    pub h: u32,
    /// Font family ID (e.g. "inter", "caveat"). Empty = default.
    #[serde(default)]
    pub font: String,
}

/// A text block on the canvas (FigJam-style rich text).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsTextBlock {
    /// The x-coordinate of the text block on the canvas.
    pub x: i32,
    /// The y-coordinate of the text block on the canvas.
    pub y: i32,
    /// HTML content (bold, links, colored spans, lists).
    pub content: String,
    /// Font size key: "xs" | "sm" | "md" | "lg" | "xl".
    pub font_size: String,
    /// Hex color like "#ffffff".
    pub color: String,
    /// Text alignment: "left" | "center" | "right".
    pub align: String,
    /// Font family ID (e.g. "inter", "caveat"). Empty = default.
    #[serde(default)]
    pub font: String,
}

/// Metadata for a source file, extracted by the CLI workspace analyzer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsSourceFile {
    /// Relative path from the workspace root.
    pub path: String,
    /// File kind: "component"|"hook"|"utility"|"config"|"type"|"other"
    pub kind: String,
    /// Local (relative) imports made by this file.
    pub local_imports: Vec<String>,
    /// Library (npm/crate) imports made by this file.
    pub libraries: Vec<String>,
    /// Exported symbols from this file.
    pub exports: Vec<String>,
    /// Files that import this file.
    pub imported_by: Vec<String>,
    /// AI-generated one-sentence description (≤120 chars).
    pub description: String,
    /// Total number of lines in the file.
    pub line_count: u32,
    /// ISO 8601 last-modified timestamp.
    pub last_modified: String,
    /// First 6144 characters of the file for in-browser preview.
    #[serde(default)]
    pub content: String,
    /// URL/path of an illustration image attached to this file card.
    #[serde(default)]
    pub image_path: String,
    /// Last known widget width in pixels (0 = use default).
    #[serde(default)]
    pub widget_w: u32,
    /// Last known widget height in pixels (0 = use default).
    #[serde(default)]
    pub widget_h: u32,
}

/// Partial metadata update for a source file, sent from the browser.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WsFileMetadataUpdate {
    /// New image URL/path (None = no change).
    pub image_path: Option<String>,
    /// Updated description text (None = no change).
    pub description: Option<String>,
    /// Updated widget width (None = no change).
    pub widget_w: Option<u32>,
    /// Updated widget height (None = no change).
    pub widget_h: Option<u32>,
    /// User-defined filename to save the image as on the CLI (None = no push).
    pub image_name: Option<String>,
}

/// A real-time event from a running Claude Code session.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsClaudeEvent {
    /// Event kind: "tool_use"|"tool_result"|"user_message"|"assistant_message"
    pub kind: String,
    /// Tool name (only present for tool_use events).
    pub tool: Option<String>,
    /// Text or JSON input (truncated to 500 chars).
    pub content: String,
    /// ISO 8601 timestamp of the event.
    pub timestamp: String,
    /// The Claude session ID this event belongs to.
    pub session_id: String,
    /// Input token count from the model usage field (assistant events only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    /// Output token count from the model usage field (assistant events only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
    /// Cache-read token count (prompt-cache hit tokens).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u32>,
    /// Cache-creation token count (tokens written into the prompt cache).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u32>,
}

/// The content/kind of a canvas widget.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum WsWidgetKind {
    /// A file-tree explorer panel.
    FileTree {
        /// The workspace root path being displayed.
        root: String,
    },
    /// A flippable card showing file metadata and code.
    FileCard {
        /// The file path this card represents.
        path: String,
    },
    /// A Claude Code activity feed panel.
    ClaudeFeed {
        /// The Claude Code session ID this feed tracks.
        #[serde(rename = "instanceId")]
        instance_id: String,
    },
    /// A pasted or uploaded image on the canvas.
    Image {
        /// URL of the image (served from /uploads/...).
        url: String,
        /// Optional alt text / filename.
        alt: String,
    },
    /// App observability overlay panel.
    AppOverlay {
        /// URL of the observed application.
        url: String,
        /// Whether the overlay can open file cards in SSHX.
        #[serde(rename = "allowOpenFile")]
        allow_open_file: bool,
        /// Whether the overlay can trigger Claude Code actions.
        #[serde(rename = "allowOpenClaude")]
        allow_open_claude: bool,
    },
    /// Embedded IDE editor (OpenVSCode Server in an iframe).
    IdeEditor {
        /// Human-readable workspace label.
        #[serde(rename = "workspaceLabel")]
        workspace_label: String,
        /// Unique IDE instance ID. Encoded in the proxy URL `/ide/s/{session}/{ide_id}/`.
        /// Defaults to 0 for legacy widgets created before multi-IDE support.
        #[serde(default, rename = "ideId")]
        ide_id: u32,
    },
}

/// A generic canvas widget with position and content.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsWidget {
    /// X position on the canvas.
    pub x: i32,
    /// Y position on the canvas.
    pub y: i32,
    /// Width in pixels.
    pub w: u32,
    /// Height in pixels.
    pub h: u32,
    /// The kind and content of this widget.
    pub kind: WsWidgetKind,
    /// Whether this widget is collapsed to a compact mini view.
    #[serde(default)]
    pub collapsed: bool,
    /// User-set display name for this widget (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Real-time message providing information about a user.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsUser {
    /// The user's display name.
    pub name: String,
    /// Live coordinates of the mouse cursor, if available.
    pub cursor: Option<(i32, i32)>,
    /// Currently focused terminal window ID.
    pub focus: Option<Sid>,
    /// Whether the user has write permissions in the session.
    pub can_write: bool,
}

/// Metadata for an active video stream (screen share or offscreen browser).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsVideoStream {
    /// The user ID of the participant sharing their screen (None = offscreen browser).
    pub owner_uid: Option<Uid>,
    /// Human-readable label for the stream (e.g. "Alice's screen", "Browser").
    pub label: String,
    /// Whether this stream originates from an offscreen browser (sshx-browser).
    pub is_browser: bool,
    /// Canvas x position.
    #[serde(default)]
    pub x: i32,
    /// Canvas y position.
    #[serde(default)]
    pub y: i32,
    /// Widget width in pixels.
    #[serde(default = "default_video_w")]
    pub w: u32,
    /// Widget height in pixels.
    #[serde(default = "default_video_h")]
    pub h: u32,
}

fn default_video_w() -> u32 { 640 }
fn default_video_h() -> u32 { 400 }

/// ICE server configuration (STUN or TURN) for WebRTC.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsIceServer {
    /// One or more STUN/TURN URLs for this server.
    pub urls: Vec<String>,
    /// TURN username (None for STUN servers).
    pub username: Option<String>,
    /// TURN credential (None for STUN servers).
    pub credential: Option<String>,
}

/// ICE candidate for WebRTC negotiation.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsIceCandidate {
    /// The SDP candidate string.
    pub candidate: String,
    /// The SDP mid identifier.
    pub sdp_mid: Option<String>,
    /// The SDP m-line index.
    pub sdp_mline_index: Option<u16>,
}

/// A freehand drawing stroke on the canvas (pencil or highlighter).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WsDrawing {
    /// Tool type: "pencil" or "highlighter".
    pub tool: String,
    /// Flat array of points: [x0, y0, x1, y1, ...] in canvas coordinates.
    #[serde(deserialize_with = "flexible_f32::vec")]
    pub points: Vec<f32>,
    /// CSS color string (e.g. "#ff0000" or "rgba(255,0,255,0.5)").
    pub color: String,
    /// Stroke width in pixels.
    #[serde(deserialize_with = "flexible_f32::scalar")]
    pub width: f32,
    /// Opacity (0.0 to 1.0). Highlighter typically uses ~0.4.
    #[serde(deserialize_with = "flexible_f32::scalar")]
    pub opacity: f32,
}

/// IDE editor state for a single user (open files, cursors, selections, etc.).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsIdeState {
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
    /// Workspace folder name (for "project opened" sync).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_folder: Option<String>,
    /// Workspace folder path on the host machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
}

/// Edit lock state — simple mutex for collaborative editing.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsEditLock {
    /// Who holds the lock (None = free).
    pub holder: Option<Uid>,
    /// Which file is locked.
    pub file: Option<String>,
    /// Expiry timestamp in milliseconds since epoch (auto-release).
    pub expires_at: u64,
}

impl Default for WsEditLock {
    fn default() -> Self {
        WsEditLock {
            holder: None,
            file: None,
            expires_at: 0,
        }
    }
}

/// A slide region (rectangle) for slideshow mode.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsSlide {
    /// Top-left x coordinate.
    pub x: i32,
    /// Top-left y coordinate.
    pub y: i32,
    /// Width of the slide region.
    pub w: u32,
    /// Height of the slide region.
    pub h: u32,
    /// Display order (1-based).
    pub order: u32,
    /// Optional label/title.
    #[serde(default)]
    pub label: String,
}

/// A real-time message sent from the server over WebSocket.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum WsServer {
    /// Initial server message, with the user's ID and session metadata.
    Hello(Uid, String),
    /// The user's authentication was invalid.
    InvalidAuth(),
    /// A snapshot of all current users in the session.
    Users(Vec<(Uid, WsUser)>),
    /// Info about a single user in the session: joined, left, or changed.
    UserDiff(Uid, Option<WsUser>),
    /// Notification when the set of open shells has changed.
    Shells(Vec<(Sid, WsWinsize)>),
    /// Subscription results, in the form of terminal data chunks.
    Chunks(Sid, u64, Vec<Bytes>),
    /// Get a chat message tuple `(uid, name, text)` from the room.
    Hear(Uid, String, String),
    /// Forward a latency measurement between the server and backend shell.
    ShellLatency(u64),
    /// Echo back a timestamp, for the the client's own latency measurement.
    Pong(u64),
    /// Alert the client of an application error.
    Error(String),
    /// Snapshot of all sticky notes when a client first connects.
    Notes(Vec<(Nid, WsNote)>),
    /// A single note was created, updated, or deleted (None = deleted).
    NoteDiff(Nid, Option<WsNote>),
    /// Snapshot of source file metadata (sent on connect and on update).
    /// Fields: (rootName, rootPath, files).
    SourceFiles(String, String, Vec<WsSourceFile>),
    /// A real-time event from a Claude Code session.
    ClaudeEvent(WsClaudeEvent),
    /// Snapshot of all canvas widgets when a client first connects.
    Widgets(Vec<(Wid, WsWidget)>),
    /// A single widget was created, moved, or removed (None = removed).
    WidgetDiff(Wid, Option<WsWidget>),
    /// Snapshot of all shell names when a client first connects.
    ShellNames(Vec<(Sid, String)>),
    /// A single shell name was set or cleared.
    ShellNameDiff(Sid, String),
    /// Snapshot of all active video streams on connect.
    VideoStreams(Vec<(Vid, WsVideoStream)>),
    /// A single video stream was added or removed (None = removed).
    VideoStreamDiff(Vid, Option<WsVideoStream>),
    /// Relay a WebRTC offer from a sharer to a viewer. (vid, target_uid, sdp)
    RtcOffer(Vid, Uid, String),
    /// Relay a WebRTC answer from a viewer to a sharer. (vid, target_uid, sender_uid, sdp)
    RtcAnswer(Vid, Uid, Uid, String),
    /// Relay an ICE candidate between peers. (vid, target_uid, sender_uid, candidate)
    RtcIce(Vid, Uid, Uid, WsIceCandidate),
    /// Current controller of an offscreen browser stream (None = no one).
    BrowserControlStatus(Vid, Option<Uid>),
    /// ICE server configuration for WebRTC (STUN/TURN). Sent once after Hello.
    IceServers(Vec<WsIceServer>),
    /// Broadcast a component-highlight request to all connected overlay clients.
    HighlightComponent(String),
    /// A raw VP8 video frame from a browser stream. (vid, timestamp_us, data, keyframe)
    BrowserFrame(Vid, u64, Bytes, bool),
    /// Snapshot of all text blocks when a client first connects.
    TextBlocks(Vec<(Tid, WsTextBlock)>),
    /// A single text block was created, updated, or deleted (None = deleted).
    TextBlockDiff(Tid, Option<WsTextBlock>),
    /// Snapshot of all drawings when a client first connects.
    Drawings(Vec<(Did, WsDrawing)>),
    /// A single drawing was created or deleted (None = deleted).
    DrawingDiff(Did, Option<WsDrawing>),
    /// Snapshot of all slides when a client first connects.
    Slides(Vec<(Slid, WsSlide)>),
    /// A single slide was created, updated, or deleted (None = deleted).
    SlideDiff(Slid, Option<WsSlide>),
    /// Whether the CLI client has IDE (OpenVSCode Server) support available.
    IdeAvailable(bool),
    /// Snapshot of all IDE states when a client first connects (keyed by Wid).
    IdeStates(Vec<(Wid, WsIdeState)>),
    /// A single IDE widget's state was updated or removed (None = removed).
    IdeStateDiff(Wid, Option<WsIdeState>),
    /// Current edit lock state.
    EditLock(WsEditLock),
    /// Raw ANSI output from the CLI's `/context` command (for the Context tab).
    ContextSnapshot(String),
}

/// A real-time message sent from the client over WebSocket.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum WsClient {
    /// Authenticate the user's encryption key by zeros block and write password
    /// (if provided).
    Authenticate(Bytes, Option<Bytes>),
    /// Set the name of the current user.
    SetName(String),
    /// Send real-time information about the user's cursor.
    SetCursor(Option<(i32, i32)>),
    /// Set the currently focused shell.
    SetFocus(Option<Sid>),
    /// Create a new shell.
    Create(i32, i32),
    /// Close a specific shell.
    Close(Sid),
    /// Move a shell window to a new position and focus it.
    Move(Sid, Option<WsWinsize>),
    /// Add user data to a given shell.
    Data(Sid, Bytes, u64),
    /// Subscribe to a shell, starting at a given chunk index.
    Subscribe(Sid, u64),
    /// Send a a chat message to the room.
    Chat(String),
    /// Send a ping to the server, for latency measurement.
    Ping(u64),
    /// Create a new sticky note at canvas position (x, y).
    CreateNote(i32, i32),
    /// Replace all fields of an existing note.
    UpdateNote(Nid, WsNote),
    /// Delete a sticky note by ID.
    DeleteNote(Nid),
    /// Open a file-tree panel at canvas position (x, y) for the given root path.
    OpenFileTree(i32, i32, String),
    /// Open a file-card widget at canvas position (x, y) for the given file path.
    OpenFileCard(i32, i32, String),
    /// Move a widget to a new canvas position.
    MoveWidget(Wid, i32, i32),
    /// Resize a widget to new pixel dimensions.
    ResizeWidget(Wid, u32, u32),
    /// Remove a widget from the canvas.
    CloseWidget(Wid),
    /// Toggle the collapsed state of a widget.
    SetWidgetCollapsed(Wid, bool),
    /// Request AI descriptions for the listed files (empty = all undescribed files).
    DescribeFiles(Vec<String>),
    /// Update metadata for a source file (image path, description, widget size).
    /// The server persists the change and forwards it to the CLI.
    UpdateFileMetadata(String, WsFileMetadataUpdate),
    /// Set a human-readable name for a shell window.
    SetShellName(Sid, String),
    /// Open a Claude activity feed widget at canvas position (x, y) for the given Claude session ID.
    OpenClaudeFeed(i32, i32, String),
    /// Set a user-defined name for a canvas widget.
    SetWidgetName(Wid, String),
    /// Begin sharing the current user's screen (creates a new video stream).
    StartScreenShare,
    /// Stop sharing the current user's screen.
    StopScreenShare,
    /// Close a specific video stream for everyone.
    CloseStream(Vid),
    /// Start watching a video stream (triggers offer/answer flow).
    WatchStream(Vid),
    /// Stop watching a video stream.
    UnwatchStream(Vid),
    /// Send a WebRTC offer to a specific peer. (vid, target_uid, sdp)
    SendRtcOffer(Vid, Uid, String),
    /// Send a WebRTC answer to a specific peer. (vid, target_uid, sdp)
    SendRtcAnswer(Vid, Uid, String),
    /// Send an ICE candidate to a specific peer. (vid, target_uid, candidate)
    SendRtcIce(Vid, Uid, WsIceCandidate),
    /// Move a video stream widget to a new canvas position.
    MoveVideoStream(Vid, i32, i32),
    /// Resize a video stream widget.
    ResizeVideoStream(Vid, u32, u32),
    /// Forward a mouse/keyboard event to the offscreen browser controller.
    BrowserInput(Vid, String),
    /// Request exclusive control of an offscreen browser stream.
    RequestBrowserControl(Vid),
    /// Release control of an offscreen browser stream.
    ReleaseBrowserControl(Vid),
    /// Create an image widget at canvas position (x, y) with URL, alt text, and optional filename for auto-push.
    CreateImageWidget(i32, i32, String, String, Option<String>),
    /// Push an ImageWidget's image to the CLI with the given filename. (wid, new_name, old_name)
    PushImageWidget(Wid, String, String),
    /// Request all connected overlay clients to flash a component by name.
    HighlightComponent(String),
    /// Open the app overlay widget at canvas position (x, y).
    OpenAppOverlay(i32, i32),
    /// Update the app overlay widget settings. (wid, url, allow_open_file, allow_open_claude)
    UpdateAppOverlay(Wid, String, bool, bool),
    /// Create a new text block at canvas position (x, y).
    CreateTextBlock(i32, i32),
    /// Replace all fields of an existing text block.
    UpdateTextBlock(Tid, WsTextBlock),
    /// Delete a text block by ID.
    DeleteTextBlock(Tid),
    /// Create a new drawing stroke on the canvas.
    CreateDrawing(WsDrawing),
    /// Delete a drawing stroke by ID.
    DeleteDrawing(Did),
    /// Create a new slide region on the canvas.
    CreateSlide(WsSlide),
    /// Update an existing slide.
    UpdateSlide(Slid, WsSlide),
    /// Delete a slide by ID.
    DeleteSlide(Slid),
    /// Reorder slides: list of (Slid, new_order) pairs.
    ReorderSlides(Vec<(Slid, u32)>),
    /// Open an IDE editor widget at canvas position (x, y) with a workspace label.
    OpenIdeEditor(i32, i32, String),
    /// Report local IDE editor state, scoped by widget ID.
    UpdateIdeState(Wid, WsIdeState),
    /// Request the edit lock on a file path.
    RequestEditLock(String),
    /// Explicitly release the edit lock.
    ReleaseEditLock,
    /// Request the CLI to capture and return the current context window summary.
    RequestContextSnapshot,
}
