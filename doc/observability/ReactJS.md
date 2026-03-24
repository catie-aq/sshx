# React App Observability via SSHX

Make any React app (Vite, Create React App, Parcel, etc.) observable without modifying component code — hover any element to see the component name and file path, then click "Show in SSHX" to surface that source file as a FileCard in your running SSHX session.

---

## Overview

React DevTools shows you component trees in a devtools panel. This approach is different: it surfaces component identity *in the page itself*, on demand, and bridges it into a collaborative SSHX session so teammates can see what you're looking at.

**End result:**
- A small terminal icon appears in the bottom-left corner of the page
- Click the icon to open the connection panel, paste your SSHX session URL
- Press `Alt+I` to toggle the component inspector
- Hover any element → a badge appears: component name, file path, line number
- The element gets a purple outline
- Click "Show in SSHX" → a FileCard opens in the SSHX session
- (Bidirectional, optional) From the SSHX session, trigger a highlight → matching elements flash in the React app

This works without modifying any component files.

---

## Part 1 — Component Inspection (Dev Mode Only)

React attaches internal fiber data to DOM nodes in development builds. Every DOM node rendered by React has a property with a key matching `__reactFiber$...` (the suffix is a random ID stable per page load). Walking up the fiber tree from that node gives you:

- `fiber.type` — the component function/class (its `.name` property is the component name)
- `fiber._debugSource` — `{ fileName, lineNumber, columnNumber }` — set by React 18 development builds
- `fiber._debugStack` — an Error object whose stack trace contains file info — used by React 19

No build configuration needed. The overlay script reads these at hover time.

```js
function getFiberFromDom(domNode) {
  const key = Object.keys(domNode).find(k => k.startsWith('__reactFiber$'));
  return key ? domNode[key] : null;
}

function findComponentFiber(domNode) {
  let fiber = getFiberFromDom(domNode);
  while (fiber) {
    if (typeof fiber.type === 'function' && fiber.type.name) {
      return fiber;
    }
    fiber = fiber.return;
  }
  return null;
}

function getComponentInfo(domNode) {
  const fiber = findComponentFiber(domNode);
  if (!fiber) return null;

  let file = null, line = null;

  // React 18: _debugSource has fileName/lineNumber directly.
  if (fiber._debugSource) {
    file = fiber._debugSource.fileName;
    line = fiber._debugSource.lineNumber;
  }

  // React 19: _debugStack is an Error whose stack trace contains file info.
  if (!file && fiber._debugStack) {
    const parsed = parseSourceFromStack(fiber._debugStack);
    if (parsed) { file = parsed.file; line = parsed.line; }
  }

  return {
    name: fiber.type.name || fiber.type.displayName || '(anonymous)',
    file, line,
  };
}
```

**Requirements:**
- Must run in development mode — `_debugSource`/`_debugStack` are only set in development builds
- React's fiber internals are not a public API and may change across major versions
- Works reliably with React 16–19
- Vite, CRA, and Parcel all set development mode when running `npm run dev`

---

## Part 2 — Overlay Script

The overlay is `sshx-overlay.js` — a self-contained vanilla JS file that requires no bundler. It provides:

- **Connection panel** (bottom-left floating icon) — paste your SSHX session URL and connect
- **Component inspector** — toggle with `Alt+I`, hover elements to see React component info
- **"Show in SSHX" button** — sends the file path to the SSHX session to open a FileCard
- **Bidirectional highlight** — listens for `HighlightComponent` messages via BroadcastChannel

### File path handling for different bundlers

The overlay's `cleanFilePath()` function strips bundler-specific prefixes:

| Bundler | Dev-mode URL format | Cleaned path |
|---------|-------------------|-------------|
| **Vite** | `http://localhost:3001/src/App.tsx?t=123` | `/src/App.tsx` |
| **Webpack (CRA)** | `webpack-internal:///./src/App.tsx` | `src/App.tsx` |
| **Next.js RSC** | `rsc://React/Server/webpack-internal:///(rsc)/./app/page.tsx` | `app/page.tsx` |

