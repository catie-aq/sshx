# CLAUDE.md — sshx Project Guide

This file helps Claude Code navigate and extend the sshx codebase effectively.

## What is sshx?

**sshx** is a secure, web-based collaborative terminal sharing tool. Users run a CLI client (`sshx`) that connects to a server. A browser-based Svelte frontend lets others join the session, see live terminal output, and type input — all end-to-end encrypted.

Key capabilities:
- End-to-end encryption (Argon2id + AES-128-CTR); server never sees plaintext
- Real-time multi-user collaboration on an infinite canvas
- Live cursors, chat, and window drag/resize
- Predictive echo (Mosh-like latency hiding)
- Optional Redis-backed distributed mesh for horizontal scaling

---

## Repository Layout

```
sshx/
├── CLAUDE.md                  # ← you are here
├── Cargo.toml                 # Rust workspace root
├── package.json               # Frontend (SvelteKit) root
├── crates/
│   ├── sshx-core/             # Shared proto + types (library)
│   ├── sshx/                  # CLI client binary
│   └── sshx-server/           # HTTP/WebSocket/gRPC server binary
├── src/                       # Svelte frontend (SvelteKit)
│   ├── lib/                   # Core logic and reusable components
│   │   ├── Session.svelte     # Central session component (main UI state machine)
│   │   ├── encrypt.ts         # Client-side E2E encryption
│   │   ├── srocket.ts         # Reconnecting WebSocket with CBOR-X
│   │   ├── protocol.ts        # TypeScript type aliases for WS protocol
│   │   ├── settings.ts        # Persisted user settings store
│   │   ├── arrange.ts         # Placement algorithm for new terminals
│   │   ├── lock.ts            # Async mutex for sequential encryption
│   │   ├── toast.ts           # Toast notification helpers
│   │   ├── typeahead.ts       # xterm.js typeahead addon
│   │   ├── action/            # Svelte custom actions (slide, touchZoom)
│   │   └── ui/                # All UI components (XTerm, Chat, Toolbar…)
│   └── routes/                # SvelteKit pages (+page.svelte, s/[id]/…)
├── scripts/                   # Build helpers (release.sh)
├── static/                    # Static assets served by the server
├── build/                     # Compiled frontend output (committed)
├── compose.yaml               # Docker Compose for local dev
├── Dockerfile                 # Production image
├── Tailscale.md               # Deployment notes for Tailscale
└── mprocs.yaml                # Multi-process dev runner config
```

---

## Architecture Overview

```
┌────────────────────────────────────────────────────────────┐
│  User's machine                                            │
│  sshx CLI (crates/sshx)                                   │
│    - Spawns a PTY shell                                    │
│    - Streams encrypted output via gRPC (Tonic)            │
│    - Accepts encrypted input from server                  │
└────────────────────┬───────────────────────────────────────┘
                     │ gRPC / HTTP2 (sshx.proto)
┌────────────────────▼───────────────────────────────────────┐
│  sshx-server (crates/sshx-server)                         │
│    - Axum HTTP server (port 8051)                         │
│    - gRPC endpoint for CLI clients                        │
│    - WebSocket endpoint for browsers (/api/s/{name})      │
│    - In-memory session store (DashMap)                    │
│    - Optional Redis mesh for horizontal scaling           │
│    - Serves compiled Svelte app from build/               │
└────────────────────┬───────────────────────────────────────┘
                     │ WebSocket + CBOR-X binary protocol
┌────────────────────▼───────────────────────────────────────┐
│  Browser (src/ — SvelteKit + Svelte)                      │
│    - Session.svelte: main state machine                   │
│    - xterm.js terminals on an infinite canvas             │
│    - Client-side decryption (same key, URL fragment)      │
│    - Real-time cursors, chat, resize/move                 │
└────────────────────────────────────────────────────────────┘
```

---

## Key Files by Concern

### Encryption (end-to-end, symmetric)
| File | Role |
|---|---|
| `crates/sshx/src/encrypt.rs` | Rust: Argon2id KDF + AES-CTR encryption |
| `crates/sshx-core/src/lib.rs` | Rust: `Sid`, `Uid`, `IdCounter` shared types |
| `src/lib/encrypt.ts` | TS: Mirror encryption using Web Crypto API + argon2-browser |

