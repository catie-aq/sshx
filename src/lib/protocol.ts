type Sid = number; // u32
type Uid = number; // u32
type Nid = number; // u32
type Wid = number; // u32
type Vid = number; // u32
type Tid = number; // u32

/** Source file metadata, see WsSourceFile in the Rust server. */
export type WsSourceFile = {
  path: string;
  kind: string;
  localImports: string[];
  libraries: string[];
  exports: string[];
  importedBy: string[];
  description: string;
  lineCount: number;
  lastModified: string;
  /** First 6144 chars of the file, for the FileCard code preview. */
  content: string;
  /** URL/path of an illustration image attached to this file card. */
  imagePath: string;
  /** Last known widget width in pixels (0 = use default). */
  widgetW: number;
  /** Last known widget height in pixels (0 = use default). */
  widgetH: number;
};

/** Partial metadata update for a source file, see WsFileMetadataUpdate in Rust. */
export type WsFileMetadataUpdate = {
  imagePath?: string;
  description?: string;
  widgetW?: number;
  widgetH?: number;
};

/** A real-time event from a Claude Code session. */
export type WsClaudeEvent = {
  kind: "tool_use" | "tool_result" | "user_message" | "assistant_message" | "transcript" | "claude_pid" | "session_end";
  tool: string | null;
  content: string;
  timestamp: string;
  sessionId: string;
  /** Input token count (present on tool_use and assistant_message events). */
  inputTokens?: number;
  /** Output token count (present on tool_use and assistant_message events). */
  outputTokens?: number;
  /** UNIX timestamp (seconds) of the transcript file — only on synthetic "transcript" events. */
  fileMtime?: number;
};

/** A node in the component graph, keyed by file path. */
export type WsGraphNode = {
  path: string;
  kind: string;
  /** Filename portion of the path, for display labels. */
  label: string;
  description: string;
  lineCount: number;
};

/** A directed edge in the component graph. */
export type WsGraphEdge = {
  /** Source node path (the file that imports). */
  from: string;
  /** Target node path (the file being imported). */
  to: string;
};

/**
 * Component dependency graph derived from workspace analysis.
 *
 * `nodes` is a path→node map for O(1) adjacency look-up.
 * `codeEdges` encodes static import relationships.
 * `displayEdges` is reserved for UI render-tree relationships (populated later).
 */
export type WsComponentGraph = {
  nodes: Record<string, WsGraphNode>;
  codeEdges: WsGraphEdge[];
  displayEdges: WsGraphEdge[];
};

/** The kind/content of a canvas widget. */
export type WsWidgetKind =
  | { type: "fileTree"; root: string }
  | { type: "fileCard"; path: string }
  | { type: "graphView" }
  | { type: "claudeFeed"; instanceId: string }
  | { type: "image"; url: string; alt: string }
  | { type: "appOverlay"; url: string; allowOpenFile: boolean; allowOpenClaude: boolean };

/** A generic canvas widget. */
export type WsWidget = {
  x: number;
  y: number;
  w: number;
  h: number;
  kind: WsWidgetKind;
  /** Whether this widget is collapsed to a compact mini view. */
  collapsed: boolean;
  /** User-set display name for this widget (optional). */
  name?: string;
};

/** Metadata for an active video stream (screen share or offscreen browser). */
export type WsVideoStream = {
  ownerUid: Uid | null;
  label: string;
  isBrowser: boolean;
  x: number;
  y: number;
  w: number;
  h: number;
};

/** ICE server configuration (STUN or TURN) for WebRTC. */
export type WsIceServer = {
  urls: string[];
  username?: string;
  credential?: string;
};

/** ICE candidate for WebRTC negotiation. */
export type WsIceCandidate = {
  candidate: string;
  sdpMid: string | null;
  sdpMlineIndex: number | null;
};

/** Sticky note on the canvas, see the Rust version. */
export type WsNote = {
  x: number;
  y: number;
  text: string;
  color: string;
  pinned: boolean;
};

/** A text block on the canvas (FigJam-style rich text). */
export type WsTextBlock = {
  x: number;
  y: number;
  content: string;
  fontSize: string;
  color: string;
  align: string;
};

/** Position and size of a window, see the Rust version. */
export type WsWinsize = {
  x: number;
  y: number;
  rows: number;
  cols: number;
};

/** Information about a user, see the Rust version */
export type WsUser = {
  name: string;
  cursor: [number, number] | null;
  focus: number | null;
  canWrite: boolean;
};

