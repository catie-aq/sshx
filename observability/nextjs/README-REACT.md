# SSHX Observability — React (Vite / CRA) Integration

Inspect React components in the browser and bridge them into a running SSHX session. Hover any element to see its component name and source file, then click "Show in SSHX" to open a FileCard on the collaborative canvas.

## Requirements

- **React dev mode** (`npm run dev`) — the overlay reads `_debugSource` (React 18) or `_debugStack` (React 19) from React fiber internals, which are only present in development builds.
- A running SSHX session.

## Quick Start

### Option A: Load scripts from the SSHX server (recommended)

No files to copy — the SSHX server serves the scripts directly.

Add to your `index.html` (Vite) or `public/index.html` (CRA):

```html
<body>
  <div id="root"></div>
  <script type="module" src="/src/main.tsx"></script>
  <!-- SSHX Observability Overlay -->
  <script src="https://your-sshx-server:8051/sshx-overlay.js" defer></script>
  <script src="https://your-sshx-server:8051/sshx-connect.js" defer></script>
</body>
```

### Option B: Copy scripts to `public/`

```bash
cp sshx-overlay.js sshx-connect.js /path/to/your-react-app/public/
```

Or create symlinks:

```bash
ln -sf /path/to/sshx/observability/nextjs/sshx-overlay.js public/sshx-overlay.js
ln -sf /path/to/sshx/observability/nextjs/sshx-connect.js public/sshx-connect.js
```

Then add to `index.html`:

```html
<!-- Vite: index.html at project root -->
<script src="/sshx-overlay.js" defer></script>
<script src="/sshx-connect.js" defer></script>

<!-- CRA: public/index.html -->
<script src="%PUBLIC_URL%/sshx-overlay.js" defer></script>
<script src="%PUBLIC_URL%/sshx-connect.js" defer></script>
```

### Option C: Dev-only conditional loading (Vite)

Load the scripts programmatically so they never appear in production:

```tsx
// src/main.tsx
if (import.meta.env.DEV) {
  const overlay = document.createElement('script');
  overlay.src = '/sshx-overlay.js';
  overlay.defer = true;
  document.body.appendChild(overlay);

  const connect = document.createElement('script');
  connect.src = '/sshx-connect.js';
  connect.defer = true;
  document.body.appendChild(connect);
}
```

### 3. Run and use

```bash
npm run dev
```

The overlay loads automatically. A small terminal icon appears in the bottom-left corner of the page.

1. Click the icon to open the connection panel
2. Paste your SSHX session URL (e.g. `https://host/s/SESSION#KEY`)
3. Click **Connect**
4. Press **Alt+I** (or click **Inspect**) to activate the component inspector
5. Hover React components to see name/file/line, click **Show in SSHX** to open a FileCard

The URL is saved in `localStorage` and auto-reconnects on page reload.

## Environment Variables

### Vite

| Variable | Default | Effect |
|----------|---------|--------|
| `VITE_SSHX_OVERLAY` | *(unset)* | When set to `1`, forces overlay on in production |

Use `import.meta.env.DEV` for automatic dev-only behavior (no env var needed).

### Create React App

| Variable | Default | Effect |
|----------|---------|--------|
| `REACT_APP_SSHX_OVERLAY` | *(unset)* | When set to `1`, forces overlay on |

No env var is needed for the SSHX session URL — users paste it directly in the UI.

## Workspace Analysis

Run `sshx analyze` on your React project before starting a session. This indexes all source files and populates the FileTree and FileCard widgets in the SSHX canvas.

```bash
sshx analyze --workspace /path/to/your-react-app
```

Output example:
```
  sshx analyze  Scanning /home/user/my-react-app…

  ✓  67 files indexed
      component    56
      config       5
      hook         1
      utility      5

  ℹ  descriptions: 0/67

  →  /home/user/my-react-app/.sshx-analysis.json
```

Then start the session with:
```bash
sshx --server http://your-sshx-server:8052 --workspace /path/to/your-react-app
```

| Flag | Default | Description |
|------|---------|-------------|
| `--workspace PATH` | Current directory | Root directory to scan |
| `--output FILE` | `<workspace>/.sshx-analysis.json` | Output file path |

Re-running `sshx analyze` while a session is live pushes updates to all connected browsers. AI descriptions can be added from the browser UI's Workspace panel.

## How It Works

### sshx-overlay.js (React-specific)

- Floating connection panel (bottom-left) with URL input and status indicator
- React fiber walking (`__reactFiber$`) for component names and `_debugSource` / `_debugStack` file paths
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

## File Path Handling

The overlay automatically cleans file paths from different bundlers:

| Bundler | Dev-mode URL format | Cleaned path |
|---------|-------------------|-------------|
| **Vite** | `http://localhost:3001/src/App.tsx?t=123` | `/src/App.tsx` |
| **Webpack (CRA)** | `webpack-internal:///./src/App.tsx` | `src/App.tsx` |
| **Next.js** | `rsc://React/Server/webpack-internal:///(rsc)/./app/page.tsx` | `app/page.tsx` |

## SSHX Protocol Messages Used

| Direction | Message | Purpose |
|-----------|---------|---------|
| Client -> Server | `openFileCard: [x, y, path]` | Open a FileCard on the canvas |
| Client -> Server | `highlightComponent: name` | Broadcast highlight to all overlays |
| Server -> Client | `highlightComponent: name` | Flash matching components in the overlay |

These messages are already implemented in the SSHX server.

## Differences from Next.js Setup

| Aspect | Next.js | Vite / CRA |
|--------|---------|-----------|
| Script loading | `next/script` with `strategy` | `<script defer>` in `index.html` |
| Dev gating | `process.env.NODE_ENV` | `import.meta.env.DEV` or `REACT_APP_*` |
| HTML template | `app/layout.tsx` | `index.html` (root) or `public/index.html` |
| SSR | Yes | No |

The JS scripts (`sshx-overlay.js`, `sshx-connect.js`) are **identical** — no framework-specific changes needed.

## Troubleshooting

**Badge shows "no source"** — You must be in dev mode (`npm run dev`). Production builds strip `_debugSource`. React 19 uses `_debugStack` instead — the overlay handles both.

**WebSocket connection fails** — Verify the URL includes the `#key` fragment. Check the browser console for specific errors.

**argon2-browser fails to load** — The script loads it from jsDelivr CDN. For air-gapped environments, install `argon2-browser` locally and serve from `public/`.

**Overlay not appearing** — Ensure the `<script>` tags are in `index.html` and the files exist in `public/`. Check the browser Network tab.

**File paths show full URLs** — This is expected for Vite dev mode. The `cleanFilePath` function strips the `http://host:port` prefix, leaving the project-relative path.
