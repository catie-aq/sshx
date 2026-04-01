# sshx CLI Client — Feature & Code Reference

> Crate: `crates/sshx/` — The CLI binary that spawns a PTY, encrypts terminal I/O, and streams it to the server via gRPC.

---

## Modules

| Module | File | Purpose |
|--------|------|---------|
| `main` | `src/main.rs` | CLI entry point, `clap` args, session bootstrap |
| `controller` | `src/controller.rs` | gRPC client, session orchestration, infinite reconnection loop |
| `runner` | `src/runner.rs` | PTY I/O loop, rolling buffer, UTF-8 streaming, backpressure |
| `encrypt` | `src/encrypt.rs` | Argon2id KDF + AES-128-CTR symmetric encryption |
| `terminal` | `src/terminal.rs` | Cross-platform PTY abstraction (unix/windows) |
| `terminal::unix` | `src/terminal/unix.rs` | Fork + execv PTY driver (nix crate) |
| `terminal::windows` | `src/terminal/windows.rs` | ConPTY driver |
| `analyze` | `src/analyze.rs` | Workspace scanner — walks files, parses imports/exports, builds `AnalysisDb` |
| `workspace` | `src/workspace.rs` | File watcher, Claude JSONL tracker, source metadata streaming |
| `ide` | `src/ide.rs` | OpenVSCode Server management + IDE sync SSE server |
| `openvscode` | `src/openvscode.rs` | Auto-download & install of OpenVSCode Server binary |
| `tunnel` | `src/tunnel.rs` | HTTP/WebSocket tunnel for IDE forwarding through gRPC |

---

## CLI Arguments (`src/main.rs:21–86`)

```
sshx [OPTIONS] [COMMAND]

Options:
  --server <URL>          Server URL (default: https://sshx.io)
  --shell <CMD>           Shell to spawn (auto-detected if omitted)
  -q, --quiet             Suppress greeting output
  --name <NAME>           Session display name (default: user@host)
  --enable-readers        Allow read-only access via separate URL
  --workspace <PATH>      Workspace root for file analysis
  --no-workspace          Disable workspace features
  --no-claude-tracking    Disable Claude Code transcript tailing
  --with-browser <URL>    Launch offscreen browser (sshx-browser)
  --browser-bin <PATH>    Path to sshx-browser binary
  --openvscode-bin <PATH> Path to OpenVSCode Server binary
  --ide                   Enable IDE mode (auto-downloads OpenVSCode)

Subcommands:
  analyze                 Run workspace analysis → .sshx-analysis.json
```

---

## End-to-End Flow

```
CLI start
  ├─ Parse args → resolve shell (get_default_shell)
  ├─ (--ide) openvscode::ensure_binary() → download if missing
  │
  ├─ Controller::new()
  │   ├─ Generate 14-char encryption key (83.3 bits entropy)
  │   ├─ Argon2id(key) → 16-byte AES key
  │   ├─ gRPC Open(encrypted_zeros, name, write_password_hash, snapshot)
  │   └─ Receive session ID + token + URLs
  │
  ├─ Spawn background tasks
  │   ├─ spawn_source_analyzer() — watch .sshx-analysis.json
  │   ├─ spawn_claude_tracker() — tail Claude Code JSONL
  │   ├─ spawn_claude_pid_tracker() — poll for Claude process
  │   └─ send_widget_names() — restore persisted widget state
  │
  ├─ Print greeting with session URLs
  │
  └─ controller.run() — infinite reconnection loop
      └─ try_channel() — single gRPC stream session
          ├─ Heartbeat every 2s
          ├─ Forward output_tx messages to server
          └─ Handle ServerMessage variants:
              CreateShell → Terminal::new() + shell_task()
              Input → decrypt → write to PTY
              Resize → set_winsize()
              Sync → update sequence numbers
              DescribeFiles → call `claude --print`
              HttpTunnelRequest → lazy-start IDE, forward HTTP
              SessionSnapshot → save ~/.sshx/session.json
              ...
```

---

## Key Structs & Types

### `Controller` (`src/controller.rs:39`)
Main session orchestrator. Holds gRPC connection, encryption key, runner, per-shell channels, tunnel & IDE managers.

### `Runner` (`src/runner.rs:20`)
```rust
enum Runner { Shell(String), Echo }
```
`Shell` spawns a real PTY; `Echo` is a test mode.

