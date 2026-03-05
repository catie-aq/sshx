/**
 * Runtime graph — tracks live links between canvas objects.
 *
 * Nodes represent terminals, open FileCard widgets, and active Claude sessions.
 * Edges represent which files each Claude session has touched via tool_use calls.
 */

import type { WsClaudeEvent, WsWidget, WsWinsize } from "./protocol";

export type RuntimeEdge = {
  from: string; // "claude:<sessionId>" | "terminal:<sid>" | "file:<path>"
  to: string;
  kind: "claude_file" | "terminal_file";
};

export type RuntimeNode = {
  id: string;
  label: string;
  category: "file" | "claude" | "terminal";
};

/** Extract a file path from a Claude tool_use event's content JSON. */
export function extractFilePathFromEvent(event: WsClaudeEvent): string | null {
  if (event.kind !== "tool_use") return null;
  try {
    const input = JSON.parse(event.content);
    for (const key of ["path", "file_path", "filename", "file"]) {
      if (typeof input[key] === "string") return input[key];
    }
  } catch {
    const m = event.content.match(/"(?:path|file_path|filename)"\s*:\s*"([^"]+)"/);
    if (m) return m[1];
  }
  return null;
}

/**
 * Build runtime nodes and edges from live session state.
 *
 * @param claudeInstances  Map of session-id → instance with events array.
 * @param widgets          Current canvas widget map (wid → WsWidget).
 * @param shells           Current shell list ([sid, WsWinsize][]).
 */
export function buildRuntimeEdges(
  claudeInstances: Map<string, { events: WsClaudeEvent[] }>,
  widgets: Map<number, WsWidget>,
  shells: [number, WsWinsize][],
): { nodes: RuntimeNode[]; edges: RuntimeEdge[] } {
  const nodes: RuntimeNode[] = [];
  const edges: RuntimeEdge[] = [];
  const fileNodeIds = new Set<string>();

  // Terminal nodes
  for (const [sid] of shells) {
    nodes.push({
      id: `terminal:${sid}`,
      label: `Terminal ${sid}`,
      category: "terminal",
    });
  }

  // File nodes from open FileCard widgets
  for (const [, widget] of widgets) {
    if (widget.kind.type === "fileCard") {
      const path = widget.kind.path;
      const nodeId = `file:${path}`;
      if (!fileNodeIds.has(nodeId)) {
        fileNodeIds.add(nodeId);
        const label = path.split("/").pop() ?? path;
        nodes.push({ id: nodeId, label, category: "file" });
      }
    }
  }

  // Claude instance nodes + file edges
  for (const [sessionId, inst] of claudeInstances) {
    if (inst.events.length === 0) continue;
    const claudeId = `claude:${sessionId}`;
    nodes.push({
      id: claudeId,
      label: `Claude ${sessionId.slice(0, 6)}`,
      category: "claude",
    });

    // Collect all unique file paths this session has touched
    const touchedFiles = new Set<string>();
    for (const ev of inst.events) {
      const path = extractFilePathFromEvent(ev);
      if (path) touchedFiles.add(path);
    }

    for (const path of touchedFiles) {
      const nodeId = `file:${path}`;
      if (!fileNodeIds.has(nodeId)) {
        fileNodeIds.add(nodeId);
        const label = path.split("/").pop() ?? path;
        nodes.push({ id: nodeId, label, category: "file" });
      }
      edges.push({ from: claudeId, to: nodeId, kind: "claude_file" });
    }
  }

  return { nodes, edges };
}
