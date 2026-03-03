# sshx — CLI Client

**Type:** Binary crate
**Binary name:** `sshx`
**Role:** The command users run on their machine to share a terminal session.

## What It Does

1. Parses CLI arguments
2. Spawns a PTY shell subprocess (bash, zsh, etc.)
3. Connects to the sshx server via gRPC (bidirectional streaming)
4. Streams encrypted terminal output to the server
5. Receives encrypted input from the server and writes it to the PTY
6. Displays the shareable URL to the user
7. Reconnects automatically on network interruptions

## Directory Structure

```
sshx/
├── Cargo.toml
└── src/
    ├── main.rs         # CLI entry point, argument parsing
    ├── controller.rs   # gRPC client, session lifecycle, reconnection
    ├── runner.rs       # Terminal I/O streaming, rolling output buffer
    ├── encrypt.rs      # Argon2id KDF + AES-128-CTR encryption
    ├── terminal.rs     # PTY management (delegates to platform impl)
    └── terminal/
        ├── unix.rs     # Unix PTY via nix (ioctl, signals)
        └── windows.rs  # Windows PTY via conpty
```

## Key Modules

### `main.rs` — Entry Point

CLI arguments:
| Flag | Default | Description |
|---|---|---|
| `--server` | `https://sshx.io` | Server URL |
| `--shell` | `$SHELL` or `bash` | Shell to spawn |
| `--quiet` | false | Suppress greeting output |
| `--name` | hostname | Human-readable session name |
| `--enable-readers` | false | Allow unauthenticated readers |

Flow:
1. Generate 14-char random encryption key
2. Create `Terminal` (PTY)
3. Create `Controller` (gRPC client)
4. Call `controller.run()` — blocks until session ends
5. Print shareable URL on start

### `controller.rs` — gRPC Client & Session Manager

The `Controller` struct owns:
- gRPC channel to the server
- Encryption key + `encrypted_zeros` proof
- Map of active shell runners

**Session open:** Calls `Open()`, receives session `name` + `token` + `url`.

**Channel loop:** Calls `Channel()`, starts bidirectional streaming:
- Spawns one `Runner` task per shell
- Forwards `ServerUpdate` messages to the appropriate `Runner`
- Aggregates `ClientUpdate` messages from all `Runner`s
- Sends a heartbeat every **2 seconds**
- Reconnects on error with **exponential backoff (2–60s)**

**Reconnection:** Uses the stored `token` for re-authentication. Restores shell runners across reconnects.

### `runner.rs` — Terminal I/O

The `Runner` manages one PTY shell:

- **Output path:** PTY → chunked (max 64KB) → encrypted → `ClientUpdate::chunks`
- **Input path:** `ServerUpdate::chunks` → decrypted → PTY stdin
- **Rolling buffer:** Keeps last 8–12 MB of output for sync requests
- **UTF-8 streaming:** Correct chunking at character boundaries
- **Sync:** On `request_sync`, resends buffered data from the requested sequence number
- **3-strike rule:** Three consecutive sync failures → session terminates

Runner types:
- `Shell(String)`: Live PTY shell — main mode
- `Echo`: Loopback test runner

### `encrypt.rs` — Encryption

```
Key derivation: Argon2id(password=<14-char key>, salt=<public constant>)
                → 16-byte symmetric key

Encryption: AES-128-CTR
  - Stream number encodes: upper 32 bits = stream identity, lower 32 = byte offset
  - Different stream numbers for each shell (prevents nonce reuse)

encrypted_zeros: AES-CTR(key, stream=1, zeros) — used to prove key ownership
```

Public constants (identical in Rust and TypeScript):
- Argon2 params: memory=19×1024 KiB, iterations=2, parallelism=1
- Salt: `"This is a non-random salt for sshx.io..."`

### `terminal.rs` — PTY Management

Platform-agnostic interface:
```rust
Terminal::new(shell: &str) -> Terminal
terminal.set_winsize(rows, cols)
terminal.get_winsize() -> (rows, cols)
```

- **Unix** (`terminal/unix.rs`): Uses `nix` for `posix_openpt`, `ioctl`, `SIGWINCH`
- **Windows** (`terminal/windows.rs`): Uses `conpty` crate

## Dependencies

| Crate | Purpose |
|---|---|
| `tokio` | Async runtime |
| `tonic` | gRPC client |
| `sshx-core` | Protocol types |
| `aes`, `ctr` | AES-CTR cipher |
| `argon2` | Key derivation |
| `nix` | Unix PTY operations |
| `conpty` | Windows PTY |
| `rand` | Key generation |

## Extension Points

- **Add a new CLI flag:** Edit `main.rs` (uses `clap`)
- **Change encryption params:** Edit `encrypt.rs` (must match `src/lib/encrypt.ts`)
- **Add a new `ClientUpdate` variant:** Edit `runner.rs` + proto
- **Add platform support:** Add a new file alongside `terminal/unix.rs`
