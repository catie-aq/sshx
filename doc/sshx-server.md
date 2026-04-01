# sshx Server — Feature & Code Reference

> Crate: `crates/sshx-server/` — Axum HTTP + Tonic gRPC server that manages sessions, relays terminal data, and serves the Svelte frontend.

---

## Modules

| Module | File | Purpose |
|--------|------|---------|
| `main` | `src/main.rs` | CLI args, server bootstrap, signal handling |
| `lib` | `src/lib.rs` | `ServerOptions`, `Server` struct, background task spawning |
| `listen` | `src/listen.rs` | HTTP/gRPC multiplexer (Hyper + Tower Steer) |
| `grpc` | `src/grpc.rs` | `SshxService` + `BrowserService` gRPC implementations |
| `state` | `src/state.rs` | `ServerState`: global session store, HMAC signing, ICE servers |
| `state::mesh` | `src/state/mesh.rs` | `StorageMesh`: Redis pub/sub for horizontal scaling |
| `session` | `src/session.rs` | `Session`: per-session state — shells, users, notes, widgets, video, IDE |
| `session::snapshot` | `src/session/snapshot.rs` | Protobuf + JSON serialization for persistence & Redis sync |
| `web` | `src/web.rs` | Axum router: static files, upload, WebSocket upgrade |
| `web::protocol` | `src/web/protocol.rs` | `WsServer` / `WsClient` enums + all data types (CBOR-X) |
| `web::socket` | `src/web/socket.rs` | WebSocket handler: auth, replay, message routing |
| `web::ide_proxy` | `src/web/ide_proxy.rs` | HTTP/WebSocket tunnel proxy for OpenVSCode Server |
| `utils` | `src/utils.rs` | `Shutdown` struct (graceful termination) |

---

## Server CLI (`src/main.rs`)

```
sshx-server [OPTIONS]

Options:
  --port <PORT>           Listen port (default: 8051)
  --listen <ADDR>         Bind address (default: ::1)
  --secret <SECRET>       HMAC signing secret (env: SSHX_SECRET)
  --override-origin <URL> Override origin in Open() response
  --redis-url <URL>       Redis for distributed mesh (env: SSHX_REDIS_URL)
  --host <HOST>           This server's hostname (for mesh)
  --stun-servers <URLS>   Comma-separated STUN URLs
  --turn-server <SPEC>    url,username,credential (repeatable)
```

---

## Architecture

```
TCP listener (single port)
  │
  ├─ Tower Steer (Content-Type routing)
  │   ├─ application/grpc → Tonic (gRPC)
  │   │   ├─ SshxService  — CLI client RPCs
  │   │   └─ BrowserService — offscreen browser RPCs
  │   │
  │   └─ everything else → Axum (HTTP)
  │       ├─ GET /           → SPA (build/spa.html)
  │       ├─ GET /s/[id]     → SPA
  │       ├─ POST /api/s/{name}/upload → multipart image upload
  │       ├─ GET|POST /api/s/{name}    → WebSocket upgrade
  │       ├─ /ide/s/{name}/* → IDE proxy (HTTP tunnel or WS tunnel)
  │       ├─ /uploads/*      → serve uploaded images
  │       └─ /*              → static assets from build/
  │
  └─ Background tasks
      ├─ close_old_sessions() — every 60s, close sessions inactive > 5 min
      └─ listen_for_transfers() — Redis pub/sub for distributed mesh
```

---

## gRPC Service (`src/grpc.rs`)

### SshxService RPCs

| RPC | Request | Response | Purpose |
|-----|---------|----------|---------|
| `Open` | `OpenRequest` | `OpenResponse` | Create session, return name + token + URL |
| `Channel` | `stream ClientUpdate` | `stream ServerUpdate` | Bidirectional: terminal data, pings, snapshots |
| `Close` | `CloseRequest` | `CloseResponse` | Graceful session close |

### Channel Handler (`handle_streaming`, line 175)

