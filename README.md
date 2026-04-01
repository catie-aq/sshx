# sshx

> **CATIE fork:** for Tailscale/tailnet setup, see [Tailscale.md](Tailscale.md).

A secure web-based, collaborative terminal.

![](https://i.imgur.com/Q3qKAHW.png)

**Features:**

- Run a single command to share your terminal with anyone.
- Resize, move windows, and freely zoom and pan on an infinite canvas.
- See other people's cursors moving in real time.
- Connect to the nearest server in a globally distributed mesh.
- End-to-end encryption with Argon2 and AES.
- Automatic reconnection and real-time latency estimates.
- Predictive echo for faster local editing (à la Mosh).

Visit [sshx.io](https://sshx.io) to learn more.

## Installation

Just run this command to get the `sshx` binary for your platform.

```shell
curl -sSf https://sshx.io/get | sh
```

Supports Linux and MacOS on x86_64 and ARM64 architectures, as well as embedded
ARMv6 and ARMv7-A systems. The Linux binaries are statically linked.

For Windows, there are binaries for x86_64, x86, and ARM64, linked to MSVC for
maximum compatibility.

If you just want to try it out without installing, use:

```shell
curl -sSf https://sshx.io/get | sh -s run
```

Inspect the script for additional options.

You can also install it with [Homebrew](https://brew.sh/) on macOS.

```shell
brew install sshx
```

### CI/CD

You can run sshx in continuous integration workflows to help debug tricky
issues, like in GitHub Actions.

```yaml
name: CI
on: push

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3

      # ... other steps ...

      - run: curl -sSf https://sshx.io/get | sh -s run
      #      ^
      #      └ This will open a remote terminal session and print the URL. It
      #        should take under a second.
```

We don't have a prepackaged action because it's just a single command. It works
anywhere: GitLab CI, CircleCI, Buildkite, CI on your Raspberry Pi, etc.

Be careful adding this to a public GitHub repository, as any user can view the
logs of a CI job while it is running.

## Development

Here's how to work on the project, if you want to contribute.

### Building from source

To build the latest version of the client from source, clone this repository and
run, with [Rust](https://rust-lang.com/) installed:

```shell
cargo install --path crates/sshx
```

This will compile the `sshx` binary and place it in your `~/.cargo/bin` folder.

### Workflow

First, start service containers for development.

```shell
docker compose up -d
```

Install [Rust 1.70+](https://www.rust-lang.org/),
[Node v18](https://nodejs.org/), [NPM v9](https://www.npmjs.com/), and
[mprocs](https://github.com/pvolok/mprocs). Then, run

```shell
npm install
mprocs
```

This will compile and start the server, an instance of the client, and the web
frontend in parallel on your machine.

## Deployment

I host the application servers on [Fly.io](https://fly.io/) and with
[Redis Cloud](https://redis.com/).

Self-hosted deployments are not supported at the moment. If you want to deploy
sshx, you'll need to properly implement HTTP/TCP reverse proxies, gRPC
forwarding, TLS termination, private mesh networking, and graceful shutdown.

Please do not run the development commands in a public setting, as this is
insecure.


## Tailscale MagicDNS Development Setup (with HTTPS)

This configuration allows you to run sshx in development with valid HTTPS certificates via Tailscale.

### Prerequisites

1. Generate Tailscale certificates (only needed once):
```shell
tailscale cert homa-server2.gaur-toad.ts.net
```

This will create:
- `homa-server2.gaur-toad.ts.net.crt` (public certificate)
- `homa-server2.gaur-toad.ts.net.key` (private key)

### Running the Development Environment

**Terminal 1 - Start Redis (if not already running):**
```shell
docker compose up -d
```

**Terminal 2 - Start the backend server:**
```shell
cargo run --bin sshx-server -- \
  --override-origin https://homa-server2.gaur-toad.ts.net:5173 \
  --secret dev-secret \
  --redis-url redis://localhost:12601 \
  --listen 0.0.0.0
```

**Terminal 3 - Start the frontend with HTTPS:**
```shell
npm run dev
```

The Vite dev server will use the Tailscale certificates and serve on:
- **https://homa-server2.gaur-toad.ts.net:5173** (valid HTTPS, accessible via Tailscale)
- **https://localhost:5173** (also works locally)

**Terminal 4 - Connect a client:**
```shell
cargo run --bin sshx -- --server http://homa-server2.gaur-toad.ts.net:8051

## On remote machine
sshx --server http://homa-server2.gaur-toad.ts.net:8051 
```

Or use the installed version:
```shell
sshx --server http://homa-server2.gaur-toad.ts.net:8051
```

### Benefits of this Setup

- ✅ Valid HTTPS certificates (no browser warnings)
- ✅ Web Crypto API (`crypto.subtle`) available for encryption
- ✅ Accessible from any device on your Tailscale network
- ✅ Certificates auto-renewable via Tailscale


## VS Code IDE Integration

sshx can embed a full VS Code editor ([OpenVSCode Server](https://github.com/gitpod-io/openvscode-server)) in the browser canvas, with real-time state sync between all participants.

### Quick start

```shell
sshx --server http://localhost:8051 --ide
```

That's it. On first run, sshx automatically downloads OpenVSCode Server (~73 MB) to `~/.sshx/openvscode-server/` and manages it from there. Subsequent runs reuse the cached binary instantly.

Once connected, click the **IDE** button in the browser toolbar to place a VS Code widget on the canvas.

### How auto-download works

When you pass `--ide`, sshx:

1. Checks `~/.sshx/openvscode-server/.version` — if it matches the pinned version (currently **v1.109.5**), uses the cached binary
2. If missing or version mismatch, downloads the correct tarball from [gitpod-io/openvscode-server releases](https://github.com/gitpod-io/openvscode-server/releases)
3. Extracts to `~/.sshx/openvscode-server.tmp/`, verifies, then atomically renames to `~/.sshx/openvscode-server/`
4. Starts the server and passes `SSHX_SYNC_PORT` to enable the `sshx-collab` extension

Platform is detected automatically:

| OS | Arch | Tarball |
|---|---|---|
| Linux | x86_64 | `linux-x64` |
| Linux | aarch64 | `linux-arm64` |
| macOS | x86_64 (Intel) | `darwin-x64` |
| macOS | aarch64 (Apple Silicon) | `darwin-arm64` |

### Storage layout

```
~/.sshx/
├── openvscode-server/         # Auto-downloaded release (managed by sshx)
│   ├── .version               # "1.109.5" — version marker for upgrade detection
│   ├── bin/openvscode-server  # Shell script launcher
│   ├── node                   # Bundled Node.js binary
│   ├── out/server-main.js     # VS Code server
│   ├── node_modules/
│   └── extensions/            # Built-in VS Code extensions (languages, themes)
└── vscode-data/               # User settings, state (persisted across sessions)
```

When sshx is upgraded and pins a newer OpenVSCode version, the old install is automatically replaced on next `--ide` run.

### Advanced usage

**Override with a custom binary** (skips auto-download):

```shell
sshx --server http://localhost:8051 \
     --openvscode-bin /path/to/openvscode-server/bin/openvscode-server

# Or via env var
export SSHX_OPENVSCODE_BIN=/path/to/openvscode-server/bin/openvscode-server
sshx --server http://localhost:8051
```

**With Tailscale:**

```shell
sshx --server http://homa-server2.gaur-toad.ts.net:8051 --ide
```

### What syncs

The `sshx-collab` extension (in `extensions/sshx-collab/`, auto-loaded by sshx) syncs the following:

| Feature | Direction | Description |
|---|---|---|
| **Project/folder opened** | IDE → Browser | Workspace folder name and path shown in the IDE widget |
| **Open files** | IDE → Browser, IDE ↔ IDE | List of open tabs synced in real time |
| **Active file** | IDE → Browser, IDE ↔ IDE | Currently focused file highlighted for all participants |
| **File scroll position** | IDE → Browser, IDE ↔ IDE | Visible line range synced so collaborators can follow along |
| **Cursor positions** | IDE ↔ IDE | Remote cursors shown as colored decorations in the editor |
| **Selections** | IDE ↔ IDE | Multi-cursor selections visible to all participants |
| **Edit lock** | IDE ↔ Browser ↔ IDE | Mutex-style lock (5s auto-expiry) prevents conflicting edits |

### Architecture

```
VS Code Extension (sshx-collab)
  │  POST /state  (every 100ms, debounced)
  │  POST /lock   (on text change)
  │  GET  /events (SSE stream for remote state)
  ▼
IDE Sync Server (127.0.0.1:$SSHX_SYNC_PORT)
  │  Runs inside the sshx CLI process
  │  Forwards state as gRPC ClientMessage
  ▼
sshx-server
  │  Stores IDE state per-user
  │  Broadcasts IdeStateDiff to WebSocket clients
  │  Relays browser IDE state back to CLI via gRPC
  ▼
Browser (IdeEditorWidget)
  │  Shows collaborator dots, active files, lock status
  │  Sends UpdateIdeState for browser-side IDE changes
```

### Key files

```
sshx/
├── extensions/sshx-collab/              # VS Code extension (auto-loaded)
│   ├── package.json
│   └── src/extension.js                 # State sync, cursors, scroll, locks
├── crates/sshx/src/openvscode.rs        # Auto-download + version management
├── crates/sshx/src/ide.rs               # Sync HTTP server + IDE process manager
├── crates/sshx-server/src/web/protocol.rs  # WsIdeState, WsEditLock types
└── src/lib/ui/IdeEditorWidget.svelte    # Browser-side IDE widget
```

### Troubleshooting

- **Download fails**: Check your internet connection. sshx downloads from `github.com` — if behind a proxy, set `HTTPS_PROXY`. A failed download leaves no broken state (uses a temp directory).
- **Extension not activating**: Open a terminal inside the IDE widget and run `echo $SSHX_SYNC_PORT` — it should print a port number. If empty, the sync server didn't start.
- **No sync visible**: Open browser DevTools and look for `ideStateDiff` in WebSocket frames. If absent, the extension isn't sending state.
- **Lock not working**: Edit locks auto-expire after 5 seconds. The extension requests a lock on every text change; if another user holds it, the request is silently denied.
- **Extension not found**: sshx looks for `extensions/` next to the `sshx` binary first, then in the workspace root. Check logs for `using bundled extensions dir`.
- **Force re-download**: Delete `~/.sshx/openvscode-server/` and run with `--ide` again.