All formats are handled transparently.

---

## Part 3 — WebSocket Connection

The connector is `sshx-connect.js` — a framework-agnostic vanilla JS file that handles:

- URL parsing (session ID from path, encryption key from `#` fragment)
- Key derivation via Argon2id (lazy-loads `argon2-browser` from CDN)
- AES-128-CTR encrypted zeros authentication proof
- Minimal CBOR encoder/decoder (zero dependencies)
- WebSocket connection with auto-reconnect (exponential backoff 2s–30s)
- `localStorage` persistence of the session URL

It exposes:
```js
window.__sshxConnect(url)          // Connect to an SSHX session
window.__sshxDisconnect()          // Close connection
window.__sshxOpenFileCard(x,y,path) // Open a FileCard in the session
window.__sshxStatus                // 'disconnected' | 'connecting' | 'connected'
window.__sshxOnStatus(fn)          // Register status change callback
```

---

## Part 4 — Integration with Vite + React

### Option A: Load from the SSHX server (recommended)

No files to copy — the SSHX server serves the scripts directly. Add to `index.html`:

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
# From the sshx repo:
cp observability/nextjs/sshx-overlay.js /path/to/your-react-app/public/
cp observability/nextjs/sshx-connect.js /path/to/your-react-app/public/
```

Then add to `index.html`:

```html
<body>
  <div id="root"></div>
  <script type="module" src="/src/main.tsx"></script>
  <!-- SSHX Observability Overlay -->
  <script src="/sshx-overlay.js" defer></script>
  <script src="/sshx-connect.js" defer></script>
</body>
```

Vite serves files from `public/` as static assets at the root path.

### Option C: Conditional loading (dev-only)

If you want the overlay only in development, use a Vite plugin or a conditional in your entry file:

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

Or use an environment variable:

```tsx
// src/main.tsx
const enableOverlay = import.meta.env.VITE_SSHX_OVERLAY === '1' || import.meta.env.DEV;
if (enableOverlay) {
  // ... load scripts as above
}
```

---

## Part 5 — Integration with Create React App

CRA uses `public/index.html` as the template. Add the scripts there:

```html
<!-- public/index.html -->
<body>
  <div id="root"></div>
  <script src="%PUBLIC_URL%/sshx-overlay.js" defer></script>
  <script src="%PUBLIC_URL%/sshx-connect.js" defer></script>
</body>
```

Copy the scripts to `public/`:
```bash
cp sshx-overlay.js sshx-connect.js /path/to/your-cra-app/public/
```

For dev-only loading, wrap in a conditional check using `REACT_APP_*` env var:
```bash
# .env.development
REACT_APP_SSHX_OVERLAY=1
```

---

## Part 6 — SSHX Protocol Messages

These messages enable the bidirectional bridge:

| Direction | Message | Purpose |
|-----------|---------|---------|
| Client → Server | `openFileCard: [x, y, path]` | Open a FileCard on the canvas |
| Client → Server | `highlightComponent: name` | Broadcast highlight to all overlays |
| Server → Client | `highlightComponent: name` | Flash matching components in the overlay |

These are already implemented in the SSHX server.

---

## Workspace Analysis

Before starting a session, run `sshx analyze` on your React project to index all source files. This populates the FileTree and FileCard widgets in the SSHX canvas with component metadata.

```bash
# From the sshx repo (or wherever the sshx binary is)
sshx analyze --workspace /path/to/your-react-app
```

This scans all source files (TSX, TS, JS, CSS, JSON, etc.), extracts import/export graphs, line counts, and timestamps, then writes `.sshx-analysis.json` to the workspace root.

Example output:
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

Then start sshx with the `--workspace` flag pointing to the same directory:

```bash
sshx --server http://your-sshx-server:8052 --workspace /path/to/your-react-app
```

The analysis file is read at session start and streamed to all connected browsers. Re-running `sshx analyze` while a session is live pushes updates to all browsers automatically.

**Options:**

| Flag | Default | Description |
|------|---------|-------------|
| `--workspace PATH` | Current directory | Root directory to scan |
| `--output FILE` | `<workspace>/.sshx-analysis.json` | Output file path |

AI descriptions are left empty by default — they can be filled in from the browser UI's Workspace panel.

---

## Step-by-Step MVP (Vite + React)

### Prerequisites

- SSHX server running and accessible
- React app using Vite (e.g. created with `npm create vite@latest`)
- A running SSHX session with its URL + key fragment

### Step 0: Analyze the workspace (optional but recommended)

```bash
sshx analyze --workspace /path/to/your-react-app
```

### Step 1: Copy the overlay files

```bash
mkdir -p public/
cp /path/to/sshx/observability/nextjs/sshx-overlay.js public/
cp /path/to/sshx/observability/nextjs/sshx-connect.js public/
```

### Step 2: Add script tags to `index.html`

```html
<body>
  <div id="root"></div>
  <script type="module" src="/src/main.tsx"></script>
  <script src="/sshx-overlay.js" defer></script>
  <script src="/sshx-connect.js" defer></script>