Multiplexed `tokio::select!` loop:
- **Sync** (every 5s) — send sequence numbers to CLI
- **Ping** (every 2s) — measure latency
- **Snapshot** (every 30s) — send JSON session snapshot for CLI persistence
- **Session updates** — relay buffered `ServerMessage` variants to CLI
- **Client updates** — process incoming `ClientMessage` (see below)

### ClientMessage Variants (`handle_update`, line 248)

| Variant | Action |
|---------|--------|
| `Data(id, data, seq)` | Append encrypted terminal data to shell buffer |
| `CreatedShell(x, y)` | Register new shell |
| `ClosedShell(id)` | Mark shell closed |
| `SourceMetadata(bytes)` | Parse zstd-compressed JSON, store source files |
| `ClaudeEvent(bytes)` | Broadcast Claude activity to browsers |
| `ComponentGraph(bytes)` | Store workspace dependency graph |
| `WidgetNames(bytes)` | Apply names to Claude feed widgets |
| `Pong(ts)` | Measure CLI latency |
| `HttpTunnelResponse` / `WsTunnelFrame` | Deliver tunnel traffic to browser |
| `IdeState(bytes)` | Store IDE editor state for UID 0 (CLI) |
| `EditLockRequest` | Request/release collaborative edit lock |

### BrowserGrpcServer (`line 375`)

Handles offscreen browser (sshx-browser) RPCs: `Join`, `Stream`, `Leave`.

---

## Session State (`src/session.rs`)

### `Metadata` (line 33)
```rust
encrypted_zeros: Bytes     // auth proof
name: String               // human-readable
write_password_hash: Option<Bytes>  // bcrypt hash
```

### `Session` — Major State Groups

| Category | Fields | Purpose |
|----------|--------|---------|
| **Shells** | `shells: HashMap<Sid, State>`, `shell_names` | Terminal buffers (2 MiB max each), names |
| **Users** | `users: HashMap<Uid, WsUser>` | Connected browsers, cursor, focus, permissions |
| **Notes** | `notes: HashMap<Nid, WsNote>` | Sticky notes on canvas |
| **Text Blocks** | `text_blocks: HashMap<Tid, WsTextBlock>` | Rich text blocks |
| **Drawings** | `drawings: HashMap<Did, WsDrawing>` | Freehand strokes |
| **Slides** | `slides: HashMap<Slid, WsSlide>` | Presentation regions |
| **Widgets** | `widgets: HashMap<Wid, WsWidget>` | Canvas widgets (file tree, file card, graph, claude feed, image, overlay, IDE) |
| **Source Files** | `source_files`, `component_graph` | Workspace metadata + import graph |
| **Claude Events** | `claude_events: VecDeque<WsClaudeEvent>` | Ring buffer (max 200) for replay |
| **Video Streams** | `video_streams`, `browser_controllers`, `browser_frame_*` | Screen share + offscreen browser |
| **IDE** | `ide_states`, `edit_lock`, `ide_available` | Per-user editor state + collaborative locking |
| **Tunnels** | `tunnel_responses`, `ws_tunnels`, `next_tunnel_id` | HTTP/WS tunnel request demux |
| **Channels** | `broadcast`, `update_tx/rx`, watch senders | Real-time event distribution |
| **Lifecycle** | `last_accessed`, `shutdown`, `sync_notify` | Heartbeat, termination, Redis sync |

### Per-Shell State (line 162)
```rust
struct State {
    seqnum: u64,          // cumulative bytes received
    data: Vec<Bytes>,     // data chunks (pruned at 2 MiB)
    chunk_offset: u64,    // pruned chunk count
    byte_offset: u64,     // cumulative pruned bytes
    closed: bool,
    notify: Arc<Notify>,
}
```

### Channel Architecture

| Channel | Type | Capacity | Purpose |
|---------|------|----------|---------|
| `broadcast` | `broadcast::Sender<WsServer>` | 64 | WS messages → all browsers |
| `update_tx/rx` | `async_channel` | 256 | ServerMessage → gRPC CLI |
| `source` | `watch::Sender` | 1 | Shell list snapshot |
| `notes_source` | `watch::Sender` | 1 | Notes snapshot |
| `widget_source` | `watch::Sender` | 1 | Widgets snapshot |
| `source_files_source` | `watch::Sender` | 1 | File metadata snapshot |
| `video_streams_source` | `watch::Sender` | 1 | Video streams snapshot |

