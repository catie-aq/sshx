# Claude Code Interactions

Real-time visibility into a running Claude Code session from the sshx browser UI.

---

## Overview

When you run `sshx` in a directory where Claude Code is active, the sshx server
automatically discovers Claude's JSONL transcript and streams events to every
connected browser in real time. Viewers see every tool call, response, and user
message as it happens — without any extra configuration.

---

## Architecture

```
Claude Code process
  └─ writes JSONL to ~/.claude/projects/<encoded-cwd>/*.jsonl
        │
        ▼
sshx CLI  (crates/sshx/src/workspace.rs)
  run_claude_tracker()
  └─ tail_transcript()  ─── reads last 200 lines on startup, then tails
        │  WsClaudeEvent (JSON bytes)
        ▼ gRPC ServerUpdate::ClaudeEvent
sshx-server  (crates/sshx-server/src/grpc.rs)
  └─ stores in Session::claude_events  (ring buffer, 200 events)
        │  WsServer::ClaudeEvent
        ▼ WebSocket (CBOR-X)
Browser  (src/lib/Session.svelte)
  └─ claudeInstances map  (rolling buffer, 200 events per session)
        │
        ▼ ClaudeActivityFeed.svelte
```

---

## Session Discovery

`workspace.rs` maps the current working directory to a Claude project path:

1. Replace every `/` in the absolute CWD with `-` (including the leading slash).
2. Example: `/home/user/repos/sshx` → `-home-user-repos-sshx`
3. Look for `~/.claude/projects/<encoded>/` — the directory containing JSONL files.

The encoded path **always starts with `-`** (do not strip it).

`run_claude_tracker` loops forever:
- If no transcript exists, retries every 5 seconds.
- When `tail_transcript` returns (file rotated or removed), it re-discovers.

This means sshx can be started before Claude Code — the feed activates as soon as
Claude begins a session.

---

## WsClaudeEvent Fields

```typescript
interface WsClaudeEvent {
  sessionId:    string;        // Claude session UUID
  kind:         string;        // see Kinds below
  content:      string;        // truncated at 500 chars
  tool?:        string;        // tool name for tool_use events
  timestamp?:   string;        // ISO-8601 from the JSONL record
  inputTokens?: number;        // assistant_message only
  outputTokens?: number;       // assistant_message only
}
```

### Kinds

| kind | Source | Description |
|---|---|---|
| `tool_use` | Claude Code | Claude is calling a tool (Read, Bash, Grep, etc.) |
| `tool_result` | Claude Code | Tool returned its output |
| `user_message` | Claude Code | Human turn (the prompt) |
| `assistant_message` | Claude Code | Claude's text response |

Content is truncated server-side to 500 characters to keep WebSocket payloads
small; the full content lives in the JSONL transcript on disk.

---

## UI Components

### Toolbar Button — "Claude sessions"

Located in `src/lib/ui/Toolbar.svelte`. Shows a count badge when there are active
instances. Clicking opens the `ClaudeInstanceList` dropdown.

### ClaudeInstanceList Dropdown

Lists every discovered Claude session (keyed by `sessionId`). Each row shows:
- Session name (derived from the first user message, truncated to 60 chars)
- Session UUID fallback if no name is available yet
- "open" button — creates a `ClaudeActivityFeed` widget on the canvas

### ClaudeActivityFeed Panel

`src/lib/ui/ClaudeActivityFeed.svelte` — a floating, moveable panel on the canvas.

Props:

| Prop | Type | Description |
|---|---|---|
| `widget` | `WsWidget` | Position, size |
| `collapsed` | `boolean` | Double-click header to toggle |
| `events` | `WsClaudeEvent[]` | Event list (up to 200) |
| `claudeActive` | `boolean` | Transcript tail is live (green pulse) |
| `transcriptPath` | `string\|null` | Shown in full mode for debugging |
| `autoOpenCards` | `boolean` | Auto-open file cards on tool_use |
| `claudePid` | `string\|null` | PID of the `claude` process |
| `claudePidDead` | `boolean` | Whether that PID has exited |
| `sessionId` | `string\|null` | UUID (shown if no name yet) |
| `sessionName` | `string\|null` | First user message (shown in title) |

