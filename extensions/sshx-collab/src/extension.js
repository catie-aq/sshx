// sshx-collab: VS Code extension for syncing editor state with sshx.
//
// Communicates with the sshx CLI sync endpoint via:
//   POST http://127.0.0.1:${SSHX_SYNC_PORT}/state  — send local state
//   GET  http://127.0.0.1:${SSHX_SYNC_PORT}/events  — receive remote states (SSE)
//   POST http://127.0.0.1:${SSHX_SYNC_PORT}/lock    — request edit lock
//   POST http://127.0.0.1:${SSHX_SYNC_PORT}/unlock  — release edit lock

const vscode = require("vscode");
const http = require("http");

/** @type {number} */
let syncPort = 0;

/** @type {number} IDE instance / widget ID, used for per-widget state scoping. */
let ideId = 0;

/** @type {NodeJS.Timeout | null} */
let debounceTimer = null;

/** @type {import("http").IncomingMessage | null} */
let sseResponse = null;

/** @type {Map<string, vscode.TextEditorDecorationType>} */
const cursorDecorations = new Map();

/** User colors for remote cursors. */
const CURSOR_COLORS = [
  "rgba(59,130,246,0.5)", // blue
  "rgba(239,68,68,0.5)", // red
  "rgba(34,197,94,0.5)", // green
  "rgba(168,85,247,0.5)", // purple
  "rgba(234,179,8,0.5)", // yellow
  "rgba(236,72,153,0.5)", // pink
];

/**
 * Collect the current IDE state.
 * @returns {object}
 */
function collectState() {
  const tabGroups = vscode.window.tabGroups;
  const openFiles = [];
  for (const group of tabGroups.all) {
    for (const tab of group.tabs) {
      if (tab.input && tab.input.uri) {
        const rel = vscode.workspace.asRelativePath(tab.input.uri, false);
        openFiles.push(rel);
      }
    }
  }

  const activeEditor = vscode.window.activeTextEditor;
  let activeFile = null;
  const cursors = [];
  const selections = [];
  const visibleRanges = [];

  if (activeEditor) {
    activeFile = vscode.workspace.asRelativePath(
      activeEditor.document.uri,
      false
    );

    for (const sel of activeEditor.selections) {
      cursors.push([activeFile, sel.active.line, sel.active.character]);
      if (!sel.isEmpty) {
        selections.push([
          activeFile,
          sel.start.line,
          sel.start.character,
          sel.end.line,
          sel.end.character,
        ]);
      }
    }

    for (const range of activeEditor.visibleRanges) {
      visibleRanges.push([activeFile, range.start.line, range.end.line]);
    }
  }

  // Include workspace folder info for "project/folder opened" sync.
  const workspaceFolders = vscode.workspace.workspaceFolders || [];
  const workspaceFolder =
    workspaceFolders.length > 0 ? workspaceFolders[0].name : null;
  const workspacePath =
    workspaceFolders.length > 0 ? workspaceFolders[0].uri.fsPath : null;

  return {
    openFiles,
    activeFile,
    cursors,
    selections,
    visibleRanges,
    sidebarVisible: true,
    sidebarView: null,
    panelVisible: false,
    workspaceFolder,
    workspacePath,
    ideId,
  };
}

/**
 * Send the current state to the sync endpoint (debounced).
 */
function sendState() {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    const state = collectState();
    const data = JSON.stringify(state);
    const req = http.request(
      {
        hostname: "127.0.0.1",
        port: syncPort,
        path: "/state",
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "Content-Length": Buffer.byteLength(data),
        },
      },
      () => {} // ignore response
    );
    req.on("error", () => {}); // ignore connection errors
    req.write(data);
    req.end();
  }, 100);
}

/**
 * Connect to the SSE events stream from the sync endpoint.
 */
function connectSSE() {
  const req = http.get(
    {
      hostname: "127.0.0.1",
      port: syncPort,
      path: `/events?ide_id=${ideId}`,
      headers: { Accept: "text/event-stream" },
    },
    (res) => {
      sseResponse = res;
      let buffer = "";
      res.on("data", (chunk) => {
        buffer += chunk.toString();
        // Parse SSE frames.
        const lines = buffer.split("\n");
        buffer = lines.pop() || "";
        let eventType = "";
        let eventData = "";
        for (const line of lines) {
          if (line.startsWith("event:")) {
            eventType = line.slice(6).trim();
          } else if (line.startsWith("data:")) {
            eventData = line.slice(5).trim();
          } else if (line === "") {
            if (eventData) {
              handleRemoteEvent(eventType, eventData);
            }
            eventType = "";
            eventData = "";
          }
        }
      });
      res.on("end", () => {
        sseResponse = null;
        // Reconnect after a delay.
        setTimeout(connectSSE, 3000);
      });
      res.on("error", () => {
        sseResponse = null;
        setTimeout(connectSSE, 3000);
      });
    }
  );
  req.on("error", () => {
    setTimeout(connectSSE, 3000);
  });
}