Encryption key lives only in the URL fragment (`#<key>`) — never sent to server.

### Protocol
| File | Role |
|---|---|
| `crates/sshx-core/proto/sshx.proto` | gRPC service + message definitions |
| `crates/sshx-core/src/lib.rs` | Prost-generated types, `Sid`/`Uid` wrappers |
| `src/lib/protocol.ts` | TypeScript types mirroring WebSocket protocol |
| `crates/sshx-server/src/web/protocol.rs` | Rust `WsServer`/`WsClient` enum definitions |

### WebSocket layer (browser ↔ server)
| File | Role |
|---|---|
| `crates/sshx-server/src/web/socket.rs` | Server WebSocket handler |
| `src/lib/srocket.ts` | Client reconnecting WebSocket wrapper |
| `src/lib/Session.svelte` | Processes all incoming WS messages, dispatches WsClient |

WebSocket messages are serialized with **CBOR-X** (binary, compact). Close code `4404` = session not found, `4500` = server error.

### Session & State (server-side)
| File | Role |
|---|---|
| `crates/sshx-server/src/state.rs` | Global `ServerState`, session lookup, HMAC signing |
| `crates/sshx-server/src/session/session.rs` | Per-session: shells, users, broadcast channel |
| `crates/sshx-server/src/state/mesh.rs` | Redis pub/sub for distributed mesh |

### gRPC (CLI ↔ server)
| File | Role |
|---|---|
| `crates/sshx-server/src/grpc.rs` | `SshxService` implementation |
| `crates/sshx/src/controller.rs` | gRPC client, reconnect logic, heartbeat |
| `crates/sshx/src/runner.rs` | Terminal I/O streaming, rolling buffer |

### Frontend UI
| File | Role |
|---|---|
| `src/lib/Session.svelte` | **Main component**: canvas, state, all WS message handlers |
| `src/lib/ui/XTerm.svelte` | xterm.js terminal widget |
| `src/lib/ui/Chat.svelte` | Chat panel |
| `src/lib/ui/Toolbar.svelte` | Top menu bar |
| `src/lib/ui/LiveCursor.svelte` | Real-time cursor per user |
| `src/lib/ui/Settings.svelte` | User preference panel |
| `src/lib/ui/themes.ts` | xterm.js color themes |
| `src/routes/s/[id]/+page.svelte` | Session page route |

---

## Communication Protocols

### gRPC (CLI → Server)
```
Open(OpenRequest)                  → Creates session, returns session ID + HMAC token
Channel(stream ClientUpdate)       → Bidirectional: terminal data, sizes, pong
  ClientUpdate: chunks | newShell | closeShell | setWinsize | pong
  ServerUpdate: chunks | newShell | closeShell | requestSync | ping
Close(CloseRequest)                → Graceful shutdown
```

### WebSocket (Browser → Server) — CBOR-X binary
```
Client → Server: Authenticate | SetName | SetCursor | SetFocus |
                 Create | Close | Move | Data | Subscribe | Chat | Ping
Server → Client: Hello | InvalidAuth | Users | UserDiff | Shells |
                 Chunks | Hear | ShellLatency | Pong | Error
```

---

## Important Constants & Defaults

| Constant | Value | Location |
|---|---|---|
| Server port | `8051` | `crates/sshx-server/src/main.rs` |
| Shell output buffer | 2 MB per shell | `crates/sshx-server/src/session/session.rs` |
| Client rolling buffer | 8–12 MB | `crates/sshx/src/runner.rs` |
| Session expiry (disconnected) | 5 minutes | `crates/sshx-server/src/state.rs` |
| Redis session expiry | 5 minutes | `crates/sshx-server/src/state/mesh.rs` |
| gRPC heartbeat | 2 seconds | `crates/sshx/src/controller.rs` |
| gRPC reconnect interval | 60 seconds | `crates/sshx/src/controller.rs` |
| Reconnect backoff | 2–60 seconds (exponential) | `crates/sshx/src/controller.rs` |
| WS broadcast buffer | 64 messages | `crates/sshx-server/src/session/session.rs` |
| Argon2 memory | 19×1024 KiB | `crates/sshx/src/encrypt.rs` |

