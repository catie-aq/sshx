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

## Security Notes

- Encryption key is in the **URL fragment** — never logged or sent to server
- Server stores only encrypted ciphertext
- `encrypted_zeros` is the authentication proof (client has the key)
- Use **constant-time comparison** for all security-sensitive checks (see `socket.rs`)
- Write passwords are bcrypt-hashed server-side
- Token signing uses **HMAC-SHA256** with a per-server secret