/**
 * Handle a remote IDE event from the SSE stream.
 * @param {string} kind
 * @param {string} dataStr
 */
function handleRemoteEvent(kind, dataStr) {
  try {
    const wrapper = JSON.parse(dataStr);
    if (kind === "state" && wrapper.data) {
      const state = wrapper.data;
      // Only show cursor decorations — do NOT auto-navigate to the remote
      // file or auto-scroll to the remote position. Forcing navigation
      // whenever the collaborator switches files makes independent editing
      // impossible (both editors would continuously chase each other).
      applyRemoteCursors(state);
    } else if (kind === "lock" && wrapper.data) {
      applyRemoteLock(wrapper.data);
    }
  } catch {
    // ignore parse errors
  }
}

/**
 * Apply remote cursor decorations from a remote IdeState.
 * @param {object} state
 */
function applyRemoteCursors(state) {
  // Clear old decorations.
  for (const [, dec] of cursorDecorations) {
    dec.dispose();
  }
  cursorDecorations.clear();

  if (!state.cursors || !Array.isArray(state.cursors)) return;

  const activeEditor = vscode.window.activeTextEditor;
  if (!activeEditor) return;

  const currentFile = vscode.workspace.asRelativePath(
    activeEditor.document.uri,
    false
  );

  // Show remote cursors that are in the same file.
  let colorIdx = 0;
  for (const cursor of state.cursors) {
    const [path, line, col] = cursor;
    if (path !== currentFile) continue;

    const color = CURSOR_COLORS[colorIdx % CURSOR_COLORS.length];
    colorIdx++;

    const dec = vscode.window.createTextEditorDecorationType({
      backgroundColor: color,
      isWholeLine: false,
      rangeBehavior: vscode.DecorationRangeBehavior.ClosedClosed,
      after: {
        contentText: " ◆",
        color: color.replace("0.5", "1"),
        fontStyle: "normal",
        fontWeight: "bold",
      },
    });

    const pos = new vscode.Position(line, col);
    const range = new vscode.Range(pos, pos.translate(0, 1));
    activeEditor.setDecorations(dec, [range]);
    cursorDecorations.set(`remote-${colorIdx}`, dec);
  }
}

/**
 * Show edit lock status in the status bar.
 * @param {object} lock — { holder, file, expiresAt }
 */
function applyRemoteLock(lock) {
  if (lock.holder !== null && lock.holder !== undefined) {
    // Skip expired locks (server auto-expires after 5 s; don't show stale UI).
    const now = Date.now();
    if (lock.expiresAt && lock.expiresAt < now) return;
    vscode.window.setStatusBarMessage(
      `🔒 Edit lock: ${lock.file || "unknown"} (user ${lock.holder})`,
      5000
    );
  }
}

/**
 * @param {vscode.ExtensionContext} context
 */
function activate(context) {
  syncPort = parseInt(process.env.SSHX_SYNC_PORT || "0", 10);
  if (!syncPort) {
    // Not running under sshx — do nothing.
    return;
  }
  ideId = parseInt(process.env.SSHX_IDE_ID || "0", 10);

  console.log(`[sshx-collab] Activating with sync port ${syncPort}, IDE ID ${ideId}`);

  // Observe editor state changes and send updates.
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(() => sendState()),
    vscode.window.onDidChangeTextEditorSelection(() => sendState()),
    vscode.window.onDidChangeVisibleTextEditors(() => sendState()),
    vscode.window.onDidChangeTextEditorVisibleRanges(() => sendState()),
    vscode.workspace.onDidOpenTextDocument(() => sendState()),
    vscode.workspace.onDidCloseTextDocument(() => sendState())
  );

  // Request edit lock on text changes.
  context.subscriptions.push(
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (e.contentChanges.length > 0) {
        const path = vscode.workspace.asRelativePath(e.document.uri, false);
        const data = JSON.stringify({ file: path, ideId });
        const req = http.request(
          {
            hostname: "127.0.0.1",
            port: syncPort,
            path: "/lock",
            method: "POST",
            headers: {
              "Content-Type": "application/json",
              "Content-Length": Buffer.byteLength(data),
            },
          },
          () => {}
        );
        req.on("error", () => {});
        req.write(data);
        req.end();
      }
    })
  );

  // Send initial state.
  sendState();

  // Connect to SSE for remote events.
  connectSSE();
}

function deactivate() {
  if (debounceTimer) clearTimeout(debounceTimer);
  if (sseResponse) {
    sseResponse.destroy();
    sseResponse = null;
  }
  for (const [, dec] of cursorDecorations) {
    dec.dispose();
  }
  cursorDecorations.clear();
}

module.exports = { activate, deactivate };