---

## Development Setup

```bash
# Start all services (backend + frontend watcher)
mprocs

# Or manually:
cargo run -p sshx-server              # Start server
npm run dev                           # Start Svelte dev server
cargo run -p sshx -- --server http://localhost:8051  # Test client

# Build frontend
npm run build

# Run tests
cargo test
```

See `mprocs.yaml` for exact dev commands and `Tailscale.md` for deployment notes.

---

## Extension Guide

### Adding a new WebSocket message type
1. Add variant to `WsServer` or `WsClient` in `crates/sshx-server/src/web/protocol.rs`
2. Mirror it in `src/lib/protocol.ts`
3. Handle it in `crates/sshx-server/src/web/socket.rs` (server side)
4. Handle it in `src/lib/Session.svelte` (client side)

### Adding a new gRPC message type
1. Edit `crates/sshx-core/proto/sshx.proto`
2. Run `cargo build -p sshx-core` to regenerate Prost types
3. Update `crates/sshx-server/src/grpc.rs` and `crates/sshx/src/controller.rs`

### Adding a new UI component
1. Create `src/lib/ui/MyComponent.svelte`
2. Import and use in `Session.svelte` or relevant parent
3. Style with Tailwind CSS (dark theme, zinc palette)

### Adding server-side session state
1. Extend `Session` struct in `crates/sshx-server/src/session/session.rs`
2. If it needs persistence/migration, update `SerializedSession` in the proto
3. Update `StorageMesh` snapshot logic in `crates/sshx-server/src/state/mesh.rs`

---

## UI Design Principles

The sshx frontend is built around a dark, dense, functional aesthetic — closer to a professional IDE than a consumer app. These principles govern all UI decisions.

### Visual language

- **Dark zinc palette** — `zinc-900` backgrounds, `zinc-800` panels, `zinc-700` borders, `zinc-300` text. Avoid pure black or pure white.
- **Opacity as depth** — idle panels sit at `opacity-90`; focused/hovered panels go `opacity-100`. Gives the canvas a layered feel without heavy shadows.
- **Rounded corners** — `rounded-lg` on all floating panels (terminals, file cards, file tree). `rounded-xl` for the global `.panel` utility class.
- **`border border-zinc-700`** on every floating element — thin, subtle boundary that separates layers without shouting.

### Window chrome

All movable/floating elements share the same title-bar structure (mirroring macOS window chrome):

```
┌─────────────────────────────────────────┐
│  ● ● ●  │   centered title   │  (info)  │
│ circle  │   text-sm zinc-300  │  flex-1  │
│ buttons │   flex-grow-[4]     │  spacer  │
└─────────────────────────────────────────┘
```

- **CircleButtons** (red/yellow/green) on the left — red = close, yellow = collapse/shrink, green = expand. Implemented via `CircleButton` + `CircleButtons` components.
- **Centered title** — `w-0 flex-grow-[4]` so it expands into the middle; `overflow-hidden whitespace-nowrap text-ellipsis` for long paths.
- **Right slot** — symmetric `flex-1` spacer, optionally holds secondary info (file count, etc.).
- The entire title bar is the drag handle (`on:mousedown → dispatch("startMove")`).

This applies to: `XTerm.svelte`, `FileCard.svelte`, `FileTreePanel.svelte`.
Exception: `StickyNote.svelte` uses a colored post-it aesthetic by design.

### Fixed UI (HUD)

Panels that are **not** on the canvas (chat, Claude activity) live in a single bottom-right column container, anchored `absolute bottom-4 right-4`:

- **Chat** sits above, `flex-1 min-h-0` — expands to fill available space.
- **Claude Activity** sits below, `flex-shrink-0` — fixed height with its own collapse button.
- Container is `pointer-events-none`; each panel inside is `pointer-events-auto`.
- Max height: `calc(100vh - 80px)` to stay clear of the toolbar.

### Canvas interaction

- **Ctrl/Meta + scroll** = zoom (fast: ~44% per mouse-wheel notch, smooth on trackpad).
- **Scroll** = pan vertically; **Shift + scroll** = pan horizontally (non-macOS).
- **Alt + scroll** = zoom (same path as ctrl).
- Zoom range: `MIN_ZOOM` to `MAX_ZOOM` (clamped), centered on the cursor position.