---

## WebSocket Protocol (`src/web/protocol.rs`)

### Data Types

| Type | Purpose |
|------|---------|
| `WsWinsize` | Terminal position (x, y) + size (rows, cols) |
| `WsUser` | User info (name, cursor, focus, canWrite) |
| `WsNote` | Sticky note (x, y, text, color, w, h, font, pinned) |
| `WsTextBlock` | Rich text (x, y, content, fontSize, color, align, font) |
| `WsDrawing` | Freehand stroke (tool, points, color, width, opacity) |
| `WsSlide` | Presentation region (x, y, w, h, order, label) |
| `WsSourceFile` | File metadata (path, kind, imports, exports, description, content preview) |
| `WsFileMetadataUpdate` | Partial file metadata patch |
| `WsClaudeEvent` | Claude Code transcript event |
| `WsWidgetKind` | FileTree \| FileCard \| GraphView \| ClaudeFeed \| Image \| AppOverlay \| IdeEditor |
| `WsWidget` | Canvas widget (x, y, w, h, kind, collapsed, name) |
| `WsVideoStream` | Screen share / browser stream (owner, label, is_browser, position, size) |
| `WsIceServer` | STUN/TURN config |
| `WsIceCandidate` | WebRTC ICE candidate |
| `WsComponentGraph` | File dependency graph (nodes, code_edges, display_edges) |
| `WsIdeState` | IDE editor state (open files, cursors, selections, sidebar, panel) |
| `WsEditLock` | Collaborative lock (holder, file, expires_at) |

### WsServer (server → browser) — 35+ variants
Key messages: `Hello`, `Users/UserDiff`, `Shells`, `Chunks`, `Hear`, `Notes/NoteDiff`, `TextBlocks/TextBlockDiff`, `Drawings/DrawingDiff`, `Slides/SlideDiff`, `SourceFiles`, `Widgets/WidgetDiff`, `ComponentGraph`, `ClaudeEvent`, `VideoStreams/VideoStreamDiff`, `RtcOffer/Answer/Ice`, `BrowserControlStatus`, `BrowserFrame`, `IdeStates/IdeStateDiff`, `EditLock`, `IceServers`, `IdeAvailable`.

### WsClient (browser → server) — 40+ variants
Key messages: `Authenticate`, `SetName/Cursor/Focus`, `Create/Close/Move/Data/Subscribe` (shells), `CreateNote/UpdateNote/DeleteNote`, `CreateTextBlock/UpdateTextBlock/DeleteTextBlock`, `CreateDrawing/DeleteDrawing`, `CreateSlide/UpdateSlide/DeleteSlide/ReorderSlides`, `OpenFileTree/OpenFileCard/MoveWidget/ResizeWidget/CloseWidget`, `DescribeFiles/UpdateFileMetadata`, `StartScreenShare/StopScreenShare`, `WatchStream/SendRtcOffer/Answer/Ice`, `BrowserInput/RequestBrowserControl`, `UpdateIdeState/RequestEditLock/ReleaseEditLock`, `Chat/Ping`.

---

## WebSocket Handler (`src/web/socket.rs`)

### Connection Handshake
1. Assign UID → send `Hello(uid, session_name)`
2. Send `IceServers`, `IdeAvailable`
3. Receive `Authenticate(encrypted_zeros, write_password)` — constant-time verify
4. Replay full state: users, shells, notes, text blocks, drawings, slides, widgets, source files, component graph, Claude events (last 200), video streams, browser control, IDE states, edit lock

### Main Loop
`tokio::select!` on: broadcast channel, shell/note/widget/source_file/video_stream watches, chunk subscribers, browser frame subscribers, and incoming client messages.

---

## Snapshots & Persistence (`src/session/snapshot.rs`)

### JSON Snapshot (CLI persistence → `~/.sshx/session.json`)
`JsonSessionSnapshot` includes: notes, text_blocks, drawings, slides, widgets, shells (with base64 data), shell_names, counter state.

