type Sid = number; // u32
type Uid = number; // u32
type Nid = number; // u32
type Wid = number; // u32

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
  kind: "tool_use" | "tool_result" | "user_message" | "assistant_message" | "transcript" | "claude_pid";
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
  | { type: "claudeFeed"; instanceId: string };

/** A generic canvas widget. */
export type WsWidget = {
  x: number;
  y: number;
  w: number;
  h: number;
  kind: WsWidgetKind;
  /** Whether this widget is collapsed to a compact mini view. */
  collapsed: boolean;
};

/** Sticky note on the canvas, see the Rust version. */
export type WsNote = {
  x: number;
  y: number;
  text: string;
  color: string;
  pinned: boolean;
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
  /** [workspaceRootName, files[]] */
  sourceFiles?: [string, WsSourceFile[]];
  claudeEvent?: WsClaudeEvent;
  widgets?: [Wid, WsWidget][];
  widgetDiff?: [Wid, WsWidget | null];
  /** Component graph derived from workspace analysis. */
  componentGraph?: WsComponentGraph;
  /** Snapshot of all shell names on connect. */
  shellNames?: [Sid, string][];
  /** A single shell name was set. */
  shellNameDiff?: [Sid, string];
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
