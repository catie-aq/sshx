# sshx-server — Backend Server

**Type:** Binary crate
**Role:** The central server that brokers between CLI clients (via gRPC) and browser clients (via WebSocket). Also serves the compiled Svelte frontend.

## What It Does

1. Listens on a single TCP port (default **8051**)
2. Serves the compiled Svelte app from `build/` (static files + gzip/brotli)
3. Accepts gRPC connections from `sshx` CLI clients
4. Accepts WebSocket connections from browsers at `/api/s/{name}`
5. Forwards encrypted terminal data between CLI clients and browser subscribers
6. Tracks user presence, cursors, and chat messages
7. Optionally distributes sessions across multiple nodes via Redis

## Directory Structure

```
sshx-server/
├── Cargo.toml
└── src/
    ├── main.rs            # Entry point, CLI args, graceful shutdown
    ├── lib.rs             # Server struct, bind(), background tasks
    ├── grpc.rs            # gRPC SshxService implementation
    ├── utils.rs           # Utility functions
    ├── listen.rs          # TCP listener helpers
    ├── state.rs           # Global state: session map, HMAC signing
    ├── session/
    │   └── session.rs     # Per-session state: shells, users, channels
    ├── state/
    │   └── mesh.rs        # Redis-backed distributed mesh
    └── web/
        ├── web.rs         # Axum router setup, static file serving
        ├── protocol.rs    # WsServer/WsClient enum definitions
        └── socket.rs      # WebSocket upgrade + message handler
```

## Key Modules

### `main.rs` — Entry Point

CLI arguments:
| Flag | Default | Description |
|---|---|---|
| `--port` | `8051` | TCP listen port |
| `--listen` | `0.0.0.0` | Bind address |
| `--secret` | random | HMAC secret for token signing |
| `--override-origin` | none | Force a specific origin URL |
| `--redis-url` | none | Enable mesh mode via Redis |
| `--host` | none | Public hostname for mesh node identity |

### `lib.rs` — Server Orchestration

The `Server` struct wraps `ServerState` and owns:
- TCP listener
- Background tasks: session sync (every 20s), expiry cleanup (every 1 min)

`Server::bind()` sets `TCP_NODELAY` for low latency.

### `state.rs` — Global State

`ServerState` contains:
- `DashMap<String, Arc<Session>>`: lock-free concurrent session map
- HMAC-SHA256 key for signing session tokens
- Optional `StorageMesh` for Redis integration

Key operations:
- `find_or_create_session()`: Atomic session lookup or creation
- `sign_token()` / `verify_token()`: HMAC-SHA256 with constant-time comparison
- Session expiry: Disconnected sessions removed after **5 minutes**

### `session/session.rs` — Per-Session State

`Session` holds:
- `Metadata`: `encrypted_zeros`, `name`, `write_pass` hash
- `shells`: `HashMap<Sid, ShellState>`
- `users`: `HashMap<Uid, WsUser>`
- `broadcast`: Tokio broadcast channel (64-message buffer) for WebSocket subscribers
- `update_tx`: Buffered channel (256) for sending updates to the gRPC client

`ShellState` holds:
- `seqnum`: Output sequence number
- `data`: `Vec<Bytes>` — rolling buffer of output chunks (max **2 MB**)
- `winsize`: Current `WsWinsize` (x, y, rows, cols)
- `closed`: Flag

### `grpc.rs` — gRPC Service Implementation

Implements `SshxService` from `sshx-core`:

**`open()`:**
1. Generate 10-char alphanumeric session ID
2. Validate `encrypted_zeros` (auth check)
3. Store session in `ServerState`
4. Return HMAC-signed `token` + shareable `url`

**`channel()`:**
1. Verify token via HMAC
2. Start bidirectional stream
3. Fan out `ServerUpdate::chunks` from WebSocket users → CLI client
4. Store incoming `ClientUpdate::chunks` in `ShellState`
5. Broadcast new chunks to all WebSocket subscribers
6. Send ping every **2 seconds**, request sync every **5 seconds**