### Protobuf Snapshot (Redis mesh)
- Shell data pruned to 32 KiB each
- Max total snapshot: 4 MiB
- Used by `StorageMesh::background_sync()` (every 20s)

---

## Redis Mesh (`src/state/mesh.rs`)

### Keys
- `session:{name}:owner` — hostname of session owner
- `session:{name}:snapshot` — protobuf session state
- `session:{name}:closed` — permanent close marker
- `transfers:{host}` — pub/sub channel for session transfers

### Behavior
- `background_sync()` — serialize + store every 20s (5-min TTL)
- `backend_connect()` — lookup local or restore from Redis snapshot
- `frontend_connect()` — lookup local or redirect to owner host
- `mark_closed()` — expire keys, set closed flag
- `notify_transfer()` / `listen_for_transfers()` — pub/sub for cross-host migration

---

## Key Constants

| Constant | Value | Location | Purpose |
|----------|-------|----------|---------|
| `DISCONNECTED_SESSION_EXPIRY` | 5 min | `state.rs:28` | Close inactive sessions |
| `SHELL_STORED_BYTES` | 2 MiB | `session.rs:30` | Max buffer per shell |
| `SYNC_INTERVAL` | 5 sec | `grpc.rs:28` | Sequence number sync to CLI |
| `PING_INTERVAL` | 2 sec | `grpc.rs:31` | Latency measurement |
| `SNAPSHOT_INTERVAL` | 30 sec | `grpc.rs:34` | JSON snapshot to CLI |
| `STORAGE_SYNC_INTERVAL` | 20 sec | `mesh.rs:14` | Redis snapshot sync |
| `STORAGE_EXPIRY` | 5 min | `mesh.rs:17` | Redis key TTL |
| `SHELL_SNAPSHOT_BYTES` | 32 KiB | `snapshot.rs:19` | Max shell data in protobuf snapshot |
| `MAX_SNAPSHOT_SIZE` | 4 MiB | `snapshot.rs:21` | Total snapshot limit |
| Broadcast capacity | 64 | `session.rs:196` | WsServer broadcast channel |
| Update capacity | 256 | `session.rs:188` | gRPC update channel |

---

## IDE Proxy (`src/web/ide_proxy.rs`)

Routes `/ide/s/{name}/*` to the CLI's OpenVSCode Server via gRPC tunnel:
- HTTP requests: serialize → `ServerMessage::HttpTunnelRequest` → CLI → local OpenVSCode → `HttpTunnelResponse` chunks → reassemble
- WebSocket: upgrade → bidirectional `WsTunnelFrame` messages via gRPC
- Tunnel IDs allocated by `session.next_tunnel_id()` (atomic u32)

---

## Tests (`tests/`)

| File | Purpose |
|------|---------|
| `simple.rs` | Basic RPC + HTTP GET smoke test |
| `snapshot.rs` | Snapshot → restore flow verification |
| `with_client.rs` | Full CLI + WebSocket integration |
| `screen_share.rs` | WebRTC signaling + video frame relay |
| `session_persistence.rs` | Redis mesh persistence |
| `common/mod.rs` | `TestServer` + `ClientSocket` helpers |

---

## Dependencies (Key Crates)

| Crate | Purpose |
|-------|---------|
| `axum` | HTTP server, WebSocket upgrade, multipart upload |
| `tonic` | gRPC server (SshxService, BrowserService) |
| `tower` + `tower-http` | Middleware: Steer, TraceLayer, ServeDir, compression |
| `dashmap` | Concurrent session store |
| `deadpool-redis` + `redis` | Redis connection pool + pub/sub |
| `hmac` + `sha2` + `subtle` | Token signing + constant-time auth |
| `ciborium` | CBOR serialization for WebSocket messages |
| `zstd` | Compression for source metadata |
| `parking_lot` | Fast mutexes/rwlocks |
| `async-channel` | Bounded async MPMC channel |
| `sshx-core` | Shared proto types + prost-generated code |
