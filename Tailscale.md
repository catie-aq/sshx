# Running sshx over Tailscale (MagicDNS + HTTPS)

This guide explains how to expose an sshx development instance over your Tailscale tailnet with valid HTTPS certificates, making it accessible from any device on the tailnet with no browser security warnings.

## How it works

Tailscale's MagicDNS gives each machine a stable hostname (`<machine>.<tailnet>.ts.net`) and can issue Let's Encrypt certificates for it. The setup here wires that certificate into both the Vite dev server (frontend) and the sshx backend, so every component speaks HTTPS/TLS within the tailnet.

```
[Any tailnet device]
        |  HTTPS :5173
        v
  Vite dev server  (TLS terminated with Tailscale cert)
        |  HTTP proxy /api → :8051
        v
  sshx-server  (listens on 0.0.0.0:8051, override-origin points to Vite)
        |
        v
  Redis :12601  (localhost only)
```

## Prerequisites

- Tailscale installed and authenticated on the server machine
- HTTPS enabled in Tailscale admin console (Admin → DNS → Enable HTTPS)
- Docker + Docker Compose (for the containerised workflow), **or** Rust + Node.js installed locally (for the manual workflow)

## 1. Generate Tailscale certificates

Run once on the server machine. Certificates are valid for ~90 days and can be renewed the same way.

```shell
tailscale cert <your-machine>.<tailnet>.ts.net
```

This creates two files in the current directory:

| File | Purpose |
|------|---------|
| `<your-machine>.<tailnet>.ts.net.crt` | Public certificate (chain) |
| `<your-machine>.<tailnet>.ts.net.key` | Private key |

> These files are already listed in `.gitignore` — do not commit them.

## 2. Configure the project

### `vite.config.ts`

Point Vite at the certificate files and proxy `/api` to the backend hostname:

```ts
import { readFileSync } from "node:fs";

server: {
  port: 5173,
  strictPort: true,
  https: {
    key: readFileSync("<your-machine>.<tailnet>.ts.net.key"),
    cert: readFileSync("<your-machine>.<tailnet>.ts.net.crt"),
  },
  proxy: {
    "/api": {
      target: "http://<your-machine>.<tailnet>.ts.net:8051",
      changeOrigin: true,
      ws: true,
      secure: false,   // backend is plain HTTP
    },
  },
},
```

### `package.json`

Bind Vite to all interfaces so Tailscale can reach it:

```json
"dev": "vite dev --host 0.0.0.0"
```

## 3a. Run with Docker Compose (recommended)

`compose.yaml` defines three services: `redis`, `backend`, and `web`. All use `network_mode: host` so they can reach each other on localhost and are reachable from the tailnet on their respective ports.

```shell
# Build and start everything
UID=$(id -u) GID=$(id -g) docker compose up -d

# Follow logs
docker compose logs -f

# Stop
docker compose down
```

The backend service mounts the certificate files read-only at `/certs/` inside the container (even though the backend itself currently serves plain HTTP — the mount is there for future TLS offload).

Key backend flags set in `compose.yaml`:

| Flag | Value | Purpose |
|------|-------|---------|
| `--override-origin` | `https://<machine>.<tailnet>.ts.net:5173` | Tells clients which URL to open |
| `--listen` | `0.0.0.0` | Accept connections from the tailnet |
| `--redis-url` | `redis://localhost:12601` | Local Redis |

## 3b. Run manually (mprocs or separate terminals)

`mprocs.yaml` orchestrates the same processes without Docker. Start it with:

```shell
mprocs
```

Or run each process in a separate terminal:

**Terminal 1 — Redis:**
```shell
docker compose up -d redis
```

**Terminal 2 — Backend:**
```shell
cargo run --bin sshx-server -- \
  --override-origin https://<your-machine>.<tailnet>.ts.net:5173 \
  --secret dev-secret \
  --redis-url redis://localhost:12601 \
  --listen 0.0.0.0
```

**Terminal 3 — Frontend:**
```shell
npm run dev
```

The `client` process in `mprocs.yaml` has `autostart: false`; launch it manually when needed:

```shell
cargo run --bin sshx -- --server http://<your-machine>.<tailnet>.ts.net:8051
```

## 4. Connect from another tailnet device

Once the stack is running, open a session from any device on the tailnet:

```shell
# Installed binary
sshx --server http://<your-machine>.<tailnet>.ts.net:8051

# Or during development (cargo)
cargo run --bin sshx -- --server http://<your-machine>.<tailnet>.ts.net:8051
```

The terminal link printed by `sshx` will point to `https://<your-machine>.<tailnet>.ts.net:5173`, which opens directly in any browser on the tailnet with a valid certificate.

## Ports at a glance

| Port | Service | Binding |
|------|---------|---------|
| 5173 | Vite / frontend (HTTPS) | 0.0.0.0 (all interfaces) |
| 8051 | sshx-server (HTTP) | 0.0.0.0 (all interfaces) |
| 12601 | Redis | 127.0.0.1 (localhost only) |

## Renewing certificates

```shell
tailscale cert <your-machine>.<tailnet>.ts.net
```

Then restart the frontend and backend so they pick up the new files.