</body>
```

### Step 3: Start the dev server

```bash
npm run dev
```

Open the app in your browser. Open the browser console and verify:

```
[SSHX overlay] loaded — press Alt+I to toggle inspector
[SSHX connect] loaded
```

### Step 4: Connect to SSHX

1. Click the terminal icon (bottom-left corner)
2. Paste your SSHX session URL (e.g. `https://host/s/SESSION#KEY`)
3. Click **Connect**
4. Verify the console shows:
   ```
   [SSHX connect] WebSocket open
   [SSHX connect] authenticated
   ```

### Step 5: Inspect components

1. Press `Alt+I` to activate the inspector
2. Hover any React component — a badge shows the component name and file path
3. Click "Show in SSHX" to open a FileCard in the SSHX session

---

## Differences from Next.js Integration

| Aspect | Next.js | Vite / CRA / React |
|--------|---------|-------------------|
| Script loading | `next/script` with `strategy="afterInteractive"` | `<script defer>` in `index.html` |
| Dev-only gating | `process.env.NODE_ENV` | `import.meta.env.DEV` (Vite) or `REACT_APP_*` (CRA) |
| File paths in stack traces | `webpack-internal:///` or `rsc://` prefixes | `http://host:port/src/...` (Vite) or `webpack-internal:///` (CRA) |
| Server-side rendering | Yes (App Router / RSC) | No — client-only |
| Public dir | `public/` (same) | `public/` (same for Vite/CRA) |
| Env var prefix | `NEXT_PUBLIC_*` | `VITE_*` (Vite) or `REACT_APP_*` (CRA) |

The overlay scripts (`sshx-overlay.js` and `sshx-connect.js`) are identical for both — they handle all bundler URL formats automatically.

---

## Troubleshooting

**Badge shows "no source"**

- Make sure you're running in dev mode (`npm run dev`). Production builds strip `_debugSource`.
- Verify React fiber keys exist: in the browser console, run `Object.keys(document.querySelector('#root > *'))` and look for a key starting with `__reactFiber$`.
- React 19 uses `_debugStack` instead of `_debugSource` — the overlay handles both.

**WebSocket connection fails**

- Check the session URL includes the `#key` fragment (copy directly from SSHX terminal output).
- The key is the URL fragment — it appears after `#` in the SSHX link.
- Check CORS if loading scripts cross-origin from the SSHX server.

**argon2-browser fails to load**

- The CDN URL requires internet access. For air-gapped environments, install `argon2-browser` locally and serve from `public/`.

**"Show in SSHX" does nothing**

- Confirm you are connected (green dot in the connection panel).
- Check the SSHX server logs for unexpected CBOR decode errors.
- The `OpenFileCard` message must be implemented in the SSHX server.

**Scripts not loading in Vite**

- Vite serves `public/` contents at the root. Verify the files exist at `public/sshx-overlay.js`.
- Check the browser Network tab — the scripts should load as separate requests.

**Overlay appears in production**

- Remove the `<script>` tags from `index.html`, or use the conditional loading approach from Part 4 Option C.
