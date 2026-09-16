# sshx — CATIE fork

A secure web-based, collaborative terminal — extended into a collaborative
workspace for AI-assisted development.

![](https://i.imgur.com/Q3qKAHW.png)

This is the [CATIE](https://www.catie.fr) fork of
[ekzhang/sshx](https://github.com/ekzhang/sshx) (based on v0.4.1). Upstream
features are all still there:

- Run a single command to share your terminal with anyone.
- Resize, move windows, and freely zoom and pan on an infinite canvas.
- See other people's cursors moving in real time.
- End-to-end encryption with Argon2 and AES.
- Automatic reconnection and real-time latency estimates.
- Predictive echo for faster local editing (à la Mosh).

## Why this fork: AI tools with zero setup

Upstream sshx solves one problem very well: sharing a terminal **without any
configuration** — no SSH keys, no open ports, no VPN, no account. One command
prints a URL; whoever opens it gets the terminal, end-to-end encrypted.

This fork carries that property to a new use case: **giving people access to AI
coding tools such as Claude Code when they don't know how to set up SSH or
install a codebase.**

1. An operator (expert, trainer, support team) prepares a machine: repository
   cloned, dependencies installed, Claude Code authenticated, `sshx` running.
2. The end user receives **a single URL** — nothing to install, no SSH or API
   keys to manage, no environment to build.
3. In the browser they get the whole workshop: the terminal to talk to Claude
   Code, a **Claude activity feed** to follow what the agent does without
   reading raw terminal output, **FileCards** to see the files it touches
   without knowing git, an embedded **VS Code** to open the code, and the canvas
   plus **screen sharing** for guidance — usable from a phone.
4. A helper co-pilots live (cursors, chat, annotations), and the **admin API**
   lets one shared server host many users.

Every feature thus plays two roles: a collaboration tool between developers,
and a mediation layer that makes a coding agent usable by non-specialists.
End-to-end encryption keeps the code private even on a shared server, whose
administrator only sees metadata.

## What the fork adds

| Feature | Status | Usage |
|---|---|---|
| **Claude Code integration** — transcripts are discovered automatically and streamed live: activity feed, active instances, execution graph, session timeline. A 200-event server buffer replays history on reconnect; sshx can start before Claude. | working | run `sshx` where Claude works (`--no-claude-tracking` to opt out) |
| **Workspace analysis & canvas widgets** — FileCards (metadata, import graph, image previews), searchable file tree, library cards, Markdown sticky notes, Cmd+K palette. Positions and images persist in the workspace. | working | `sshx analyze`, then `sshx` (`--workspace`, `--no-workspace`) |
| **Embedded VS Code** — OpenVSCode Server auto-downloaded and tunneled through the existing sshx connection (no extra port), with the `sshx-collab` extension syncing files, cursors and edit locks. | working | `sshx --ide` — see [below](#vs-code-ide-integration) |
| **P2P screen sharing** — WebRTC between participants; the server only relays signaling, never media. | working | "Share your screen" toolbar button |
| **Offscreen browser** — `sshx-browser` runs Chromium in Xvfb, streams VP8 and accepts remote keyboard/mouse. Frame ingestion works; relaying to viewers is not done yet. | partial | `sshx --with-browser <url>` (needs xvfb, chromium, ffmpeg) |
| **React / Next.js observability** — injected dev-mode scripts show the component and source file under the cursor; "Show in SSHX" opens its FileCard on the canvas. | working | see [doc/observability](doc/observability) |
| **Rich text, drawing, slideshow** — TipTap text blocks, free images, pen/highlighter layer with undo/redo, canvas regions presented as slides. | in progress | context menu, toolbar |
| **Mobile support** — reworked touch gestures, toolbar and terminals for small screens. | working | open the URL on a phone |
| **Session persistence** — widgets and notes survive restarts. | working | automatic |
| **Custom session slugs** — readable URLs (`/s/<slug>`). | working | `--slug <name>` |
| **Administration & sharing** — `/api/sessions` behind an admin token (constant-time check) and a `/sessions` page listing sessions, users and shells; **Share** button copying the session URL. | working | `sshx-server --admin-token <secret>` (or `SSHX_ADMIN_TOKEN`) |
| **Self-update** — `sshx update` installs the latest client published by the server it points to; a background check runs at most hourly. | working | `sshx update [--check] [--force]`, opt out with `--no-auto-update` |

Detailed write-ups (in French) of each feature — description, implications,
usage — live in [doc/livrables/](doc/livrables/00-synthese.md); architecture
docs are in [doc/](doc) and [CLAUDE.md](CLAUDE.md).

### Getting the fork's client

The fork's server publishes its own client builds. Download the binary from the
server's landing page (it shows the latest version), then keep it current with:

```shell
sshx --server https://<your-server> update
```

Operators publish a new build with `scripts/publish-release.sh`, which bumps the
workspace version, builds (Linux via `cargo zigbuild` against glibc 2.28 rather
than musl, so Tailscale MagicDNS names resolve), and writes
`releases/manifest.json`, served by the server under `/releases`. For a tailnet
deployment with HTTPS, see [Tailscale.md](Tailscale.md); packaging scripts for
Linux, macOS and Arch are in [distribution/](distribution).

---

The sections below document the upstream project; the `sshx.io` install
commands give you the upstream client, without the fork's features.

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

Upstream hosts the application servers on [Fly.io](https://fly.io/) and with
[Redis Cloud](https://redis.com/).

Upstream does not support self-hosted deployments. The CATIE fork is
self-hosted on a Tailscale tailnet (see [Tailscale.md](Tailscale.md)); in the
general case you'll need to properly implement HTTP/TCP reverse proxies, gRPC
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
