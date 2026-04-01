<script lang="ts">
  import { createEventDispatcher, onDestroy } from "svelte";
  import { marked } from "marked";
  import CircleButton from "./CircleButton.svelte";
  import CircleButtons from "./CircleButtons.svelte";
  import type { WsClaudeEvent, WsWidget } from "../protocol";

  export let widget: WsWidget;
  export let collapsed: boolean = false;
  export let events: WsClaudeEvent[] = [];
  export let claudeActive: boolean = false;
  export let transcriptPath: string | null = null;
  export let autoOpenCards: boolean = false;
  export let claudePid: string | null = null;
  export let claudePidDead: boolean = false;
  export let sessionId: string | null = null;
  export let sessionName: string | null = null;
  export let name: string | null = null;
  /** Latest context snapshot (markdown) from the CLI. null = none received yet. */
  export let contextSnapshot: string | null = null;
  /** Increments whenever a new snapshot arrives — ensures reactivity even for identical content. */
  export let contextSnapshotVersion: number = 0;

  const dispatch = createEventDispatcher<{
    startMove: MouseEvent;
    delete: void;
    collapse: boolean;
    highlightFile: string;
    toggleAutoOpen: void;
    rename: string;
    ctrlClick: MouseEvent;
    requestContextSnapshot: void;
  }>();

  let viewMode: "graph" | "list" | "tokens" | "context" = "graph";

  // Context tab state
  let contextLoading = false;
  let lastContextSnapshot: string | null = null;
  let lastSeenVersion = -1;

  // Clear loading and cache the snapshot whenever a new version arrives.
  // Using the version counter (not content equality) so repeated identical
  // snapshots still clear the loading spinner.
  $: if (contextSnapshotVersion !== lastSeenVersion && contextSnapshot !== null) {
    lastSeenVersion = contextSnapshotVersion;
    lastContextSnapshot = contextSnapshot;
    contextLoading = false;
  }

  function requestSnapshot() {
    contextLoading = true;
    dispatch("requestContextSnapshot");
  }

  // Compute the real token usage meter from the latest assistant event.
  $: realUsage = computeRealUsage(events);

  function computeRealUsage(events: WsClaudeEvent[]) {
    // Walk backwards to find the last assistant event with token data.
    for (let i = events.length - 1; i >= 0; i--) {
      const ev = events[i];
      if (ev.kind === "assistant_message" || ev.kind === "tool_use") {
        const input = ev.inputTokens ?? 0;
        const output = ev.outputTokens ?? 0;
        const cacheRead = ev.cacheReadTokens ?? 0;
        const cacheCreate = ev.cacheCreationTokens ?? 0;
        const total = input + output + cacheRead + cacheCreate;
        if (total > 0) {
          return { input, output, cacheRead, cacheCreate, total };
        }
      }
    }
    return null;
  }

  const CONTEXT_LIMIT = 200_000;

  $: contextHtml = lastContextSnapshot
    ? String(marked.parse(lastContextSnapshot))
    : null;
  let graphEl: HTMLDivElement;
  let listEl: HTMLDivElement;
  let tokensEl: HTMLDivElement;
  let expandedEvents = new Set<number>();
  let localName = name ?? "";
  $: localName = name ?? "";

  function toggleExpand(i: number) {
    if (expandedEvents.has(i)) expandedEvents.delete(i);
    else expandedEvents.add(i);
    expandedEvents = expandedEvents;
  }

  // ── Tool icon/color config ──
  const TOOL_CONFIG: Record<string, { icon: string; color: string; bg: string }> = {
    Read:      { icon: "\u{1F4D6}", color: "text-sky-300",     bg: "bg-sky-900/40 border-sky-700/60" },
    Write:     { icon: "\u{270F}\u{FE0F}", color: "text-amber-300",   bg: "bg-amber-900/40 border-amber-700/60" },
    Edit:      { icon: "\u{1F527}", color: "text-orange-300",  bg: "bg-orange-900/40 border-orange-700/60" },
    Bash:      { icon: "\u{1F4BB}", color: "text-emerald-300", bg: "bg-emerald-900/40 border-emerald-700/60" },
    Grep:      { icon: "\u{1F50D}", color: "text-violet-300",  bg: "bg-violet-900/40 border-violet-700/60" },
    Glob:      { icon: "\u{1F4C1}", color: "text-yellow-300",  bg: "bg-yellow-900/40 border-yellow-700/60" },
    Agent:     { icon: "\u{1F916}", color: "text-indigo-300",  bg: "bg-indigo-900/40 border-indigo-700/60" },
    WebFetch:  { icon: "\u{1F310}", color: "text-teal-300",    bg: "bg-teal-900/40 border-teal-700/60" },
    WebSearch: { icon: "\u{1F310}", color: "text-teal-300",    bg: "bg-teal-900/40 border-teal-700/60" },
    TodoWrite: { icon: "\u{2611}\u{FE0F}", color: "text-pink-300",    bg: "bg-pink-900/40 border-pink-700/60" },
  };

  const DEFAULT_CONFIG = { icon: "\u{2699}\u{FE0F}", color: "text-zinc-300", bg: "bg-zinc-800/60 border-zinc-600/60" };

  function getToolConfig(tool: string | null): { icon: string; color: string; bg: string } {
    if (tool && TOOL_CONFIG[tool]) return TOOL_CONFIG[tool];
    return DEFAULT_CONFIG;
  }

  // ── Helpers (mirrored from ClaudeActivityFeed) ──
  function extractFilePath(event: WsClaudeEvent): string | null {
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

  function getActionLabel(event: WsClaudeEvent): string {
    if (event.kind === "tool_use") return event.tool ?? "Tool";
    if (event.kind === "tool_result") return "Result";
    if (event.kind === "user_message") return "User";
    if (event.kind === "assistant_message") return "Claude";
    return event.kind;
  }

  function getSimpleDescription(event: WsClaudeEvent): string {
    let parsed: Record<string, unknown> | null = null;
    try { parsed = JSON.parse(event.content); } catch { /* raw fallback */ }

    if (event.kind === "tool_use") {
      const tool = event.tool ?? "";
      if (["Read", "Edit", "Write"].includes(tool)) {
        const fp = (parsed?.file_path ?? parsed?.path ?? parsed?.filename) as string | undefined;
        if (fp) return fp.split("/").pop() ?? fp;
      }
      if (tool === "Bash") {
        const desc = parsed?.description as string | undefined;
        if (desc) return desc.slice(0, 60);
        const cmd = parsed?.command as string | undefined;
        if (cmd) return cmd.slice(0, 60);
      }
      if (tool === "Glob") return (parsed?.pattern as string | undefined) ?? event.content.slice(0, 60);
      if (tool === "Grep") {
        const pat = parsed?.pattern as string | undefined;
        const path = parsed?.path as string | undefined;
        if (pat) return path ? `${pat} in ${path}` : pat;
      }
      return event.content.slice(0, 60);
    }
    if (event.kind === "tool_result") {
      const lines = (event.content.match(/\n/g)?.length ?? 0) + 1;
      return `${lines} line${lines === 1 ? "" : "s"}`;
    }
    return event.content.slice(0, 80);
  }

  // ── Layout types ──
  interface GraphNode {
    x: number;
    y: number;
    w: number;
    h: number;
    event: WsClaudeEvent;
    index: number;
    label: string;
    filePath: string | null;
    description: string;
    config: { icon: string; color: string; bg: string };
    isTurnSeparator?: boolean;
    turnLabel?: string;
  }

  interface GraphEdge {
    fromX: number;
    fromY: number;
    toX: number;
    toY: number;
    isCrossTurn: boolean;
  }

  // ── Layout algorithm ──
  const NODE_GAP_X = 20;
  const NODE_GAP_Y = 16;
  const TURN_GAP_Y = 36;
  const TITLE_BAR_H = 36;
  const CONTROLS_H = 32;
  const MIN_NODE_W = 234;
  const MAX_NODE_W = 624;
  const BASE_NODE_W = 338;

  function computeNodeWidth(event: WsClaudeEvent): number {
    const tokens = (event.inputTokens ?? 0) + (event.outputTokens ?? 0);
    const scaled = BASE_NODE_W + tokens * 0.02;
    return Math.round(Math.max(MIN_NODE_W, Math.min(MAX_NODE_W, scaled)));
  }

  const PREVIEW_LINE_H = 14; // px per line in preview
  const MAX_PREVIEW_LINES = 5;
  const PREVIEW_AREA_MAX = MAX_PREVIEW_LINES * PREVIEW_LINE_H + 8; // +padding

  /** Extract up to MAX_PREVIEW_LINES of inline preview text for a node */
  function getPreviewLines(event: WsClaudeEvent): string[] {
    if (event.kind === "tool_result") {
      // Show file content / command output
      const lines = event.content.split("\n").slice(0, MAX_PREVIEW_LINES);
      return lines;
    }
    if (event.kind === "tool_use") {
      let parsed: Record<string, unknown> | null = null;
      try { parsed = JSON.parse(event.content) as Record<string, unknown>; } catch { /* raw */ }
      if (event.tool === "Edit" && parsed) {
        // Will render as diff instead
        return [];
      }
      if (event.tool === "Bash" && parsed) {
        const cmd = (parsed.command as string | undefined) ?? "";
        return cmd.split("\n").slice(0, MAX_PREVIEW_LINES);
      }
      if (event.tool === "Write" && parsed) {
        const content = (parsed.content as string | undefined) ?? "";
        return content.split("\n").slice(0, MAX_PREVIEW_LINES);
      }
      if (event.tool === "Grep" && parsed) {
        const pat = (parsed.pattern as string | undefined) ?? "";
        return [pat];
      }
    }
    if (event.kind === "assistant_message" || event.kind === "user_message") {
      return event.content.split("\n").slice(0, MAX_PREVIEW_LINES);
    }
    return [];
  }

  /** Get inline diff lines for Edit tool_use */
  function getEditDiffLines(event: WsClaudeEvent): DiffLine[] {
    if (event.kind !== "tool_use" || event.tool !== "Edit") return [];
    let parsed: Record<string, unknown> | null = null;
    try { parsed = JSON.parse(event.content) as Record<string, unknown>; } catch { return []; }
    if (!parsed || typeof parsed.old_string !== "string" || typeof parsed.new_string !== "string") return [];
    return computeInlineDiff(parsed.old_string, parsed.new_string);
  }

  function computeNodeHeight(event: WsClaudeEvent): number {
    const headerH = 36;
    const filePathH = 20;
    const baseH = headerH + (extractFilePath(event) ? filePathH : 0);

    if (event.kind === "tool_use" && event.tool === "Edit") {
      const diff = getEditDiffLines(event);
      const lines = Math.min(diff.length, MAX_PREVIEW_LINES);
      return baseH + 6 + Math.max(lines * PREVIEW_LINE_H + 10, 42);
    }

    const preview = getPreviewLines(event);
    if (preview.length > 0) {
      const lines = Math.min(preview.length, MAX_PREVIEW_LINES);
      return baseH + 6 + Math.max(lines * PREVIEW_LINE_H + 10, 42);
    }

    if (event.kind === "tool_use") return 104;
    if (event.kind === "tool_result") return 73;
    return 83;
  }

  // Group events into turns. A new turn starts on user_message or assistant_message.
  function groupIntoTurns(events: WsClaudeEvent[]): { turnLabel: string; events: WsClaudeEvent[]; startIdx: number }[] {
    const turns: { turnLabel: string; events: WsClaudeEvent[]; startIdx: number }[] = [];
    let current: WsClaudeEvent[] = [];
    let currentLabel = "";
    let currentStart = 0;

    for (let i = 0; i < events.length; i++) {
      const ev = events[i];
      // Skip non-renderable events
      if (ev.kind === "transcript" || ev.kind === "claude_pid" || ev.kind === "session_end") continue;

      const isNewTurn = ev.kind === "user_message" || ev.kind === "assistant_message";
      if (isNewTurn && current.length > 0) {
        turns.push({ turnLabel: currentLabel, events: current, startIdx: currentStart });
        current = [];
      }
      if (isNewTurn) {
        currentLabel = ev.kind === "user_message" ? "User" : "Claude";
        currentStart = i;
      }
      if (!currentLabel) {
        currentLabel = "Start";
        currentStart = i;
      }
      current.push(ev);
    }
    if (current.length > 0) {
      turns.push({ turnLabel: currentLabel, events: current, startIdx: currentStart });
    }
    return turns;
  }

  // Pair tool_use with immediately following tool_result for vertical stacking
  function pairToolEvents(evts: WsClaudeEvent[]): { primary: WsClaudeEvent; result?: WsClaudeEvent; primaryIdx: number }[] {
    const pairs: { primary: WsClaudeEvent; result?: WsClaudeEvent; primaryIdx: number }[] = [];
    let i = 0;
    while (i < evts.length) {
      const ev = evts[i];
      if (ev.kind === "tool_use" && i + 1 < evts.length && evts[i + 1].kind === "tool_result") {
        pairs.push({ primary: ev, result: evts[i + 1], primaryIdx: i });
        i += 2;
      } else {
        pairs.push({ primary: ev, primaryIdx: i });
        i++;
      }
    }
    return pairs;
  }

  $: layout = layoutNodes(events);

  function layoutNodes(events: WsClaudeEvent[]): { nodes: GraphNode[]; edges: GraphEdge[]; totalW: number; totalH: number } {
    const nodes: GraphNode[] = [];
    const edges: GraphEdge[] = [];
    const turns = groupIntoTurns(events);

    let cursorY = 8;
    let maxW = 0;
    let prevTurnLastNode: GraphNode | null = null;

    for (const turn of turns) {
      // Turn separator banner
      const bannerNode: GraphNode = {
        x: 8, y: cursorY, w: 200, h: 18,
        event: turn.events[0], index: turn.startIdx,
        label: turn.turnLabel, filePath: null, description: "",
        config: DEFAULT_CONFIG, isTurnSeparator: true, turnLabel: turn.turnLabel,
      };
      nodes.push(bannerNode);
      cursorY += 18 + 8;

      // Cross-turn edge
      if (prevTurnLastNode) {
        edges.push({
          fromX: prevTurnLastNode.x + prevTurnLastNode.w / 2,
          fromY: prevTurnLastNode.y + prevTurnLastNode.h,
          toX: bannerNode.x + bannerNode.w / 2,
          toY: bannerNode.y,
          isCrossTurn: true,
        });
      }

      const pairs = pairToolEvents(turn.events);
      let cursorX = 8;
      let rowMaxH = 0;
      let lastNodeInTurn: GraphNode | null = null;

      for (const pair of pairs) {
        const pw = computeNodeWidth(pair.primary);
        const ph = computeNodeHeight(pair.primary);

        // Wrap to next row if too wide
        if (cursorX + pw > (widget.w - 16) && cursorX > 8) {
          cursorY += rowMaxH + NODE_GAP_Y;
          cursorX = 8;
          rowMaxH = 0;
        }

        const primaryNode: GraphNode = {
          x: cursorX, y: cursorY, w: pw, h: ph,
          event: pair.primary, index: pair.primaryIdx,
          label: getActionLabel(pair.primary),
          filePath: extractFilePath(pair.primary),
          description: getSimpleDescription(pair.primary),
          config: getToolConfig(pair.primary.tool),
        };
        nodes.push(primaryNode);

        // Edge from previous node in turn
        if (lastNodeInTurn) {
          edges.push({
            fromX: lastNodeInTurn.x + lastNodeInTurn.w,
            fromY: lastNodeInTurn.y + lastNodeInTurn.h / 2,
            toX: primaryNode.x,
            toY: primaryNode.y + primaryNode.h / 2,
            isCrossTurn: false,
          });
        }

        let pairH = ph;

        // Stack tool_result below
        if (pair.result) {
          const rh = computeNodeHeight(pair.result);
          const resultNode: GraphNode = {
            x: cursorX, y: cursorY + ph + 4, w: pw, h: rh,
            event: pair.result, index: pair.primaryIdx + 1,
            label: "Result",
            filePath: null,
            description: getSimpleDescription(pair.result),
            config: { icon: "\u{2713}", color: "text-green-400", bg: "bg-green-900/30 border-green-700/50" },
          };
          nodes.push(resultNode);
          pairH = ph + 4 + rh;

          // Vertical edge: tool_use -> tool_result
          edges.push({
            fromX: primaryNode.x + primaryNode.w / 2,
            fromY: primaryNode.y + primaryNode.h,
            toX: resultNode.x + resultNode.w / 2,
            toY: resultNode.y,
            isCrossTurn: false,
          });
        }

        rowMaxH = Math.max(rowMaxH, pairH);
        lastNodeInTurn = primaryNode;
        cursorX += pw + NODE_GAP_X;
        maxW = Math.max(maxW, cursorX);
      }

      if (lastNodeInTurn) prevTurnLastNode = lastNodeInTurn;
      cursorY += rowMaxH + TURN_GAP_Y;
    }

    return { nodes, edges, totalW: Math.max(maxW + 8, 300), totalH: cursorY + 16 };
  }

  // ── Token layout ──
  // All slices same height, width proportional to token count.
  // Wide slices wrap naturally via flex-wrap.
  const SLICE_H = 64; // fixed height for all slices
  const MIN_SLICE_W = 40; // minimum width so tiny events are still visible
  const PX_PER_TOKEN = 0.06; // pixels of width per token — tune to taste

  interface TokenSlice {
    event: WsClaudeEvent;
    index: number;
    label: string;
    filePath: string | null;
    description: string;
    config: { icon: string; color: string; bg: string };
    tokens: number;
    inputTokens: number;
    outputTokens: number;
    widthPx: number;
    snippet: string | null; // text snippet for user_message / first assistant_message / tool_result
  }

  interface TokenTurnGroup {
    turnLabel: string;
    slices: TokenSlice[];
    totalTokens: number;
  }

  interface TokenLayout {
    turns: TokenTurnGroup[];
    totalInput: number;
    totalOutput: number;
    totalTokens: number;
    maxTokens: number;
    byTool: { tool: string; tokens: number; config: { icon: string; color: string; bg: string } }[];
  }

  $: tokenLayout = layoutTokens(events);

  function layoutTokens(events: WsClaudeEvent[]): TokenLayout {
    const turns = groupIntoTurns(events);
    let totalInput = 0;
    let totalOutput = 0;
    let maxTokens = 0;
    const toolTotals = new Map<string, number>();

    // First pass: compute totals
    for (const turn of turns) {
      for (const ev of turn.events) {
        const inp = ev.inputTokens ?? 0;
        const out = ev.outputTokens ?? 0;
        const tok = inp + out;
        totalInput += inp;
        totalOutput += out;
        if (tok > maxTokens) maxTokens = tok;
        if (ev.kind === "tool_use" || ev.kind === "tool_result") {
          const key = ev.tool ?? ev.kind;
          toolTotals.set(key, (toolTotals.get(key) ?? 0) + tok);
        }
      }
    }

    const result: TokenTurnGroup[] = [];

    for (const turn of turns) {
      const slices: TokenSlice[] = [];
      let turnTotal = 0;
      let seenAssistant = false;

      for (let ei = 0; ei < turn.events.length; ei++) {
        const ev = turn.events[ei];
        const inp = ev.inputTokens ?? 0;
        const out = ev.outputTokens ?? 0;
        const tok = inp + out;
        turnTotal += tok;
        // Width proportional to tokens, with minimum
        const widthPx = Math.max(MIN_SLICE_W, Math.round(tok * PX_PER_TOKEN));

        // Show text snippet for user messages, first assistant message, and tool results
        let snippet: string | null = null;
        if (ev.kind === "user_message") {
          snippet = ev.content.replace(/\s+/g, " ").trim().slice(0, 300);
        } else if (ev.kind === "assistant_message" && !seenAssistant) {
          snippet = ev.content.replace(/\s+/g, " ").trim().slice(0, 300);
          seenAssistant = true;
        } else if (ev.kind === "tool_result") {
          snippet = ev.content.trim().slice(0, 400);
        }

        slices.push({
          event: ev,
          index: turn.startIdx + ei,
          label: getActionLabel(ev),
          filePath: extractFilePath(ev),
          description: getSimpleDescription(ev),
          config: getToolConfig(ev.tool),
          tokens: tok,
          inputTokens: inp,
          outputTokens: out,
          widthPx,
          snippet,
        });
      }

      if (slices.length > 0) {
        result.push({ turnLabel: turn.turnLabel, slices, totalTokens: turnTotal });
      }
    }

    // Build tool breakdown sorted by tokens desc
    const byTool = Array.from(toolTotals.entries())
      .map(([tool, tokens]) => ({ tool, tokens, config: getToolConfig(tool) }))
      .sort((a, b) => b.tokens - a.tokens);

    return {
      turns: result,
      totalInput,
      totalOutput,
      totalTokens: totalInput + totalOutput,
      maxTokens,
      byTool,
    };
  }



  function formatTokenCount(n: number): string {
    if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
    if (n >= 1_000) return (n / 1_000).toFixed(1) + "k";
    return String(n);
  }

  // ── SVG path generation ──
  function edgePath(e: GraphEdge): string {
    if (e.isCrossTurn) {
      // Vertical bezier
      const midY = (e.fromY + e.toY) / 2;
      return `M ${e.fromX} ${e.fromY} C ${e.fromX} ${midY}, ${e.toX} ${midY}, ${e.toX} ${e.toY}`;
    }
    // Horizontal bezier
    const midX = (e.fromX + e.toX) / 2;
    return `M ${e.fromX} ${e.fromY} C ${midX} ${e.fromY}, ${midX} ${e.toY}, ${e.toX} ${e.toY}`;
  }

  let contextEl: HTMLDivElement;

  function scrollToLatest() {
    if (viewMode === "graph" && graphEl) graphEl.scrollTop = graphEl.scrollHeight;
    if (viewMode === "list" && listEl) listEl.scrollTop = listEl.scrollHeight;
    if (viewMode === "tokens" && tokensEl) tokensEl.scrollTop = tokensEl.scrollHeight;
    if (viewMode === "context" && contextEl) contextEl.scrollTop = 0;
  }

  function handleFileClick(path: string) {
    dispatch("highlightFile", path);
  }

  // Shortened file path for display
  function shortPath(p: string): string {
    const parts = p.split("/");
    if (parts.length <= 2) return p;
    return ".../" + parts.slice(-2).join("/");
  }

  function formatTime(ts: string | undefined): string {
    if (!ts) return "";
    try { return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" }); }
    catch { return ""; }
  }

  // ── Tooltip content modes ──
  type TooltipMode =
    | { kind: "diff"; original: string; modified: string; filePath: string }
    | { kind: "json"; value: unknown }
    | { kind: "plain"; text: string };

  function resolveTooltipMode(node: GraphNode): TooltipMode {
    const ev = node.event;

    // Try parsing as JSON
    let parsed: unknown = null;
    try { parsed = JSON.parse(ev.content); } catch { /* not json */ }
    const isObj = parsed !== null && typeof parsed === "object";

    // Edit tool_use → inline diff view
    if (ev.kind === "tool_use" && ev.tool === "Edit" && isObj && !Array.isArray(parsed)) {
      const obj = parsed as Record<string, unknown>;
      if (typeof obj.old_string === "string" && typeof obj.new_string === "string") {
        const fp = (obj.file_path ?? obj.path ?? "") as string;
        return { kind: "diff", original: obj.old_string, modified: obj.new_string, filePath: fp };
      }
    }

    // tool_use JSON params → syntax-highlighted JSON
    if (ev.kind === "tool_use" && isObj) {
      return { kind: "json", value: parsed };
    }

    // tool_result: try JSON, otherwise plain text
    if (ev.kind === "tool_result") {
      if (isObj) return { kind: "json", value: parsed };
      return { kind: "plain", text: ev.content };
    }

    // Messages and other kinds → plain text
    return { kind: "plain", text: ev.content };
  }

  // ── Simple line-level diff ──
  interface DiffLine {
    type: "removed" | "added" | "context";
    text: string;
  }

  function computeInlineDiff(original: string, modified: string): DiffLine[] {
    const oldLines = original.split("\n");
    const newLines = modified.split("\n");
    const result: DiffLine[] = [];

    // Simple LCS-based diff
    const m = oldLines.length;
    const n = newLines.length;

    // Build LCS table
    const dp: number[][] = Array.from({ length: m + 1 }, () => Array(n + 1).fill(0));
    for (let i = 1; i <= m; i++) {
      for (let j = 1; j <= n; j++) {
        dp[i][j] = oldLines[i - 1] === newLines[j - 1]
          ? dp[i - 1][j - 1] + 1
          : Math.max(dp[i - 1][j], dp[i][j - 1]);
      }
    }

    // Backtrack to produce diff
    const diff: DiffLine[] = [];
    let i = m, j = n;
    while (i > 0 || j > 0) {
      if (i > 0 && j > 0 && oldLines[i - 1] === newLines[j - 1]) {
        diff.push({ type: "context", text: oldLines[i - 1] });
        i--; j--;
      } else if (j > 0 && (i === 0 || dp[i][j - 1] >= dp[i - 1][j])) {
        diff.push({ type: "added", text: newLines[j - 1] });
        j--;
      } else {
        diff.push({ type: "removed", text: oldLines[i - 1] });
        i--;
      }
    }
    diff.reverse();
    return diff;
  }

  function escapeHtml(s: string): string {
    return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }

  // ── JSON syntax highlighting ──
  function highlightJsonValue(val: unknown, indent: number = 0): string {
    if (val === null) return `<span class="tt-null">null</span>`;
    if (typeof val === "boolean") return `<span class="tt-bool">${val}</span>`;
    if (typeof val === "number") return `<span class="tt-num">${val}</span>`;
    if (typeof val === "string") {
      return `<span class="tt-str">"${escapeHtml(val)}"</span>`;
    }
    if (Array.isArray(val)) {
      if (val.length === 0) return `<span class="tt-brace">[]</span>`;
      const pad = "  ".repeat(indent + 1);
      const endPad = "  ".repeat(indent);
      const items = val.map(v => `${pad}${highlightJsonValue(v, indent + 1)}`).join(",\n");
      return `<span class="tt-brace">[</span>\n${items}\n${endPad}<span class="tt-brace">]</span>`;
    }
    if (typeof val === "object") {
      const obj = val as Record<string, unknown>;
      const keys = Object.keys(obj);
      if (keys.length === 0) return `<span class="tt-brace">{}</span>`;
      const pad = "  ".repeat(indent + 1);
      const endPad = "  ".repeat(indent);
      const items = keys.map(k => {
        return `${pad}<span class="tt-key">"${escapeHtml(k)}"</span><span class="tt-brace">:</span> ${highlightJsonValue(obj[k], indent + 1)}`;
      }).join(",\n");
      return `<span class="tt-brace">{</span>\n${items}\n${endPad}<span class="tt-brace">}</span>`;
    }
    return String(val);
  }

  // ── Hover tooltip state ──
  let hoveredNode: GraphNode | null = null;
  let pinnedNode: GraphNode | null = null;
  let tooltipX = 0;
  let tooltipY = 0;
  let hoverTimeout: ReturnType<typeof setTimeout> | null = null;
  let currentTooltipMode: TooltipMode | null = null;
  let diffLines: DiffLine[] = [];

  function showTooltipForNode(node: GraphNode) {
    hoveredNode = node;
    positionTooltip(node);
    currentTooltipMode = resolveTooltipMode(node);
    if (currentTooltipMode.kind === "diff") {
      diffLines = computeInlineDiff(currentTooltipMode.original, currentTooltipMode.modified);
    } else {
      diffLines = [];
    }
  }

  function onNodeEnter(node: GraphNode, _e: MouseEvent) {
    if (pinnedNode) return; // don't override a pinned tooltip
    if (hoverTimeout) clearTimeout(hoverTimeout);
    hoverTimeout = setTimeout(() => showTooltipForNode(node), 250);
  }

  function onNodeLeave() {
    if (hoverTimeout) { clearTimeout(hoverTimeout); hoverTimeout = null; }
    if (pinnedNode) return; // keep tooltip open when pinned
    hoveredNode = null;
    currentTooltipMode = null;
    diffLines = [];
  }

  function onNodeClick(node: GraphNode) {
    if (pinnedNode?.index === node.index) {
      // Click same node again → unpin
      pinnedNode = null;
      hoveredNode = null;
      currentTooltipMode = null;
      diffLines = [];
    } else {
      // Pin this node
      pinnedNode = node;
      showTooltipForNode(node);
    }
  }

  function dismissPinned() {
    pinnedNode = null;
    hoveredNode = null;
    currentTooltipMode = null;
    diffLines = [];
  }

  function positionTooltip(node: GraphNode) {
    if (viewMode === "tokens") {
      // For tokens view, position tooltip at fixed location (top-right of container)
      tooltipX = 8;
      tooltipY = 8;
      return;
    }
    const tooltipW = 480;
    const rightEdge = node.x + node.w + 8 + tooltipW;
    if (rightEdge < (widget.w - 16)) {
      tooltipX = node.x + node.w + 8;
    } else {
      tooltipX = Math.max(4, node.x - tooltipW - 8);
    }
    tooltipY = node.y;
  }

  onDestroy(() => {
    if (hoverTimeout) clearTimeout(hoverTimeout);
  });
</script>

<div
  class="panel-window flex flex-col"
  style:width={collapsed ? "280px" : `${widget.w}px`}
  style:height={collapsed ? "auto" : `${widget.h}px`}
>
  <!-- Title bar -->
  <div
    class="flex select-none flex-shrink-0 cursor-grab active:cursor-grabbing"
    on:mousedown={(e) => dispatch("startMove", e)}
    on:dblclick={() => dispatch("collapse", !collapsed)}
  >
    <div class="flex items-center px-3 py-1.5 bg-zinc-800 rounded-t border-b border-zinc-700 w-full gap-2">
      <div class="flex-1 flex items-center">
        <CircleButtons>
          <CircleButton kind="red" on:click={() => dispatch("delete")} />
          <CircleButton kind="yellow" on:click={() => dispatch("collapse", !collapsed)} />
          <CircleButton kind="green" on:click={scrollToLatest} />
        </CircleButtons>
      </div>

      <div class="w-0 flex-grow-[4] text-sm text-zinc-300 text-center font-medium overflow-hidden whitespace-nowrap text-ellipsis" title={name ?? sessionName ?? sessionId ?? undefined}>
        {#if name}
          <span class="text-zinc-200">{name}</span>
        {:else if sessionName}
          <span class="text-zinc-400">{sessionName}</span>
        {:else if sessionId}
          Claude <span class="font-mono text-xs text-indigo-300">{sessionId.slice(0, 8)}</span>
        {:else}
          Claude Graph
        {/if}
      </div>

      <div class="flex-1 flex items-center justify-end gap-1">
        {#if claudeActive}
          <span class="inline-block w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" title="Watching transcript"></span>
        {/if}
        {#if claudePid}
          <span
            class="text-xs font-mono"
            class:text-emerald-400={!claudePidDead}
            class:text-red-400={claudePidDead}
            title={claudePidDead ? `PID ${claudePid} has exited` : `claude PID ${claudePid}`}
          >
            {claudePid}{claudePidDead ? "\u2717" : ""}
          </span>
        {/if}
      </div>
    </div>
  </div>

  <!-- Collapsed summary -->
  {#if collapsed && events.length > 0}
    <div class="overflow-y-auto bg-zinc-900 rounded-b-lg px-2 py-1 flex flex-wrap gap-0.5" style:max-height="80px">
      {#each events.slice(-40) as ev}
        {#if ev.kind === "tool_use"}
          {@const cfg = getToolConfig(ev.tool)}
          <span class="text-[10px] {cfg.color}" title="{ev.tool ?? 'Tool'}: {getSimpleDescription(ev)}">{cfg.icon}</span>
        {:else if ev.kind === "user_message"}
          <span class="text-[10px] text-blue-300" title="User">👤</span>
        {:else if ev.kind === "assistant_message"}
          <span class="text-[10px] text-zinc-300" title="Claude">✦</span>
        {/if}
      {/each}
    </div>
  {/if}

  {#if !collapsed}
    <!-- Controls row -->
    <div class="flex items-center gap-1.5 px-2 py-1 border-b border-zinc-800 bg-zinc-900">
      <!-- Rename input -->
      <input
        class="flex-1 bg-transparent outline-none truncate min-w-0"
        class:text-xs={!localName}
        class:text-zinc-600={!localName}
        class:italic={!localName}
        class:text-sm={!!localName}
        class:text-zinc-300={!!localName}
        placeholder="Name this session..."
        bind:value={localName}
        on:input={() => dispatch("rename", localName)}
        on:mousedown|stopPropagation
        on:pointerdown|stopPropagation
      />

      <!-- View toggle (4-way) -->
      <div class="flex flex-shrink-0 rounded-full overflow-hidden border border-zinc-700">
        <button
          class="text-[10px] px-1.5 py-0.5 transition-colors"
          class:bg-indigo-700={viewMode === "list"}
          class:text-indigo-200={viewMode === "list"}
          class:bg-zinc-800={viewMode !== "list"}
          class:text-zinc-500={viewMode !== "list"}
          on:click={() => viewMode = "list"}
          title="List view"
        >☰</button>
        <button
          class="text-[10px] px-1.5 py-0.5 transition-colors border-l border-zinc-700"
          class:bg-indigo-700={viewMode === "graph"}
          class:text-indigo-200={viewMode === "graph"}
          class:bg-zinc-800={viewMode !== "graph"}
          class:text-zinc-500={viewMode !== "graph"}
          on:click={() => viewMode = "graph"}
          title="Graph view"
        >◉</button>
        <button
          class="text-[10px] px-1.5 py-0.5 transition-colors border-l border-zinc-700"
          class:bg-indigo-700={viewMode === "tokens"}
          class:text-indigo-200={viewMode === "tokens"}
          class:bg-zinc-800={viewMode !== "tokens"}
          class:text-zinc-500={viewMode !== "tokens"}
          on:click={() => viewMode = "tokens"}
          title="Token usage view"
        >▥</button>
        <button
          class="text-[10px] px-1.5 py-0.5 transition-colors border-l border-zinc-700"
          class:bg-indigo-700={viewMode === "context"}
          class:text-indigo-200={viewMode === "context"}
          class:bg-zinc-800={viewMode !== "context"}
          class:text-zinc-500={viewMode !== "context"}
          on:click={() => viewMode = "context"}
          title="Context window"
        >ctx</button>
      </div>

      <button
        class="text-xs px-2 py-0.5 rounded-full transition-colors flex-shrink-0"
        class:bg-indigo-700={autoOpenCards}
        class:text-indigo-200={autoOpenCards}
        class:bg-zinc-700={!autoOpenCards}
        class:text-zinc-400={!autoOpenCards}
        on:click={() => dispatch("toggleAutoOpen")}
        title={autoOpenCards ? "Auto-open files: ON" : "Auto-open files: OFF"}
      >
        auto-open
      </button>

      <span class="text-xs text-zinc-600 flex-shrink-0">{events.length}</span>
    </div>

    {#if viewMode === "list"}
      <!-- Inline list view -->
      <div
        class="overflow-y-auto flex-1 bg-zinc-900 rounded-b text-xs"
        bind:this={listEl}
        on:wheel={(e) => { if (!e.ctrlKey && !e.metaKey && !e.altKey) e.stopPropagation(); }}
        on:click={(e) => { if (e.ctrlKey || e.metaKey) { e.preventDefault(); e.stopPropagation(); dispatch("ctrlClick", e); } }}
      >
        {#if events.length === 0}
          <div class="px-3 py-4 text-zinc-500 text-center">No Claude activity yet.</div>
        {:else}
          {#each events as event, i (i)}
            {@const filePath = extractFilePath(event)}
            {@const actionLabel = getActionLabel(event)}
            {@const isExpanded = expandedEvents.has(i)}
            {@const desc = getSimpleDescription(event)}
            {@const cfg = getToolConfig(event.tool)}
            {@const kindCls = event.kind === "tool_use" ? "text-yellow-300" : event.kind === "tool_result" ? "text-green-400" : event.kind === "user_message" ? "text-blue-300" : "text-zinc-300"}
            <!-- svelte-ignore a11y-no-static-element-interactions -->
            <div
              class="border-b border-zinc-800"
              on:click={() => { toggleExpand(i); if (filePath) dispatch("highlightFile", filePath); }}
              on:keydown={(e) => { if (e.key === "Enter") { toggleExpand(i); if (filePath) dispatch("highlightFile", filePath); } }}
              role="button"
              tabindex="0"
            >
              <div class="flex items-start gap-1 px-2 py-1 hover:bg-zinc-800 cursor-pointer">
                <span class="flex-shrink-0" style="font-size:10px">{cfg.icon}</span>
                <div class="flex-1 min-w-0 flex items-baseline gap-1 overflow-hidden">
                  <span class="font-mono {kindCls} flex-shrink-0">{actionLabel}</span>
                  <span class="text-zinc-500 flex-shrink-0">·</span>
                  <span class="text-zinc-400 truncate min-w-0">{desc}</span>
                  {#if event.inputTokens != null}
                    <span class="text-zinc-600 text-[10px] flex-shrink-0 ml-auto pl-1">{event.inputTokens}↑{event.outputTokens ?? 0}↓</span>
                  {/if}
                </div>
              </div>
              {#if isExpanded}
                <div class="px-2 pb-1">
                  <pre class="text-zinc-300 bg-zinc-800 rounded p-2 text-[10px] leading-relaxed max-h-48 overflow-y-auto whitespace-pre-wrap break-all">{event.content}</pre>
                </div>
              {/if}
            </div>
          {/each}
        {/if}
      </div>
    {:else if viewMode === "tokens"}
      <!-- Tokens view -->
      <!-- svelte-ignore a11y-click-events-have-key-events -->
      <!-- svelte-ignore a11y-no-static-element-interactions -->
      <div
        class="overflow-y-auto flex-1 bg-zinc-900 rounded-b text-xs relative"
        bind:this={tokensEl}
        on:wheel={(e) => { if (!e.ctrlKey && !e.metaKey && !e.altKey) e.stopPropagation(); }}
        on:click={(e) => { if (e.ctrlKey || e.metaKey) { e.preventDefault(); e.stopPropagation(); dispatch("ctrlClick", e); } else dismissPinned(); }}
      >
        {#if events.length === 0}
          <div class="px-3 py-4 text-zinc-500 text-center">No Claude activity yet.</div>
        {:else}
          <!-- Summary header -->
          <div class="px-3 py-2 border-b border-zinc-800 bg-zinc-850">
            <div class="flex items-center gap-2 text-[11px] font-mono">
              <span class="text-sky-300">{formatTokenCount(tokenLayout.totalInput)}↑</span>
              <span class="text-amber-300">{formatTokenCount(tokenLayout.totalOutput)}↓</span>
              <span class="text-zinc-500">=</span>
              <span class="text-zinc-200 font-medium">{formatTokenCount(tokenLayout.totalTokens)}</span>
            </div>
            <!-- Tool breakdown bar -->
            {#if tokenLayout.byTool.length > 0 && tokenLayout.totalTokens > 0}
              <div class="flex mt-1.5 rounded overflow-hidden h-2.5 bg-zinc-800">
                {#each tokenLayout.byTool as seg}
                  {@const pct = (seg.tokens / tokenLayout.totalTokens) * 100}
                  {#if pct > 0.5}
                    <div
                      class="h-full {seg.config.bg} border-r border-zinc-900/50"
                      style="width:{pct}%"
                      title="{seg.tool}: {formatTokenCount(seg.tokens)} ({pct.toFixed(1)}%)"
                    ></div>
                  {/if}
                {/each}
              </div>
              <div class="flex flex-wrap gap-x-3 gap-y-0.5 mt-1">
                {#each tokenLayout.byTool.slice(0, 6) as seg}
                  <span class="text-[9px] {seg.config.color}">
                    {seg.config.icon} {seg.tool} <span class="text-zinc-500">{formatTokenCount(seg.tokens)}</span>
                  </span>
                {/each}
              </div>
            {/if}
          </div>

          <!-- All slices in a single horizontal flow, width = tokens -->
          <div class="flex flex-wrap items-start px-2 py-2 gap-1">
            {#each tokenLayout.turns as turn}
              <!-- Inline turn pill -->
              <div
                class="flex items-center px-1.5 rounded bg-zinc-800/50"
                style="height:{SLICE_H}px"
              >
                <span class="text-[8px] font-semibold uppercase tracking-wider leading-none"
                  class:text-blue-400={turn.turnLabel === "User"}
                  class:text-zinc-500={turn.turnLabel !== "User"}
                >{turn.turnLabel}<br/><span class="text-zinc-600 font-normal font-mono">{formatTokenCount(turn.totalTokens)}</span></span>
              </div>

              {#each turn.slices as slice}
                {@const barNode = { x: 0, y: 0, w: 0, h: 0, event: slice.event, index: slice.index, label: slice.label, filePath: slice.filePath, description: slice.description, config: slice.config }}
                {@const w = slice.widthPx}
                {@const hasSnippet = slice.snippet !== null && w >= 80}
                <!-- svelte-ignore a11y-click-events-have-key-events -->
                <!-- svelte-ignore a11y-no-static-element-interactions -->
                <div
                  class="relative rounded cursor-pointer hover:brightness-125 transition-all duration-100 overflow-hidden border {slice.config.bg}"
                  class:ring-1={pinnedNode?.index === slice.index}
                  class:ring-indigo-400={pinnedNode?.index === slice.index}
                  style="width:{w}px; height:{SLICE_H}px"
                  title="{slice.label}: {formatTokenCount(slice.tokens)}{slice.filePath ? '\n' + slice.filePath : ''}{slice.description ? '\n' + slice.description : ''}"
                  on:click={() => onNodeClick(barNode)}
                  on:mouseenter={(e) => onNodeEnter(barNode, e)}
                  on:mouseleave={onNodeLeave}
                >
                  {#if hasSnippet}
                    <!-- Wide snippet slice: show text preview -->
                    <div class="flex flex-col h-full p-1 relative z-10 gap-0.5">
                      <div class="flex items-center gap-1 flex-shrink-0">
                        <span class="flex-shrink-0" style="font-size:9px">{slice.config.icon}</span>
                        <span class="font-semibold {slice.config.color}" style="font-size:7px">{slice.label}</span>
                        <span class="font-mono text-zinc-500 ml-auto" style="font-size:7px">{formatTokenCount(slice.tokens)}</span>
                      </div>
                      <div
                        class="flex-1 min-h-0 overflow-y-auto text-zinc-400 whitespace-pre-wrap break-words"
                        style="font-size:7px; line-height:1.35"
                        on:wheel|stopPropagation
                      >
                        {slice.snippet}
                      </div>
                    </div>
                  {:else if w >= 56}
                    <!-- Medium slice: icon + label + token count -->
                    <div class="flex flex-col items-center justify-center h-full px-1 py-0.5 relative z-10 gap-0.5">
                      <span class="flex-shrink-0 leading-none" style="font-size:12px">{slice.config.icon}</span>
                      <span class="font-mono truncate w-full text-center font-medium {slice.config.color}" style="font-size:7px; line-height:1.2">
                        {slice.label}
                      </span>
                      <span class="font-mono text-zinc-400" style="font-size:7px; line-height:1">
                        {formatTokenCount(slice.tokens)}
                      </span>
                    </div>
                  {:else}
                    <!-- Narrow slice: icon only -->
                    <div class="flex items-center justify-center h-full relative z-10">
                      <span class="leading-none" style="font-size:11px">{slice.config.icon}</span>
                    </div>
                  {/if}
                </div>
              {/each}
            {/each}
          </div>

          <!-- Tokens tooltip (sticky overlay) -->
          {#if hoveredNode && currentTooltipMode}
            <!-- svelte-ignore a11y-no-static-element-interactions -->
            <div
              class="sticky bottom-2 z-50 rounded-lg border border-zinc-600 bg-zinc-800 shadow-xl flex flex-col mx-2 mb-2"
              style="max-height:300px"
              on:mouseenter={() => { if (hoverTimeout) clearTimeout(hoverTimeout); }}
              on:mouseleave={onNodeLeave}
            >
              <!-- Tooltip header -->
              <div class="flex items-center gap-2 px-3 py-2 border-b border-zinc-700 flex-shrink-0">
                <span style="font-size:14px">{hoveredNode.config.icon}</span>
                <span class="font-mono font-medium text-xs {hoveredNode.config.color}">{hoveredNode.label}</span>
                {#if currentTooltipMode.kind === "diff"}
                  <span class="text-zinc-500 text-[10px] font-mono truncate">{currentTooltipMode.filePath}</span>
                {/if}
                {#if hoveredNode.event.inputTokens != null}
                  <span class="ml-auto text-zinc-500 text-[10px]">
                    {hoveredNode.event.inputTokens}↑ {hoveredNode.event.outputTokens ?? 0}↓
                  </span>
                {/if}
                {#if pinnedNode}
                  <button class="text-zinc-500 hover:text-zinc-300 text-xs ml-1" on:click={dismissPinned}>✕</button>
                {/if}
              </div>

              {#if hoveredNode.filePath}
                <div class="px-3 py-1 border-b border-zinc-700 flex-shrink-0">
                  <button
                    class="text-indigo-400 hover:text-indigo-300 underline text-xs font-mono truncate block w-full text-left"
                    on:click|stopPropagation={() => hoveredNode?.filePath && handleFileClick(hoveredNode.filePath)}
                  >
                    {hoveredNode.filePath}
                  </button>
                </div>
              {/if}

              <!-- Content area -->
              {#if currentTooltipMode.kind === "diff"}
                <div class="overflow-auto flex-1 rounded-b-lg diff-view" on:wheel|stopPropagation>
                  <table class="w-full font-mono text-[11px] leading-relaxed border-collapse">
                    {#each diffLines as line, idx}
                      <tr class:diff-removed={line.type === "removed"} class:diff-added={line.type === "added"} class:diff-context={line.type === "context"}>
                        <td class="diff-gutter select-none text-right pr-2 pl-2 align-top" style="width:20px">
                          {#if line.type === "removed"}
                            <span class="text-red-500">-</span>
                          {:else if line.type === "added"}
                            <span class="text-green-500">+</span>
                          {:else}
                            <span class="text-zinc-600">{idx + 1}</span>
                          {/if}
                        </td>
                        <td class="diff-content pr-2 whitespace-pre-wrap break-all">{line.text || " "}</td>
                      </tr>
                    {/each}
                  </table>
                </div>
              {:else if currentTooltipMode.kind === "json"}
                <div class="overflow-auto flex-1 tooltip-body" on:wheel|stopPropagation>
                  <pre class="font-mono text-[11px] leading-relaxed px-3 py-2 whitespace-pre-wrap break-all">{@html highlightJsonValue(currentTooltipMode.value)}</pre>
                </div>
              {:else}
                <div class="overflow-auto flex-1" on:wheel|stopPropagation>
                  <pre class="text-zinc-300 text-[11px] leading-relaxed px-3 py-2 whitespace-pre-wrap break-all">{currentTooltipMode.text}</pre>
                </div>
              {/if}
            </div>
          {/if}
        {/if}
      </div>
    {:else if viewMode === "context"}
      <!-- Context view -->
      <div
        class="overflow-y-auto flex-1 bg-zinc-900 rounded-b flex flex-col"
        bind:this={contextEl}
        on:wheel={(e) => { if (!e.ctrlKey && !e.metaKey && !e.altKey) e.stopPropagation(); }}
      >
        <!-- Header: real usage meter + refresh button -->
        <div class="flex items-center gap-2 px-3 py-2 border-b border-zinc-800 flex-shrink-0">
          {#if realUsage}
            {@const pct = Math.min(100, (realUsage.total / CONTEXT_LIMIT) * 100)}
            {@const cachePct = realUsage.total > 0 ? ((realUsage.cacheRead + realUsage.cacheCreate) / realUsage.total) * 100 : 0}
            <div class="flex-1 min-w-0">
              <div class="flex items-center justify-between mb-0.5">
                <span class="text-[10px] text-zinc-400">
                  {(realUsage.total / 1000).toFixed(1)}k / {CONTEXT_LIMIT / 1000}k tokens
                </span>
                <span class="text-[10px] text-zinc-500">{pct.toFixed(1)}%</span>
              </div>
              <!-- Outer bar: total tokens -->
              <div class="h-2 rounded-full bg-zinc-700 overflow-hidden relative">
                <div
                  class="h-full rounded-full bg-indigo-600 transition-all duration-300"
                  style="width:{pct}%"
                ></div>
                <!-- Inner bar: cache ratio overlay -->
                {#if cachePct > 0}
                  <div
                    class="absolute top-0 left-0 h-full rounded-full bg-indigo-400/50"
                    style="width:{pct * cachePct / 100}%"
                    title="Cache-read: {realUsage.cacheRead.toLocaleString()}, Cache-write: {realUsage.cacheCreate.toLocaleString()}"
                  ></div>
                {/if}
              </div>
              <div class="flex items-center gap-2 mt-0.5 text-[9px] text-zinc-600">
                <span>in:{realUsage.input.toLocaleString()}</span>
                <span>out:{realUsage.output.toLocaleString()}</span>
                {#if realUsage.cacheRead > 0}
                  <span class="text-indigo-400">cache-read:{realUsage.cacheRead.toLocaleString()}</span>
                {/if}
                {#if realUsage.cacheCreate > 0}
                  <span class="text-indigo-300">cache-write:{realUsage.cacheCreate.toLocaleString()}</span>
                {/if}
              </div>
            </div>
          {:else}
            <span class="text-[10px] text-zinc-500 flex-1">No token data yet.</span>
          {/if}
          <button
            type="button"
            class="flex-shrink-0 text-[10px] px-2 py-1 rounded border border-zinc-700 text-zinc-400 hover:text-zinc-200 hover:border-zinc-500 transition-colors"
            class:opacity-50={contextLoading}
            disabled={contextLoading}
            on:click={requestSnapshot}
            title="Fetch /context output from CLI"
          >
            {contextLoading ? "…" : "↻ Refresh"}
          </button>
        </div>

        <!-- ANSI output body -->
        {#if contextLoading && !lastContextSnapshot}
          <div class="flex items-center justify-center flex-1 text-zinc-500 text-xs gap-2 py-8">
            <span class="animate-pulse">Fetching context window…</span>
          </div>
        {:else if lastContextSnapshot}
          <div
            class="flex-1 overflow-auto px-3 py-2 text-[11px] leading-relaxed context-md"
            on:wheel|stopPropagation
          >
            {@html contextHtml}
          </div>
        {:else}
          <div class="flex flex-col items-center justify-center flex-1 text-zinc-500 text-xs gap-2 py-8">
            <span>Click ↻ Refresh to capture the current context window.</span>
            <span class="text-zinc-600">Requires an active Claude Code session.</span>
          </div>
        {/if}
      </div>
    {:else}
      <!-- Graph view -->
      <div
        class="overflow-auto flex-1 bg-zinc-900 rounded-b relative"
        bind:this={graphEl}
        on:wheel={(e) => { if (!e.ctrlKey && !e.metaKey && !e.altKey) e.stopPropagation(); }}
        on:click={(e) => { if (e.ctrlKey || e.metaKey) { e.preventDefault(); e.stopPropagation(); dispatch("ctrlClick", e); } }}
      >
        {#if events.length === 0}
          <div class="px-3 py-4 text-zinc-500 text-center text-xs">No Claude activity yet.</div>
        {:else}
          <!-- svelte-ignore a11y-click-events-have-key-events -->
          <!-- svelte-ignore a11y-no-static-element-interactions -->
          <div class="relative" style="width:{layout.totalW}px; height:{layout.totalH}px; min-width:100%"
            on:click|self={dismissPinned}
          >
            <!-- SVG edge layer -->
            <svg
              class="absolute top-0 left-0 pointer-events-none"
              width={layout.totalW}
              height={layout.totalH}
              style="overflow:visible"
            >
              {#each layout.edges as edge}
                <path
                  d={edgePath(edge)}
                  stroke={edge.isCrossTurn ? "#52525b" : "#3f3f46"}
                  stroke-width="1"
                  fill="none"
                />
              {/each}
            </svg>

            <!-- Node layer -->
            {#each layout.nodes as node}
              {#if node.isTurnSeparator}
                <!-- Turn separator banner -->
                <div
                  class="absolute flex items-center gap-1 px-2 rounded"
                  style="left:{node.x}px; top:{node.y}px; height:{node.h}px"
                >
                  <div class="h-px flex-1 bg-zinc-700 min-w-[20px]"></div>
                  <span class="text-[10px] font-medium uppercase tracking-wider"
                    class:text-blue-400={node.turnLabel === "User"}
                    class:text-zinc-400={node.turnLabel !== "User"}
                  >
                    {node.turnLabel}
                  </span>
                  <div class="h-px flex-1 bg-zinc-700 min-w-[20px]"></div>
                </div>
              {:else}
                {@const editDiff = getEditDiffLines(node.event)}
                {@const previewLines = editDiff.length === 0 ? getPreviewLines(node.event) : []}
                <!-- Event node -->
                <!-- svelte-ignore a11y-click-events-have-key-events -->
                <!-- svelte-ignore a11y-no-static-element-interactions -->
                <div
                  class="absolute rounded border {node.config.bg} cursor-pointer hover:brightness-125 transition-all duration-100 flex flex-col overflow-hidden"
                  class:ring-1={pinnedNode?.index === node.index}
                  class:ring-indigo-400={pinnedNode?.index === node.index}
                  class:brightness-125={pinnedNode?.index === node.index}
                  style="left:{node.x}px; top:{node.y}px; width:{node.w}px; height:{node.h}px"
                  on:click={() => onNodeClick(node)}
                  on:mouseenter={(e) => onNodeEnter(node, e)}
                  on:mouseleave={onNodeLeave}
                >
                  <!-- Node header -->
                  <div class="flex items-center gap-1.5 px-2.5 py-1.5 overflow-hidden">
                    <span class="flex-shrink-0" style="font-size:14px">{node.config.icon}</span>
                    <span class="font-mono truncate font-medium {node.config.color}" style="font-size:9px; line-height:1.3">
                      {node.label}
                    </span>
                    {#if node.event.inputTokens != null}
                      <span class="ml-auto text-zinc-500 flex-shrink-0" style="font-size:7px">
                        {node.event.inputTokens}↑{node.event.outputTokens ?? 0}↓
                      </span>
                    {/if}
                  </div>

                  <!-- Node content -->
                  <div class="px-2.5 overflow-hidden flex flex-col flex-1 min-h-0" style="font-size:8px; line-height:1.4">
                    {#if node.filePath}
                      <button
                        class="text-indigo-400 hover:text-indigo-300 underline truncate block w-full text-left font-mono flex-shrink-0"
                        style="font-size:8px"
                        on:click|stopPropagation={() => node.filePath && handleFileClick(node.filePath)}
                      >
                        {shortPath(node.filePath)}
                      </button>
                    {/if}
                    {#if editDiff.length > 0}
                      <!-- Inline diff preview for Edit -->
                      <div class="mt-0.5 overflow-auto rounded bg-zinc-950/60 border border-zinc-700/50 flex-1 node-diff-view" style="max-height:{PREVIEW_AREA_MAX}px" on:wheel|stopPropagation on:mousedown|stopPropagation>
                        <table class="w-full font-mono border-collapse" style="font-size:8px; line-height:{PREVIEW_LINE_H}px">
                          {#each editDiff.slice(0, MAX_PREVIEW_LINES) as line}
                            <tr class:diff-removed={line.type === "removed"} class:diff-added={line.type === "added"} class:diff-context={line.type === "context"}>
                              <td class="diff-gutter select-none text-right pr-1 pl-1 align-top" style="width:14px">
                                {#if line.type === "removed"}<span class="text-red-500">-</span>{:else if line.type === "added"}<span class="text-green-500">+</span>{:else}<span class="text-zinc-700">&nbsp;</span>{/if}
                              </td>
                              <td class="diff-content pr-1 whitespace-pre break-all">{line.text || " "}</td>
                            </tr>
                          {/each}
                          {#if editDiff.length > MAX_PREVIEW_LINES}
                            <tr><td colspan="2" class="text-zinc-600 text-center" style="font-size:7px">+{editDiff.length - MAX_PREVIEW_LINES} more lines</td></tr>
                          {/if}
                        </table>
                      </div>
                    {:else if previewLines.length > 0}
                      <!-- Code / content preview -->
                      <div class="mt-0.5 overflow-auto rounded bg-zinc-950/60 border border-zinc-700/50 flex-1" style="max-height:{PREVIEW_AREA_MAX}px" on:wheel|stopPropagation on:mousedown|stopPropagation>
                        <pre class="font-mono text-zinc-400 px-1.5 py-1 whitespace-pre-wrap break-all" style="font-size:8px; line-height:{PREVIEW_LINE_H}px">{previewLines.join("\n")}</pre>
                      </div>
                    {:else}
                      <div class="text-zinc-400 truncate">
                        {node.description}
                      </div>
                    {/if}
                  </div>

                  <!-- Timestamp -->
                  {#if node.event.timestamp}
                    <div class="absolute bottom-0.5 right-1.5 text-zinc-600" style="font-size:6px">
                      {formatTime(node.event.timestamp)}
                    </div>
                  {/if}
                </div>
              {/if}
            {/each}

            <!-- Hover tooltip -->
            {#if hoveredNode && currentTooltipMode}
              <!-- svelte-ignore a11y-no-static-element-interactions -->
              <div
                class="absolute z-50 rounded-lg border border-zinc-600 bg-zinc-800 shadow-xl flex flex-col"
                style="left:{tooltipX}px; top:{tooltipY}px; width:{currentTooltipMode.kind === 'diff' ? 520 : 400}px; max-height:{currentTooltipMode.kind === 'diff' ? 400 : 360}px"
                on:mouseenter={() => { if (hoverTimeout) clearTimeout(hoverTimeout); }}
                on:mouseleave={onNodeLeave}
              >
                <!-- Tooltip header -->
                <div class="flex items-center gap-2 px-3 py-2 border-b border-zinc-700 flex-shrink-0">
                  <span style="font-size:14px">{hoveredNode.config.icon}</span>
                  <span class="font-mono font-medium text-xs {hoveredNode.config.color}">{hoveredNode.label}</span>
                  {#if currentTooltipMode.kind === "diff"}
                    <span class="text-zinc-500 text-[10px] font-mono truncate">{currentTooltipMode.filePath}</span>
                  {/if}
                  {#if hoveredNode.event.inputTokens != null}
                    <span class="ml-auto text-zinc-500 text-[10px]">
                      {hoveredNode.event.inputTokens}↑ {hoveredNode.event.outputTokens ?? 0}↓
                    </span>
                  {/if}
                  {#if hoveredNode.event.timestamp}
                    <span class="text-zinc-600 text-[10px]">{formatTime(hoveredNode.event.timestamp)}</span>
                  {/if}
                </div>

                {#if hoveredNode.filePath}
                  <div class="px-3 py-1 border-b border-zinc-700 flex-shrink-0">
                    <button
                      class="text-indigo-400 hover:text-indigo-300 underline text-xs font-mono truncate block w-full text-left"
                      on:click|stopPropagation={() => hoveredNode?.filePath && handleFileClick(hoveredNode.filePath)}
                    >
                      {hoveredNode.filePath}
                    </button>
                  </div>
                {/if}

                <!-- Content area -->
                {#if currentTooltipMode.kind === "diff"}
                  <!-- Inline diff view for Edit tool -->
                  <div class="overflow-auto flex-1 rounded-b-lg diff-view" on:wheel|stopPropagation>
                    <table class="w-full font-mono text-[11px] leading-relaxed border-collapse">
                      {#each diffLines as line, idx}
                        <tr class:diff-removed={line.type === "removed"} class:diff-added={line.type === "added"} class:diff-context={line.type === "context"}>
                          <td class="diff-gutter select-none text-right pr-2 pl-2 align-top" style="width:20px">
                            {#if line.type === "removed"}
                              <span class="text-red-500">-</span>
                            {:else if line.type === "added"}
                              <span class="text-green-500">+</span>
                            {:else}
                              <span class="text-zinc-600">{idx + 1}</span>
                            {/if}
                          </td>
                          <td class="diff-content pr-2 whitespace-pre-wrap break-all">{line.text || " "}</td>
                        </tr>
                      {/each}
                    </table>
                  </div>
                {:else if currentTooltipMode.kind === "json"}
                  <!-- Syntax-highlighted JSON -->
                  <div class="overflow-auto flex-1 tooltip-body" on:wheel|stopPropagation>
                    <pre class="font-mono text-[11px] leading-relaxed px-3 py-2 whitespace-pre-wrap break-all">{@html highlightJsonValue(currentTooltipMode.value)}</pre>
                  </div>
                {:else}
                  <!-- Plain text -->
                  <div class="overflow-auto flex-1" on:wheel|stopPropagation>
                    <pre class="text-zinc-300 text-[11px] leading-relaxed px-3 py-2 whitespace-pre-wrap break-all">{currentTooltipMode.text}</pre>
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  {/if}
</div>

<style lang="postcss">
  .panel-window {
    @apply inline-flex rounded-lg border border-zinc-700 bg-zinc-800 opacity-90;
    transition: opacity 200ms;
  }
  .panel-window:hover {
    @apply opacity-100;
  }

  /* JSON syntax highlighting */
  .tooltip-body :global(.tt-key) {
    color: #93c5fd; /* blue-300 */
  }
  .tooltip-body :global(.tt-str) {
    color: #6ee7b7; /* emerald-300 */
  }
  .tooltip-body :global(.tt-num) {
    color: #fcd34d; /* amber-300 */
  }
  .tooltip-body :global(.tt-bool) {
    color: #c4b5fd; /* violet-300 */
  }
  .tooltip-body :global(.tt-null) {
    color: #f87171; /* red-400 */
    font-style: italic;
  }
  .tooltip-body :global(.tt-brace) {
    color: #a1a1aa; /* zinc-400 */
  }

  /* Diff view styling */
  .diff-view :global(tr.diff-removed) {
    background: rgba(239, 68, 68, 0.15); /* red tint */
    color: #fca5a5; /* red-300 */
  }
  .diff-view :global(tr.diff-added) {
    background: rgba(34, 197, 94, 0.15); /* green tint */
    color: #86efac; /* green-300 */
  }
  .diff-view :global(tr.diff-context) {
    color: #a1a1aa; /* zinc-400 */
  }
  .diff-view :global(td.diff-gutter) {
    border-right: 1px solid #3f3f46; /* zinc-700 */
    user-select: none;
    opacity: 0.7;
  }

  /* Inline node diff styling (same colors, compact) */
  .node-diff-view :global(tr.diff-removed) {
    background: rgba(239, 68, 68, 0.12);
    color: #fca5a5;
  }
  .node-diff-view :global(tr.diff-added) {
    background: rgba(34, 197, 94, 0.12);
    color: #86efac;
  }
  .node-diff-view :global(tr.diff-context) {
    color: #71717a;
  }
  .node-diff-view :global(td.diff-gutter) {
    border-right: 1px solid #27272a;
    user-select: none;
    opacity: 0.6;
  }

  /* Markdown output area for context window snapshot */
  :global(.context-md) {
    color: #d4d4d8;
  }
  :global(.context-md h2) {
    font-size: 0.8rem;
    font-weight: 600;
    color: #a1a1aa;
    margin: 0.75rem 0 0.25rem;
    padding-bottom: 0.2rem;
    border-bottom: 1px solid #3f3f46;
  }
  :global(.context-md h3) {
    font-size: 0.75rem;
    font-weight: 500;
    color: #a1a1aa;
    margin: 0.5rem 0 0.2rem;
  }
  :global(.context-md p) {
    margin: 0.25rem 0;
  }
  :global(.context-md strong) {
    color: #e4e4e7;
    font-weight: 600;
  }
  :global(.context-md table) {
    width: 100%;
    border-collapse: collapse;
    margin: 0.4rem 0;
    font-size: 10px;
  }
  :global(.context-md th) {
    text-align: left;
    color: #71717a;
    padding: 2px 6px;
    border-bottom: 1px solid #3f3f46;
    font-weight: 500;
  }
  :global(.context-md td) {
    padding: 2px 6px;
    border-bottom: 1px solid #27272a;
    color: #d4d4d8;
  }
  :global(.context-md tr:last-child td) {
    border-bottom: none;
  }
  :global(.context-md code) {
    font-family: "Fira Code VF", monospace;
    font-size: 10px;
    background: #27272a;
    padding: 0 3px;
    border-radius: 2px;
  }
</style>
