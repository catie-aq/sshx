//! Serializable types sent and received by the web server.

use std::collections::HashMap;

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sshx_core::{Nid, Sid, Uid, Wid};

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

/// A node in the component graph, representing one source file.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsGraphNode {
    /// Relative path from the workspace root (matches the map key).
    pub path: String,
    /// File kind: "component"|"hook"|"utility"|"config"|"type"|"other".
    pub kind: String,
    /// Filename portion of the path, for display labels.
    pub label: String,
    /// AI-generated or empty description.
    pub description: String,
    /// Total source lines.
    pub line_count: u32,
}

/// A directed edge in the component graph.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsGraphEdge {
    /// Source node path (the file that imports).
    pub from: String,
    /// Target node path (the file being imported).
    pub to: String,
}

/// Component graph with nodes and two edge sets.
///
/// Nodes are keyed by relative file path for O(1) adjacency look-up.
/// `codeEdges` encodes static import relationships; `displayEdges` is
/// reserved for UI-level render-tree relationships (populated later).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsComponentGraph {
    /// Nodes keyed by relative file path.
    pub nodes: HashMap<String, WsGraphNode>,
    /// Directed code-dependency edges (importer → importee).
    pub code_edges: Vec<WsGraphEdge>,
    /// Directed display/render-tree edges (placeholder).
    pub display_edges: Vec<WsGraphEdge>,
}

/// Real-time message representing a sticky note on the canvas.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsNote {
    /// The x-coordinate of the note on the canvas.
    pub x: i32,
    /// The y-coordinate of the note on the canvas.
    pub y: i32,
    /// The text content of the note.
    pub text: String,
    /// Color key: "yellow" | "pink" | "blue" | "green" | "purple"
    pub color: String,
    /// Whether the note is pinned (position locked).
    pub pinned: bool,
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
    /// Force-directed component graph visualization.
    GraphView {},
    /// A Claude Code activity feed panel.
    ClaudeFeed {
        /// The Claude Code session ID this feed tracks.
        instance_id: String,
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
    /// The String is the workspace root basename (e.g. "my-project").
    SourceFiles(String, Vec<WsSourceFile>),
    /// A real-time event from a Claude Code session.
    ClaudeEvent(WsClaudeEvent),
    /// Component graph derived from the workspace analysis (sent on connect and on update).
    ComponentGraph(WsComponentGraph),
    /// Snapshot of all canvas widgets when a client first connects.
    Widgets(Vec<(Wid, WsWidget)>),
    /// A single widget was created, moved, or removed (None = removed).
    WidgetDiff(Wid, Option<WsWidget>),
    /// Snapshot of all shell names when a client first connects.
    ShellNames(Vec<(Sid, String)>),
    /// A single shell name was set or cleared.
    ShellNameDiff(Sid, String),
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
    /// Open a graph-view widget at canvas position (x, y).
    OpenGraphView(i32, i32),
    /// Open a Claude activity feed widget at canvas position (x, y) for the given Claude session ID.
    OpenClaudeFeed(i32, i32, String),
}