**`close()`:**
1. Verify token
2. Remove session from state
3. Notify all WebSocket subscribers with close signal

### `web/web.rs` — HTTP Router

Built on **Axum**:
- `GET /api/s/{name}` → WebSocket upgrade (session connection)
- Everything else → SvelteKit build files from `build/`
- Supports precompressed `.gz` and `.br` files
- gRPC and HTTP share the same port via `tower` service splitting

### `web/protocol.rs` — WebSocket Message Types

**`WsServer`** (server → browser):
```
Hello(Uid, String)              — user ID + session name
InvalidAuth()                   — bad encrypted_zeros
Users(Vec<(Uid, WsUser)>)       — full user list on join
UserDiff(Uid, Option<WsUser>)   — user join/leave/change
Shells(Vec<(Sid, WsWinsize)>)   — full shell list
Chunks(Sid, u64, Vec<Bytes>)    — terminal output chunks
Hear(Uid, String, String)       — chat message
ShellLatency(u64)               — backend round-trip time
Pong(u64)                       — echo of client's Ping
Error(String)                   — application error
```

**`WsClient`** (browser → server):
```
Authenticate(bytes, Option<String>)  — encrypted_zeros + write pass
SetName(String)                      — display name
SetCursor(Option<(f64, f64)>)        — cursor position on canvas
SetFocus(Option<Sid>)                — which shell is focused
Create(f64, f64)                     — create new shell at canvas position
Close(Sid)                           — close a shell
Move(Sid, WsWinsize)                 — move/resize a shell window
Data(Sid, Bytes, u64)                — send input to a shell
Subscribe(Sid, u64)                  — subscribe to shell output from offset
Chat(String)                         — send a chat message
Ping(u64)                            — latency measurement
```

Serialized with **CBOR-X** (binary, more compact than JSON).

### `web/socket.rs` — WebSocket Handler

`handle_socket()` main loop:
1. Wait for `Authenticate` message
2. Verify `encrypted_zeros` with constant-time comparison
3. Optionally verify write password (bcrypt)
4. Assign new `Uid`, send `Hello` + full state snapshot
5. Start concurrent loops:
   - Receive `WsClient` messages → process (Create, Close, Move, Data, Chat…)
   - Receive `WsServer` broadcasts → forward to browser
   - Subscribe to shell output chunks on demand
6. On disconnect: send `UserDiff(uid, None)` to remaining users

**Proxy mode:** If a session lives on a different mesh node, redirect browser via close code + `Location` header instead of handling locally.

### `state/mesh.rs` — Redis Distributed Mesh

`StorageMesh` uses a **deadpool** Redis connection pool (10 connections).

Redis key patterns:
```
session:{name}:owner     → which server node owns this session
session:{name}:snapshot  → serialized session state (SerializedSession)
```

- Session snapshots synced to Redis every **20 seconds**
- Expiry: **5 minutes** in Redis
- Pub/sub channel: transfer notifications between nodes
- If a browser connects to a non-owner node: either redirect or migrate session

## Dependencies

| Crate | Purpose |
|---|---|
| `axum` | HTTP server + WebSocket |
| `tonic` | gRPC server |
| `tokio` | Async runtime |
| `dashmap` | Lock-free concurrent HashMap |
| `parking_lot` | Faster Mutex/RwLock |
| `ciborium` | CBOR serialization (WebSocket protocol) |
| `redis` / `deadpool-redis` | Distributed mesh storage |
| `hmac` / `sha2` | Token signing |
| `prost` | Protobuf encoding |
| `sshx-core` | Shared types + proto |
| `bytes` | Zero-copy byte buffers |

## Extension Points

- **New WebSocket message:** Add variant to `web/protocol.rs`, handle in `web/socket.rs`
- **New session metadata:** Add to `Session`/`Metadata` in `session/session.rs`; update `SerializedSession` if it needs to persist
- **New HTTP route:** Add to Axum router in `web/web.rs`
- **New server config flag:** Add to `ServerOptions` in `lib.rs` and parse in `main.rs`
- **Distributed state field:** Extend `SerializedSession` proto + `state/mesh.rs`