### `ShellData` (`src/runner.rs:29`)
```rust
enum ShellData { Data(Vec<u8>), Sync(u64), Size(u32, u32) }
```
Input from server → shell channel.

### `Encrypt` (`src/encrypt.rs:13`)
Symmetric encryption: `new(key)` derives AES key via Argon2id; `segment(stream, offset, data)` encrypts/decrypts in CTR mode; `zeros()` creates auth proof.

### `Terminal` (`src/terminal/unix.rs:42`)
Fork-based PTY with `AsyncRead + AsyncWrite`. Cleans up child process on drop (SIGKILL + background reap).

### `AnalysisDb` / `AnalysisEntry` (`src/analyze.rs:55/69`)
Workspace metadata: per-file kind, imports, exports, descriptions, line count, content preview. Serialized as `.sshx-analysis.json`.

### `IdeManager` (`src/ide.rs:190`)
Manages OpenVSCode Server lifecycle: spawn on free port, seed settings, graceful stop.

### `HttpTunnel` (`src/tunnel.rs:17`)
Forwards HTTP/WebSocket requests from gRPC to local server (e.g., OpenVSCode). Chunks responses at 64 KB.

---

## Key Constants

| Constant | Value | Location | Purpose |
|----------|-------|----------|---------|
| `HEARTBEAT_INTERVAL` | 2 sec | `controller.rs:27` | gRPC keep-alive ping |
| `RECONNECT_INTERVAL` | 60 sec | `controller.rs:30` | Force reconnect cycle |
| `CONTENT_CHUNK_SIZE` | 64 KB | `runner.rs:15` | Max bytes per gRPC message |
| `CONTENT_ROLLING_BYTES` | 8 MB | `runner.rs:16` | Min buffered terminal output |
| `CONTENT_PRUNE_BYTES` | 12 MB | `runner.rs:17` | Prune trigger threshold |
| `CHUNK_SIZE` (tunnel) | 64 KB | `tunnel.rs:14` | HTTP response chunk size |
| `OPENVSCODE_VERSION` | `1.109.5` | `openvscode.rs:13` | Pinned version |
| Argon2 memory | 19×1024 KiB | `encrypt.rs:26` | KDF memory parameter |
| Argon2 iterations | 2 | `encrypt.rs:26` | KDF time parameter |

---

## Dependencies (Key Crates)

| Crate | Purpose |
|-------|---------|
| `tonic` | gRPC client (bidirectional streaming) |
| `sshx-core` | Shared proto types (`Sid`, `Uid`, `IdCounter`) |
| `aes` + `ctr` + `argon2` | Encryption stack |
| `nix` (unix) / `conpty` (windows) | PTY driver |
| `encoding_rs` | Streaming UTF-8 decoder for terminal output |
| `notify` | File system watcher (analysis JSON) |
| `walkdir` + `regex` | Workspace directory traversal + import parsing |
| `reqwest` | HTTP client for tunnel + OpenVSCode download |
| `axum` | Local IDE sync server |
| `tokio-tungstenite` | WebSocket tunnel forwarding |
| `zstd` | Compression for source metadata |
| `clap` | CLI argument parsing |

---

## Workspace Analysis (`src/analyze.rs`)

Scans the project directory, skipping `node_modules`, `.git`, `target`, `dist`, `build`, etc. Parses files with extensions: `rs`, `ts`, `tsx`, `js`, `jsx`, `svelte`, `json`, `py`, `go`.

For each file, extracts:
- **Kind**: component, hook, utility, config, type, other
- **Local imports / libraries / exports**
- **Line count, last modified, content preview** (first 6144 chars)
- **Component graph**: `GraphNode` + `GraphEdge` (import dependencies + display edges)

Output: `.sshx-analysis.json` — merged with existing to preserve AI-generated descriptions.

---

## Claude Tracking (`src/workspace.rs:255+`)

Tails `~/.claude/projects/<encoded-cwd>/` JSONL transcript:
- Path encoding: CWD with `/` → `-` (including leading slash)
- Parses role, content, timestamp, sessionId, usage tokens
- Sends `ClaudeEvent` messages to server for browser display
- Replays last 200 lines on startup
- Retries every 5s if transcript missing