---

## Display Modes

### Full Mode (default)

Each event row shows:
- Colored icon (`⚙ tool_use`, `✓ tool_result`, `👤 user_message`, `✦ assistant_message`)
- Action label (tool name, "User", "Claude", etc.)
- Short description (file basename, bash command, grep pattern, token counts…)
- Click to expand — shows full raw `content` in a scrollable `<pre>`
- Token label on `assistant_message`: `{inputTokens}↑{outputTokens}↓`

### Compact Mode

Toggle with the "compact" button in the toolbar row inside the panel.
Shows only icon + action label — no description, no tokens. Useful when
many events are present and you want to see the shape of activity at a glance.

---

## Collapsed Mode

Double-click the title bar (or click the yellow circle button) to collapse.

The panel shrinks to the title bar + a compact event list (max 160 px tall,
scrollable) showing the 20 most recent events:

```
⚙  Read                           14:32
✓  Result                         14:32
✦  Claude                         14:33
```

Each row: icon · action label · HH:MM timestamp (right-aligned).

Double-click again to expand back to full view.

---

## Auto-Open Files

When "auto-open files" is enabled (toggle button inside the panel), every
`tool_use` event whose input JSON contains a file path (`path`, `file_path`,
`filename`, or `file` key) triggers a `FileCard` widget to open on the canvas.

Implementation in `Session.svelte`:
1. Parse `event.content` as JSON.
2. Extract the first recognized file path key.
3. Check if a FileCard for that path already exists.
4. If not, dispatch `autoOpenCards` → create a new `WsWidget` of type `file`.

Supported tools that produce file paths: `Read`, `Edit`, `Write`, `Glob`, `Grep`.

---

## File Highlight

Clicking an event row in **full mode** dispatches `highlightFile(filePath)` if
the event references a file. `Session.svelte` receives this and temporarily
toggles a `highlighted` flag on the matching `FileCard` widget.

The `FileCard` component applies a `ring-2 ring-indigo-400` style while
highlighted. The highlight auto-clears after ~1 second.

Note: highlight is at the **file level only**. Line-level highlights within the
xterm or file viewer are not yet implemented.

---

## Ring Buffers

| Layer | Size | Location |
|---|---|---|
| Server ring buffer | 200 events | `Session::claude_events` in `session.rs` |
| Transcript replay | last 200 lines | `tail_transcript` in `workspace.rs` |
| Client rolling buffer | 200 events | `claudeInstances` map in `Session.svelte` |
| Initial WS replay | all buffered events | `socket.rs` handshake sends `Session::claude_events` |

When a browser connects (or reconnects), the server replays all buffered
`WsClaudeEvent`s so the viewer sees the full recent history immediately.

---

## Token Tracking

Token counts appear only on `assistant_message` events. They come from the
`usage` field in Claude's JSONL transcript. The label format is:

```
{inputTokens}↑{outputTokens}↓
```

Displayed in `text-zinc-600` at the far right of the event row (full mode only).

---

## Known Limitations

- **Line-level highlights** — clicking a Read/Edit event highlights the whole
  FileCard, not a specific line range within the file.
- **PID tracking is best-effort** — PID is parsed from the JSONL `cwd` metadata
  field when available; if Claude rotates its transcript without updating the PID
  record, `claudePidDead` may be incorrect.
- **Single-workspace** — `run_claude_tracker` watches the CWD of the `sshx`
  process. If Claude Code is running in a different directory, its events won't
  appear.
- **Content truncation** — event content is capped at 500 chars server-side;
  to see full content, check the JSONL transcript (path shown in the panel header).
- **No search/filter** — the event list has no search. Use the JSONL file for
  offline analysis.