### Typography

- UI text: system font via Tailwind defaults.
- Terminal / code: `Fira Code VF` (variable font), `font-size: 14px`, loaded via `FontFaceObserver`.
- File paths and identifiers in panels: `font-mono`.

### Tone

- Panels are **small and dense** — pack information, avoid padding waste.
- Avoid decorative icons; prefer text labels or simple SVG primitives.
- Animations: `transition-opacity duration-200` for show/hide; no bouncy or playful effects.
- Error/status colors: `emerald-400` (ok), `red-400` (error), `indigo-400` (AI/Claude actions), `yellow-300` (tools/directories).

---

## Security Notes

- Encryption key is in the **URL fragment** — never logged or sent to server
- Server stores only encrypted ciphertext
- `encrypted_zeros` is the authentication proof (client has the key)
- Use **constant-time comparison** for all security-sensitive checks (see `socket.rs`)
- Write passwords are bcrypt-hashed server-side
- Token signing uses **HMAC-SHA256** with a per-server secret

---

## Claude Activity Feed — Implementation Notes

The activity feed pipelines Claude Code JSONL transcripts to the browser in real time.

**Path encoding**: Claude Code stores transcripts under `~/.claude/projects/<encoded-cwd>/`, where the CWD is encoded by replacing `/` with `-` — including the leading slash, so paths always start with `-` (e.g. `-home-user-repos-sshx`). Do **not** strip the leading dash.

**Retry loop**: `run_claude_tracker` (`workspace.rs`) loops forever — retries every 5 s when no transcript exists, and re-discovers after `tail_transcript` returns. This handles the case where sshx starts before Claude.

**History replay on connect**: `tail_transcript` reads and sends the last 200 lines of an existing transcript before entering tail mode, so the browser sees current-session history immediately.

**Server-side ring buffer**: `Session::claude_events` (`session.rs`) holds the last 200 `WsClaudeEvent` items. `socket.rs` replays them during the initial WebSocket handshake so reconnecting browsers see recent history without a new CLI connection.

---

## Releases & Self-Update

`sshx update` (and a background check at session start, at most hourly) installs the latest client build published by the server it points to (`--server`, default baked in at build time via `SSHX_BUILD_URL`).

- **Version**: the workspace version in `Cargo.toml`, compared as `major.minor.patch`. Each publish bumps the patch (`--bump minor|major|none` otherwise); commit the bump afterwards.
- **Publishing**: `scripts/publish-release.sh` builds, gzips and writes `releases/manifest.json` (per-platform `version`, `url`, `sha256`); the server serves `./releases` at `/releases`. Keeps the last 3 builds per platform. The landing page (`src/routes/+page.svelte`) reads the manifest at runtime to show the latest version and download links.
- **Linux builds use glibc 2.28 via `cargo zigbuild`, not musl**: musl ignores nsswitch and cannot resolve Tailscale MagicDNS names when systemd-resolved is only reachable through nss-resolve.
- **Client logic**: `crates/sshx/src/update.rs`. Auto-update is skipped for binaries run from a cargo `target/` directory; opt out with `--no-auto-update` / `SSHX_NO_AUTO_UPDATE=1` (boolish: `1`, `true`, `yes`…).
- No client/server compatibility check is performed.

---

## Platform Integration & Per-Terminal Agent State (branch `platform`)

The multi-user platform lives in `../sshx-dev-platform` (see its `docs/architecture.md`). sshx stays
usable on its own; these additions are inert unless a platform is configured or hooks installed.

- **Shell environment** (`runner.rs`, `terminal/unix.rs`): every PTY gets `SSHX_SESSION`,
  `SSHX_TERMINAL_ID` and `SSHX_HOOK_SOCKET`. `Runner::run` takes a `ShellContext` (env + registry).
- **`agent.rs`**: `TerminalRegistry` (per terminal: PID, foreground process group, cwd, git info,
  project, agent state), the private hook socket (`$XDG_RUNTIME_DIR` or `$TMPDIR`/`sshx-<uid>/`,
  modes 0700/0600), the Claude Code state machine `next_agent_info`, and a 3 s poller (`ps`, and
  `/proc` or `lsof` for cwd). The shell is "at its prompt" when it leads the foreground process
  group; an agent is dropped after 10 s quiet at the prompt.
