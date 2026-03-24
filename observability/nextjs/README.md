# SSHX Observability — Next.js Integration

Inspect React components in the browser and bridge them into a running SSHX session. Hover any element to see its component name and source file, then click "Show in SSHX" to open a FileCard on the collaborative canvas.

## Requirements

- **React dev mode** (`npm run dev`) — the overlay reads `_debugSource` from React fiber internals, which are only present in development builds.
- A running SSHX session.

## Quick Start

### Option A: Load scripts from the sshx server (recommended)

No files to copy — the sshx server serves the scripts directly.

Set the env var in your Next.js app's `.env.local`:

```
NEXT_PUBLIC_SSHX_SERVER=https://your-sshx-server:8051
```

Then in `app/layout.tsx`:

```tsx
import Script from 'next/script';

const sshxServer = process.env.NEXT_PUBLIC_SSHX_SERVER || '';
const overlayEnv = process.env.NEXT_PUBLIC_SSHX_OVERLAY;
const isDev = process.env.NODE_ENV === 'development';
const showOverlay = overlayEnv === '0' ? false : (overlayEnv ? true : isDev);

// In <head> or alongside other Script tags:
{showOverlay && sshxServer && (
  <>
    <Script src={`${sshxServer}/sshx-overlay.js`} strategy="afterInteractive" />
    <Script src={`${sshxServer}/sshx-connect.js`} strategy="afterInteractive" />
  </>
)}
```

### Option B: Copy scripts to `public/`

```bash
cp sshx-overlay.js sshx-connect.js /path/to/your-nextjs-app/public/
```

Or create symlinks:

```bash
ln -sf /path/to/sshx/observability/nextjs/sshx-overlay.js public/sshx-overlay.js
ln -sf /path/to/sshx/observability/nextjs/sshx-connect.js public/sshx-connect.js
```

Then use relative paths in `app/layout.tsx`:

```tsx
{showOverlay && (
  <>
    <Script src="/sshx-overlay.js" strategy="afterInteractive" />
    <Script src="/sshx-connect.js" strategy="afterInteractive" />
  </>
)}
```

### 3. Run and use

```bash
npm run dev
```

The overlay loads automatically in dev mode. A small terminal icon appears in the bottom-left corner of the page.

1. Click the icon to open the connection panel
2. Paste your SSHX session URL (e.g. `https://host/s/SESSION#KEY`)
3. Click **Connect**
4. Press **Alt+I** (or click **Inspect**) to activate the component inspector
5. Hover React components to see name/file/line, click **Show in SSHX** to open a FileCard

The URL is saved in `localStorage` and auto-reconnects on page reload.

## Environment Variable

| Variable | Default | Effect |
|----------|---------|--------|
| `NEXT_PUBLIC_SSHX_SERVER` | *(unset)* | Origin of the sshx server (e.g. `https://host:8051`) |
| `NEXT_PUBLIC_SSHX_OVERLAY` | *(unset)* | Enabled in dev, disabled in production |
| `NEXT_PUBLIC_SSHX_OVERLAY=0` | — | Force disable (even in dev) |
| `NEXT_PUBLIC_SSHX_OVERLAY=1` | — | Force enable (even in production) |

No env var is needed for the SSHX session URL — users paste it directly in the UI.

## Workspace Analysis

Run `sshx analyze` on your Next.js project before starting a session. This indexes all source files and populates the FileTree and FileCard widgets in the SSHX canvas.

```bash
sshx analyze --workspace /path/to/your-nextjs-app
```

Then start the session with:
```bash
sshx --server http://your-sshx-server:8052 --workspace /path/to/your-nextjs-app
```

| Flag | Default | Description |
|------|---------|-------------|
| `--workspace PATH` | Current directory | Root directory to scan |
| `--output FILE` | `<workspace>/.sshx-analysis.json` | Output file path |

Re-running `sshx analyze` while a session is live pushes updates to all connected browsers. AI descriptions can be added from the browser UI's Workspace panel.

## How It Works

### sshx-overlay.js (React-specific)

- Floating connection panel (bottom-left) with URL input and status indicator
- React fiber walking (`__reactFiber$`) for component names and `_debugSource` file paths
- Inspector badge on hover with component name, file, and line number
- Purple outline on inspected elements
- "Show in SSHX" button calls `window.__sshxOpenFileCard(x, y, path)`
- Listens on `BroadcastChannel("sshx-overlay")` for `HighlightComponent` messages

### sshx-connect.js (framework-agnostic)

- Exposes `window.__sshxConnect(url)` / `window.__sshxDisconnect()` API
- Derives AES-128-CTR key via Argon2id (lazy-loads argon2-browser from CDN)
- Authenticates over WebSocket using encrypted zeros proof
- Minimal built-in CBOR encoder/decoder (zero external dependencies)
- Auto-reconnects with exponential backoff (2s–30s)
- Saves URL to `localStorage`, auto-connects on next page load
- Status tracking via `window.__sshxStatus` and `window.__sshxOnStatus(fn)`

## SSHX Protocol Messages Used

| Direction | Message | Purpose |
|-----------|---------|---------|
| Client -> Server | `openFileCard: [x, y, path]` | Open a FileCard on the canvas |
| Client -> Server | `highlightComponent: name` | Broadcast highlight to all overlays |
| Server -> Client | `highlightComponent: name` | Flash matching components in the overlay |

These messages are already implemented in the SSHX server.

## Troubleshooting

**Badge shows "no source"** — You must be in dev mode (`npm run dev`). Production builds strip `_debugSource`.

**WebSocket connection fails** — Verify the URL includes the `#key` fragment. Check the browser console for specific errors.

**argon2-browser fails to load** — The script loads it from jsDelivr CDN. For air-gapped environments, install `argon2-browser` locally and serve from `public/`.

**Overlay not appearing** — Check `NEXT_PUBLIC_SSHX_OVERLAY` is not set to `0`. In production builds, you must explicitly set it to `1`.