/** Server message type, see the Rust version. */
export type WsServer = {
  hello?: [Uid, string];
  invalidAuth?: [];
  users?: [Uid, WsUser][];
  userDiff?: [Uid, WsUser | null];
  shells?: [Sid, WsWinsize][];
  chunks?: [Sid, number, Uint8Array[]];
  hear?: [Uid, string, string];
  shellLatency?: number | bigint;
  pong?: number | bigint;
  error?: string;
  notes?: [Nid, WsNote][];
  noteDiff?: [Nid, WsNote | null];
  /** [workspaceRootName, workspaceRootPath, files[]] */
  sourceFiles?: [string, string, WsSourceFile[]];
  claudeEvent?: WsClaudeEvent;
  widgets?: [Wid, WsWidget][];
  widgetDiff?: [Wid, WsWidget | null];
  /** Component graph derived from workspace analysis. */
  componentGraph?: WsComponentGraph;
  /** Snapshot of all shell names on connect. */
  shellNames?: [Sid, string][];
  /** A single shell name was set. */
  shellNameDiff?: [Sid, string];
  /** Snapshot of all active video streams on connect. */
  videoStreams?: [Vid, WsVideoStream][];
  /** A single video stream was added or removed (null = removed). */
  videoStreamDiff?: [Vid, WsVideoStream | null];
  /** Relay a WebRTC offer. [vid, targetUid, sdp] */
  rtcOffer?: [Vid, Uid, string];
  /** Relay a WebRTC answer. [vid, targetUid, senderUid, sdp] */
  rtcAnswer?: [Vid, Uid, Uid, string];
  /** Relay an ICE candidate. [vid, targetUid, senderUid, candidate] */
  rtcIce?: [Vid, Uid, Uid, WsIceCandidate];
  /** Current controller of an offscreen browser stream (null = no one). */
  browserControlStatus?: [Vid, Uid | null];
  /** ICE server configuration for WebRTC (STUN/TURN). Sent once after Hello. */
  iceServers?: WsIceServer[];
  /** Broadcast a component-highlight to all connected overlay clients. */
  highlightComponent?: string;
  /** A raw VP8 video frame from a browser stream. [vid, timestamp_us, data, is_keyframe] */
  browserFrame?: [Vid, bigint, Uint8Array, boolean];
  /** Snapshot of all text blocks on connect. */
  textBlocks?: [Tid, WsTextBlock][];
  /** A single text block was created, updated, or deleted (null = deleted). */
  textBlockDiff?: [Tid, WsTextBlock | null];
};

/** Client message type, see the Rust version. */
export type WsClient = {
  authenticate?: [Uint8Array, Uint8Array | null];
  setName?: string;
  setCursor?: [number, number] | null;
  setFocus?: number | null;
  create?: [number, number];
  close?: Sid;
  move?: [Sid, WsWinsize | null];
  data?: [Sid, Uint8Array, bigint];
  subscribe?: [Sid, number];
  chat?: string;
  ping?: bigint;
  createNote?: [number, number];
  updateNote?: [Nid, WsNote];
  deleteNote?: Nid;
  openFileTree?: [number, number, string];
  openFileCard?: [number, number, string];
  moveWidget?: [Wid, number, number];
  resizeWidget?: [Wid, number, number];
  closeWidget?: Wid;
  /** Toggle the collapsed state of a widget. */
  setWidgetCollapsed?: [Wid, boolean];
  /** Request AI descriptions for the listed file paths (empty = all undescribed). */
  describeFiles?: string[];
  /** Update metadata for a source file (image, description, widget size). */
  updateFileMetadata?: [string, WsFileMetadataUpdate];
  /** Set a human-readable name for a shell window. */
  setShellName?: [Sid, string];
  /** Open a graph-view widget at canvas position (x, y). */
  openGraphView?: [number, number];
  /** Open a Claude activity feed widget at canvas position (x, y) for the given Claude session ID. */
  openClaudeFeed?: [number, number, string];
  /** Set a user-defined name for a canvas widget. */
  setWidgetName?: [Wid, string];
  /** Begin sharing the current user's screen. */
  startScreenShare?: true;
  /** Stop sharing the current user's screen. */
  stopScreenShare?: true;
  /** Close a specific video stream for everyone. */
  closeStream?: Vid;
  /** Start watching a video stream (triggers offer/answer flow). */
  watchStream?: Vid;
  /** Stop watching a video stream. */
  unwatchStream?: Vid;
  /** Send a WebRTC offer to a specific peer. [vid, targetUid, sdp] */
  sendRtcOffer?: [Vid, Uid, string];
  /** Send a WebRTC answer to a specific peer. [vid, targetUid, sdp] */
  sendRtcAnswer?: [Vid, Uid, string];
  /** Send an ICE candidate to a specific peer. [vid, targetUid, candidate] */
  sendRtcIce?: [Vid, Uid, WsIceCandidate];
  /** Move a video stream widget to a new canvas position. */
  moveVideoStream?: [Vid, number, number];
  /** Resize a video stream widget. */
  resizeVideoStream?: [Vid, number, number];
  /** Forward a mouse/keyboard event to the offscreen browser. [vid, jsonEvent] */
  browserInput?: [Vid, string];
  /** Request exclusive control of an offscreen browser stream. */
  requestBrowserControl?: Vid;
  /** Release control of an offscreen browser stream. */
  releaseBrowserControl?: Vid;
  /** Create an image widget at canvas position (x, y) with URL and alt text. */
  createImageWidget?: [number, number, string, string];
  /** Request all connected overlay clients to flash a component by name. */
  highlightComponent?: string;
  /** Open the app overlay widget at canvas position (x, y). */
  openAppOverlay?: [number, number];
  /** Update the app overlay widget settings. [wid, url, allowOpenFile, allowOpenClaude] */
  updateAppOverlay?: [Wid, string, boolean, boolean];
  /** Create a new text block at canvas position (x, y). */
  createTextBlock?: [number, number];
  /** Replace all fields of an existing text block. */
  updateTextBlock?: [Tid, WsTextBlock];
  /** Delete a text block by ID. */
  deleteTextBlock?: Tid;
};

/** An item in the command-palette search list. */
export type SearchItem = {
  type: "terminal" | "fileCard" | "fileTree";
  id: number;
  label: string;
  sublabel: string;
  /** Optional description shown below the label and included in search. */
  description?: string;
  x: number;
  y: number;
};
