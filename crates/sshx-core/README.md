# sshx-core — Shared Protocol & Types

**Type:** Library crate (no binary)
**Used by:** `sshx` (CLI client) and `sshx-server`

## Purpose

`sshx-core` contains everything shared between the CLI client and the server:
- gRPC service and message definitions (Protocol Buffers)
- Rust types generated from the proto (`Tonic` + `Prost`)
- Shared ID primitives (`Sid`, `Uid`, `IdCounter`)

## Directory Structure

```
sshx-core/
├── Cargo.toml
├── build.rs          # Calls tonic_build to generate Rust from .proto
├── proto/
│   └── sshx.proto    # The canonical protocol definition
└── src/
    └── lib.rs        # Re-exports generated types + Sid/Uid/IdCounter
```

## Proto Service (`proto/sshx.proto`)

### gRPC Service: `SshxService`

| Method | Request | Response | Description |
|---|---|---|---|
| `Open` | `OpenRequest` | `OpenResponse` | Create a new session |
| `Channel` | `stream ClientUpdate` | `stream ServerUpdate` | Bidirectional terminal I/O streaming |
| `Close` | `CloseRequest` | `CloseResponse` | Gracefully terminate a session |

### Key Message Types

**`OpenRequest`**
- `origin`: Client's preferred server origin URL
- `encrypted_zeros`: Proof that client has the encryption key
- `name`: Optional human-readable session name
- `write_pass`: Optional write-protection password (plaintext, hashed server-side)

**`OpenResponse`**
- `name`: Assigned 10-char session ID
- `token`: HMAC-signed authentication token (used in `Channel`/`Close`)
- `url`: Full URL to share with collaborators

**`ClientUpdate`** (client → server, streamed)
```
chunks      # Encrypted terminal output data
new_shell   # Request to open a new PTY
close_shell # Close a PTY
set_winsize # Resize a PTY window
pong        # Reply to server's ping
```

**`ServerUpdate`** (server → client, streamed)
```
chunks       # Input data to write into a PTY
new_shell    # Acknowledge new shell creation (with Sid)
close_shell  # Signal that a shell was closed
request_sync # Ask client to resend buffered output
ping         # Keepalive; client must pong
```

**`TerminalData`**
- `seq`: Sequence number (for sync and gap detection)
- `data`: Encrypted bytes (AES-CTR stream)

**`TerminalInput`**
- `offset`: CTR mode byte offset
- `data`: Encrypted input bytes

**`SerializedSession`**
- Full session snapshot for Redis-based migration between server nodes

## Shared Types (`src/lib.rs`)

```rust
pub struct Sid(pub u32);   // Shell ID — unique within a session
pub struct Uid(pub u32);   // User ID — unique within a session
pub struct IdCounter(AtomicU32);  // Thread-safe monotonic counter
```

These are lightweight wrappers used consistently across both crates.

## Code Generation

`build.rs` uses `tonic_build` to compile `proto/sshx.proto` into Rust types at build time. The generated code is emitted into `OUT_DIR` and included via `include_proto!("sshx")` in `lib.rs`.

gRPC reflection is enabled (for tools like `grpcurl`).

## Extension Points

To add a new field to the protocol:
1. Edit `proto/sshx.proto`
2. Run `cargo build -p sshx-core` — Rust types regenerate automatically
3. Update usage in `crates/sshx/src/controller.rs` (client) and `crates/sshx-server/src/grpc.rs` (server)