- **`sshx hook <agent>`** (hidden): forwards a hook payload from stdin to the socket. Handled in
  `main()` before the runtime starts. Must never print, block or fail.
- **`claude_hooks.rs`** / `sshx claude-hooks install|uninstall|status`: idempotent edit of
  `~/.claude/settings.json` (honours `CLAUDE_CONFIG_DIR`, keeps key order, writes a
  `.sshx-backup`). Hooks are synchronous so events arrive in order.
- **`platform.rs`** / `sshx platform enroll|status|forget`, `--platform`, `--platform-token`,
  `--no-platform`: registers the session and reports terminals every change (debounced) or 15 s.
  `enroll` saves the sshx server announced by the platform (`whoami.sshxServer`) and installs the
  Claude Code hooks (`--no-claude-hooks` to skip), so a plain `sshx` then works. `--server` is
  optional: explicit flag/`SSHX_SERVER`, else the enrolled server, else the built-in default.
  `status` checks every link (enrollment, token, sshx server, online sessions, hooks and their
  binary, and the hook socket when run inside an sshx terminal) and exits with 1 on failure.
  Machine token = `smt_<bearer>.<machine key>`; only the bearer part is sent. The session key and
  terminal metadata are sealed with AES-256-GCM (`seal`/`open`, AAD per slot). Keep the format
  byte-compatible with the platform's `src/lib/crypto.ts` (shared test vector).
- **IDE connection token** (`ide.rs` `connection_token`, `src/lib/ideToken.ts`): OpenVSCode now
  requires a token derived from the session key, passed with `--connection-token-file`; the IDE
  widget appends `?tkn=`. Previously anyone knowing the session name could open the IDE.


---

## Image Annotations

Pasted images (`ImageWidget.svelte`) are saved by the CLI under `.sshx/images/<name>`. The title
bar has a copy button (relative path, for pasting into a terminal / Claude) and, with write access,
an annotate button: freehand drawing and text, stored as vectors in the image's pixel space
(`WsWidgetKind::Image.annotations`, JSON) so they stay editable. Saving flattens them into a PNG
(`src/lib/imageAnnotations.ts`), uploads it and sends `saveImageAnnotations`; the server pushes it to
the CLI as `.sshx/images/<stem>-annotated.png`, and the copy button then copies that path. Keep
`sanitizeImageName`/`annotatedFileName` in step with `sanitize_image_name` (`workspace.rs`) and
`annotated_file_name` (`socket.rs`).

---

## Undo / Redo

⌘Z / Ctrl+Z undoes, ⌘⇧Z / Ctrl+Y redoes, per browser tab (`src/lib/history.ts`, wired in
`Session.svelte`). Ignored while typing in an input/editor or when a terminal has focus.

- **Steps**: `create` / `delete` / `update` / `batch` on an `ObjectKind` (note, textBlock, drawing,
  widget, slide, slideOrder, shell, shellName, video). Replayed as ordinary messages, so other users
  see the change. Updates only re-apply the fields that changed (`applyChange`), so concurrent edits
  to other fields survive.
- **Same IDs**: deleted objects come back under their original ID via `restoreNote`,
  `restoreTextBlock`, `restoreDrawing`, `restoreSlide`, `restoreWidget` (server checks the ID was
  already issued; widgets limited to fileTree / fileCard / image).
- **Creations**: the server picks the ID, so `sendCreate` registers what was sent and
  `CreateTracker` pairs it with the next new object of that kind that matches (position, path, URL).
  New create paths must go through `sendCreate`; automatic ones (Claude auto-open) must not.
- **Gestures**: call `beginGesture(kind, id)` when a drag/resize starts and `endGesture()` when it
  ends — the server echoes intermediate positions, so the start state must be captured up front.
- Text edits coalesce (`{ coalesce: true }`); a paste or a multi-selection delete is one step.
- Not undoable: creating/closing terminals, IDE / Claude feed widgets, chat, screen shares.
