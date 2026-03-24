# Next.js App Observability via SSHX

Make any Next.js app observable without modifying component code — hover any element to see the component name and file path, then click "Show in SSHX" to surface that source file as a FileCard in your running SSHX session.

---

## Overview

React DevTools shows you component trees in a devtools panel. This approach is different: it surfaces component identity *in the page itself*, on demand, and bridges it into a collaborative SSHX session so teammates can see what you're looking at.

**End result:**
- Press `Alt+I` to toggle the overlay
- Hover any element → a badge appears: component name, file path, line number
- The element gets a purple outline
- Click "Show in SSHX" → a FileCard opens in the SSHX session, positioned near where you clicked
- (Bidirectional, optional) From the SSHX session, trigger a highlight → matching elements flash in the Next.js app

This works without modifying any component files.

---

## Part 1 — Component Inspection (Dev Mode Only)

React attaches internal fiber data to DOM nodes in development builds. Every DOM node rendered by React has a property with a key matching `__reactFiber$...` (the suffix is a random ID stable per page load). Walking up the fiber tree from that node gives you:

- `fiber.type` — the component function/class (its `.name` property is the component name)
- `fiber._debugSource` — `{ fileName, lineNumber, columnNumber }` — set by React's development build

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
  return {
    name: fiber.type.name,
    file: fiber._debugSource?.fileName ?? null,
    line: fiber._debugSource?.lineNumber ?? null,
  };
}
```

**Requirements:**
- Must run `npm run dev` — `_debugSource` is only set in development builds (`NODE_ENV=development`)
- React's fiber internals are not a public API and may change across major versions
- Works reliably with React 16–19

---

## Part 2 — Overlay Script

Create `public/sshx-overlay.js`. It is a self-contained vanilla JS file — no bundler needed.

```js
// public/sshx-overlay.js
// SSHX component inspector overlay. Toggle with Alt+I.
// Set window.__SSHX_URL before loading, or export NEXT_PUBLIC_SSHX_URL.
(function () {
  'use strict';

  // ── State ──────────────────────────────────────────────────────────────────
  let active = false;
  let currentTarget = null;
  let badge = null;
  let outline = null;

  // ── Fiber walker ──────────────────────────────────────────────────────────
  function getFiberFromDom(node) {
    const key = Object.keys(node).find(k => k.startsWith('__reactFiber$'));
    return key ? node[key] : null;
  }

  function findComponentFiber(node) {
    let fiber = getFiberFromDom(node);
    while (fiber) {
      if (typeof fiber.type === 'function' && fiber.type.name) return fiber;
      fiber = fiber.return;
    }
    return null;
  }

  function getComponentInfo(node) {
    const fiber = findComponentFiber(node);
    if (!fiber) return null;
    return {
      name: fiber.type.name,
      file: fiber._debugSource?.fileName ?? null,
      line: fiber._debugSource?.lineNumber ?? null,
    };
  }

  // ── Badge & outline ────────────────────────────────────────────────────────
  function createBadge() {
    const el = document.createElement('div');
    el.id = '__sshx_badge';
    Object.assign(el.style, {
      position: 'fixed',
      zIndex: '999999',
      background: '#18181b',
      border: '1px solid #52525b',
      borderRadius: '6px',
      padding: '5px 10px',
      fontSize: '12px',
      fontFamily: '"Fira Code VF", "Fira Code", monospace',
      color: '#d4d4d8',
      pointerEvents: 'auto',
      boxShadow: '0 2px 8px rgba(0,0,0,0.5)',
      display: 'none',
      maxWidth: '420px',
      lineHeight: '1.6',
    });
    document.body.appendChild(el);
    return el;
  }

  function showBadge(x, y, info) {
    if (!badge) badge = createBadge();
    const filePart = info.file
      ? `<span style="color:#818cf8">${info.file}${info.line ? ':' + info.line : ''}</span>`
      : '<span style="color:#71717a">no source</span>';
    badge.innerHTML = `
      <div><b style="color:#a5b4fc">${info.name}</b></div>
      <div style="font-size:11px;margin-top:2px">${filePart}</div>
      <button id="__sshx_open_btn" style="
        margin-top:6px;background:#3730a3;color:#e0e7ff;border:none;
        border-radius:4px;padding:3px 8px;font-size:11px;cursor:pointer;
        font-family:inherit
      ">Show in SSHX</button>
    `;
    badge.style.display = 'block';
    // Position near cursor, keep in viewport
    const vw = window.innerWidth, vh = window.innerHeight;
    const bw = 240, bh = 90;
    badge.style.left = Math.min(x + 14, vw - bw - 8) + 'px';
    badge.style.top = Math.min(y + 14, vh - bh - 8) + 'px';

    document.getElementById('__sshx_open_btn').onclick = (e) => {
      e.stopPropagation();
      if (info.file) openFileCardInSshx(x, y, info.file);
    };
  }

  function hideBadge() {
    if (badge) badge.style.display = 'none';
  }

  function highlightElement(el) {
    if (!el) return;
    el.style.outline = '2px solid #818cf8';
    el.style.outlineOffset = '1px';
  }

  function unhighlightElement(el) {
    if (!el) return;
    el.style.outline = '';
    el.style.outlineOffset = '';
  }

  // ── Event handlers ─────────────────────────────────────────────────────────
  function onMouseMove(e) {
    const target = e.target;
    if (target === badge || badge?.contains(target)) return;
    if (target === currentTarget) return;

    unhighlightElement(currentTarget);
    currentTarget = target;

    const info = getComponentInfo(target);
    if (info) {
      highlightElement(target);
      showBadge(e.clientX, e.clientY, info);
    } else {
      hideBadge();
    }
  }

  function onKeyDown(e) {
    if (e.key === 'Escape') {
      unhighlightElement(currentTarget);
      hideBadge();
      currentTarget = null;
    }
    if ((e.altKey || e.metaKey) && e.key === 'i') {
      e.preventDefault();
      toggleOverlay();
    }
  }

  // ── Activation ─────────────────────────────────────────────────────────────
  function toggleOverlay() {
    active = !active;
    if (active) {
      document.addEventListener('mousemove', onMouseMove, true);
      console.log('[SSHX overlay] active — hover elements to inspect');
    } else {
      document.removeEventListener('mousemove', onMouseMove, true);
      unhighlightElement(currentTarget);
      hideBadge();
      currentTarget = null;
      console.log('[SSHX overlay] inactive');
    }
  }

  document.addEventListener('keydown', onKeyDown, true);
  console.log('[SSHX overlay] loaded — press Alt+I to toggle');

  // ── SSHX connection (wired up in Part 3) ──────────────────────────────────
  window.__sshxOpenFileCard = null; // set by Part 3 init

  function openFileCardInSshx(x, y, filePath) {
    if (window.__sshxOpenFileCard) {
      window.__sshxOpenFileCard(x, y, filePath);
    } else {
      console.warn('[SSHX overlay] not connected to SSHX yet');
    }
  }
})();
```

### Inject into Next.js layout

In `app/layout.tsx`, add the script tag (and expose the env var via `next.config.ts`):

```tsx
// app/layout.tsx
export default function RootLayout({ children }: { children: React.ReactNode }) {
  const sshxUrl = process.env.NEXT_PUBLIC_SSHX_URL ?? '';
  return (
    <html lang="en">
      <head>
        {sshxUrl && (
          <>
            <script
              dangerouslySetInnerHTML={{
                __html: `window.__SSHX_URL = ${JSON.stringify(sshxUrl)};`,
              }}
            />
            <script src="/sshx-overlay.js" defer />
            <script src="/sshx-connect.js" defer />
          </>
        )}
      </head>
      <body>{children}</body>
    </html>
  );
}
```

---

## Part 3 — WebSocket Connection to SSHX

Create `public/sshx-connect.js`. This file connects to the SSHX WebSocket, authenticates, and exposes `window.__sshxOpenFileCard`.

### URL parsing

```
https://homa-server2.gaur-toad.ts.net:5173/s/SESSION_ID#ENCRYPTION_KEY
                                              ^^^^^^^^^^  ^^^^^^^^^^^^^^
```

```js
function parseSshxUrl(url) {
  const u = new URL(url);
  // Session ID: last path segment
  const sessionId = u.pathname.replace(/^\/s\//, '');
  // Key: URL fragment (never sent to server)
  const key = u.hash.replace(/^#/, '');
  // WebSocket endpoint
  const wsProto = u.protocol === 'https:' ? 'wss:' : 'ws:';
  const wsUrl = `${wsProto}//${u.host}/api/s/${sessionId}`;
  return { sessionId, key, wsUrl };
}
```

### Encryption: deriving the key and computing `encrypted_zeros`

SSHX's encryption scheme:
1. **KDF**: `argon2id(password=key, salt=sessionId, m=19456, t=1, p=1)` → 16 bytes
2. **Cipher**: AES-128-CTR, nonce = random 16 bytes
3. **`encrypted_zeros`**: encrypt 32 zero bytes → `{ nonce (16 bytes) || ciphertext (32 bytes) }` = 48 bytes, base64-encoded

This proves you hold the key without revealing it.

```js
// Load argon2-browser lazily (WASM, ~200 KB)
async function loadArgon2() {
  if (window.argon2) return window.argon2;
  await new Promise((resolve, reject) => {
    const s = document.createElement('script');
    // Use the ESM-compatible CDN build:
    s.src = 'https://cdn.jsdelivr.net/npm/argon2-browser@1.18.0/dist/argon2-bundled.min.js';
    s.onload = resolve;
    s.onerror = reject;
    document.head.appendChild(s);
  });
  return window.argon2;
}

async function deriveKey(keyStr, sessionId) {
  const argon2 = await loadArgon2();
  const result = await argon2.hash({
    pass: keyStr,
    salt: sessionId,
    type: argon2.ArgonType.Argon2id,
    hashLen: 16,
    mem: 19456,
    time: 1,
    parallelism: 1,
  });
  // result.hash is a Uint8Array of 16 bytes
  return result.hash;
}

async function computeEncryptedZeros(keyBytes, nonce) {
  // Import raw AES-128-CTR key
  const cryptoKey = await crypto.subtle.importKey(
    'raw',
    keyBytes,
    { name: 'AES-CTR' },
    false,
    ['encrypt']
  );
  const zeros = new Uint8Array(32);
  const ciphertext = await crypto.subtle.encrypt(
    { name: 'AES-CTR', counter: nonce, length: 64 },
    cryptoKey,
    zeros
  );
  // Concatenate nonce (16) + ciphertext (32) = 48 bytes
  const result = new Uint8Array(48);
  result.set(nonce, 0);
  result.set(new Uint8Array(ciphertext), 16);
  return result;
}
```

### CBOR encoding

SSHX uses cborx (a fast CBOR library). Use it from a CDN ESM import, or copy the UMD build to `public/`:

```js
// Option A: CDN (requires importmap or a module script)
// <script type="importmap">{"imports": {"cborx": "https://cdn.jsdelivr.net/npm/cbor-x@1.5.4/dist/node.cjs"}}</script>

// Option B (simpler for public/): use the encode function from cbor-x UMD
// npm install cbor-x, then copy node_modules/cbor-x/dist/node.cjs to public/cbor-x.cjs
// <script src="/cbor-x.cjs"></script> — exposes window.cborx

// Minimal CBOR encoder sufficient for our messages (arrays and strings):
function encodeCbor(value) {
  // Use cborx if available
  if (window.cborx?.encode) return window.cborx.encode(value);
  throw new Error('cbor-x not loaded');
}
```

For a zero-dependency fallback, implement minimal CBOR encoding inline (sufficient for arrays of strings and bytes):

```js
function encodeCborMinimal(value) {
  const parts = [];
  function writeValue(v) {
    if (v === null || v === undefined) {
      parts.push(new Uint8Array([0xf6])); // null
    } else if (typeof v === 'string') {
      const bytes = new TextEncoder().encode(v);
      parts.push(encodeHead(3, bytes.length));
      parts.push(bytes);
    } else if (typeof v === 'number' && Number.isInteger(v)) {
      if (v >= 0) parts.push(encodeHead(0, v));
      else parts.push(encodeHead(1, -1 - v));
    } else if (v instanceof Uint8Array) {
      parts.push(encodeHead(2, v.length));
      parts.push(v);
    } else if (Array.isArray(v)) {
      parts.push(encodeHead(4, v.length));
      v.forEach(writeValue);
    } else if (typeof v === 'object') {
      const keys = Object.keys(v);
      parts.push(encodeHead(5, keys.length));
      keys.forEach(k => { writeValue(k); writeValue(v[k]); });
    }
  }
  function encodeHead(major, value) {
    const tag = major << 5;
    if (value <= 23) return new Uint8Array([tag | value]);
    if (value <= 0xff) return new Uint8Array([tag | 24, value]);
    if (value <= 0xffff) return new Uint8Array([tag | 25, value >> 8, value & 0xff]);
    return new Uint8Array([tag | 26, (value>>>24)&0xff, (value>>>16)&0xff, (value>>>8)&0xff, value&0xff]);
  }
  writeValue(value);
  const total = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(total);
  let offset = 0;
  for (const p of parts) { out.set(p, offset); offset += p.length; }
  return out;
}
```

### SSHX WsClient message format

Looking at the SSHX protocol (`crates/sshx-server/src/web/protocol.rs`), WsClient messages are CBOR-encoded enums. The TypeScript mirror in `src/lib/protocol.ts` shows them as discriminated unions serialized as `["VariantName", ...args]`.

Messages we need:
- `Authenticate`: `["Authenticate", encryptedZeros, writePassword?]`
  - `encryptedZeros`: `Uint8Array` (48 bytes)
  - `writePassword`: `null` (unless session has a write password)
- `Create`: `["Create", x, y, w, h]` — opens a new shell window (not what we want here)

**Opening a FileCard** requires a custom WS message that doesn't exist yet in the base SSHX. See the SSHX Changes section below. For now, the overlay can encode a `Create` window at the given position and set the content via normal means.

### Full connection script

`public/sshx-connect.js`:

```js
(async function () {
  'use strict';

  const sshxUrl = window.__SSHX_URL;
  if (!sshxUrl) return;

  let ws = null;
  let authenticated = false;
  let keyBytes = null;
  let sessionId = null;

  function parseSshxUrl(url) {
    const u = new URL(url);
    const sid = u.pathname.split('/').pop();
    const key = u.hash.replace(/^#/, '');
    const wsProto = u.protocol === 'https:' ? 'wss:' : 'ws:';
    return { sessionId: sid, key, wsUrl: `${wsProto}//${u.host}/api/s/${sid}` };
  }

  async function deriveKey(keyStr, sid) {
    const argon2 = await loadArgon2();
    const result = await argon2.hash({
      pass: keyStr, salt: sid,
      type: argon2.ArgonType.Argon2id,
      hashLen: 16, mem: 19456, time: 1, parallelism: 1,
    });
    return result.hash;
  }

  async function computeEncryptedZeros(kb) {
    const nonce = crypto.getRandomValues(new Uint8Array(16));
    const cryptoKey = await crypto.subtle.importKey(
      'raw', kb, { name: 'AES-CTR' }, false, ['encrypt']
    );
    const ct = await crypto.subtle.encrypt(
      { name: 'AES-CTR', counter: nonce, length: 64 },
      cryptoKey, new Uint8Array(32)
    );
    const out = new Uint8Array(48);
    out.set(nonce, 0);
    out.set(new Uint8Array(ct), 16);
    return out;
  }

  // Minimal CBOR encode (see above)
  function cbor(v) { return encodeCborMinimal(v); }

  async function connect() {
    const { sessionId: sid, key, wsUrl } = parseSshxUrl(sshxUrl);
    sessionId = sid;

    keyBytes = await deriveKey(key, sid);

    ws = new WebSocket(wsUrl);
    ws.binaryType = 'arraybuffer';

    ws.onopen = () => console.log('[SSHX connect] WebSocket open');

    ws.onmessage = async (event) => {
      // We receive CBOR — decode only what we need
      // First message is always Hello (or Error)
      if (!authenticated) {
        // Send Authenticate regardless of message content
        const ez = await computeEncryptedZeros(keyBytes);
        ws.send(cbor(['Authenticate', ez, null]));
        authenticated = true;
        console.log('[SSHX connect] authenticated');
      }
      // Handle HighlightComponent (Part 4)
      tryHandleHighlight(event.data);
    };

    ws.onclose = (e) => {
      console.log('[SSHX connect] closed', e.code);
      authenticated = false;
      // Reconnect after 5 seconds
      setTimeout(connect, 5000);
    };

    ws.onerror = (e) => console.warn('[SSHX connect] error', e);
  }

  // ── Open FileCard ────────────────────────────────────────────────────────
  // Canvas coordinates: convert from viewport (clientX/Y) using SSHX's
  // transform. If we can't read it, default to center-canvas.
  function viewportToCanvas(clientX, clientY) {
    // SSHX stores pan/zoom in the page URL hash or in the canvas element's
    // CSS transform. Try to read it; fall back to a reasonable default.
    const canvas = document.querySelector('[data-sshx-canvas]');
    if (canvas) {
      const style = getComputedStyle(canvas);
      const matrix = new DOMMatrix(style.transform);
      // Inverse transform: (clientX - tx) / scale
      const scale = matrix.a || 1;
      const tx = matrix.e || 0;
      const ty = matrix.f || 0;
      return {
        x: Math.round((clientX - tx) / scale),
        y: Math.round((clientY - ty) / scale),
      };
    }
    // Default: position near click, assume no pan/zoom
    return { x: clientX + 200, y: clientY - 50 };
  }

  window.__sshxOpenFileCard = function (clientX, clientY, filePath) {
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      console.warn('[SSHX connect] not connected');
      return;
    }
    const { x, y } = viewportToCanvas(clientX, clientY);
    // Send OpenFileCard message (requires SSHX server support — see Part 4)
    // If not yet implemented, falls back to a no-op on the server.
    ws.send(cbor(['OpenFileCard', x, y, filePath]));
    console.log('[SSHX connect] OpenFileCard', filePath, 'at', x, y);
  };

  // ── Bidirectional highlight (Part 4) ──────────────────────────────────────
  function tryHandleHighlight(data) {
    // Minimal CBOR decode for ["HighlightComponent", name] messages
    // Full decode requires cbor-x; here we use a heuristic text search
    try {
      const text = new TextDecoder().decode(data);
      if (text.includes('HighlightComponent')) {
        // Extract component name (crude but effective for ASCII names)
        const match = text.match(/HighlightComponent[^\w]+([\w]+)/);
        if (match) flashComponent(match[1]);
      }
    } catch (_) {}
  }

  function flashComponent(name) {
    const targets = findByFiberName(name);
    targets.forEach(el => {
      el.style.outline = '3px solid #f59e0b';
      el.style.outlineOffset = '2px';
      setTimeout(() => {
        el.style.outline = '';
        el.style.outlineOffset = '';
      }, 1500);
    });
  }

  function findByFiberName(name) {
    // Walk all DOM elements and check fiber type name
    const results = [];
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT);
    let node;
    while ((node = walker.nextNode())) {
      const key = Object.keys(node).find(k => k.startsWith('__reactFiber$'));
      if (!key) continue;
      let fiber = node[key];
      while (fiber) {
        if (typeof fiber.type === 'function' && fiber.type.name === name) {
          results.push(node);
          break;
        }
        fiber = fiber.return;
      }
    }
    return results;
  }

  // ── Minimal CBOR encoder ──────────────────────────────────────────────────
  function encodeCborMinimal(value) {
    const parts = [];
    function w(v) {
      if (v === null || v === undefined) { parts.push(new Uint8Array([0xf6])); return; }
      if (typeof v === 'boolean') { parts.push(new Uint8Array([v ? 0xf5 : 0xf4])); return; }
      if (typeof v === 'string') {
        const b = new TextEncoder().encode(v);
        parts.push(h(3, b.length)); parts.push(b); return;
      }
      if (typeof v === 'number') {
        if (Number.isInteger(v) && v >= 0) { parts.push(h(0, v)); return; }
        if (Number.isInteger(v) && v < 0) { parts.push(h(1, -1 - v)); return; }
        // float64
        const buf = new ArrayBuffer(9); const view = new DataView(buf);
        view.setUint8(0, 0xfb); view.setFloat64(1, v);
        parts.push(new Uint8Array(buf)); return;
      }
      if (v instanceof Uint8Array) { parts.push(h(2, v.length)); parts.push(v); return; }
      if (Array.isArray(v)) { parts.push(h(4, v.length)); v.forEach(w); return; }
      if (typeof v === 'object') {
        const keys = Object.keys(v);
        parts.push(h(5, keys.length));
        keys.forEach(k => { w(k); w(v[k]); }); return;
      }
    }
    function h(major, n) {
      const tag = major << 5;
      if (n <= 23) return new Uint8Array([tag | n]);
      if (n <= 0xff) return new Uint8Array([tag | 24, n]);
      if (n <= 0xffff) return new Uint8Array([tag | 25, n >> 8, n & 0xff]);
      return new Uint8Array([tag | 26, (n>>>24)&0xff, (n>>>16)&0xff, (n>>>8)&0xff, n&0xff]);
    }
    w(value);
    const total = parts.reduce((s, p) => s + p.length, 0);
    const out = new Uint8Array(total);
    let off = 0;
    for (const p of parts) { out.set(p, off); off += p.length; }
    return out;
  }

  // ── Load argon2-browser ───────────────────────────────────────────────────
  function loadArgon2() {
    if (window.argon2) return Promise.resolve(window.argon2);
    return new Promise((resolve, reject) => {
      const s = document.createElement('script');
      s.src = 'https://cdn.jsdelivr.net/npm/argon2-browser@1.18.0/dist/argon2-bundled.min.js';
      s.onload = () => resolve(window.argon2);
      s.onerror = reject;
      document.head.appendChild(s);
    });
  }

  // ── Start ─────────────────────────────────────────────────────────────────
  connect().catch(e => console.error('[SSHX connect] init failed', e));
})();
```

---

## Part 4 — Bidirectional: SSHX → Highlight in Next.js App

This requires adding a new WebSocket message to the SSHX server. Follow the extension pattern from `CLAUDE.md`.

### New message: `WsServer::HighlightComponent`

**`crates/sshx-server/src/web/protocol.rs`** — add to `WsServer` enum:
```rust
/// Sent to browsers to highlight a component in any connected observability overlay.
HighlightComponent(String),
```

**`src/lib/protocol.ts`** — add to `WsServer` discriminated union:
```ts
| { type: "HighlightComponent"; name: string }
```

**`crates/sshx-server/src/web/socket.rs`** — handle a new `WsClient` variant that triggers it:
```rust
// In handle_client_message:
WsClient::HighlightComponent(name) => {
    session.broadcast(WsServer::HighlightComponent(name)).await;
}
```

**New `WsClient` variant** in `protocol.rs`:
```rust
/// Trigger component highlight in connected overlay clients.
HighlightComponent(String),
```

**`src/lib/protocol.ts`** — add to `WsClient`:
```ts
| { type: "HighlightComponent"; name: string }
```

**`src/lib/Session.svelte`** — handle incoming `HighlightComponent` (if the SSHX session itself has a component tree — not applicable, but forward to connected overlay tabs via BroadcastChannel):
```svelte
// In the WS message handler:
} else if (msg.type === "HighlightComponent") {
  // Forward to any connected overlay tabs via BroadcastChannel
  const bc = new BroadcastChannel("sshx-overlay");
  bc.postMessage({ type: "HighlightComponent", name: msg.name });
  bc.close();
}
```

The overlay in `sshx-connect.js` then listens on `BroadcastChannel("sshx-overlay")` — or directly on the WebSocket — to receive this and call `flashComponent(name)`.

Alternatively, add a UI button in the SSHX FileCard to trigger highlighting — when the user is viewing a FileCard for `app/components/Header.tsx`, a "Highlight" button sends `WsClient::HighlightComponent("Header")` which propagates to all connected overlay instances.

---

## Part 5 — Also: `WsClient::OpenFileCard`

To support the "Show in SSHX" button from the overlay, add `OpenFileCard` to the SSHX protocol.

**`crates/sshx-server/src/web/protocol.rs`** — add to `WsClient`:
```rust
/// Open a FileCard for a source file at the given canvas position.
OpenFileCard { x: f32, y: f32, path: String },
```

**`crates/sshx-server/src/web/socket.rs`** — handle it:
```rust
WsClient::OpenFileCard { x, y, path } => {
    // Broadcast to all browsers: open a FileCard widget
    session.broadcast(WsServer::OpenFileCard { x, y, path }).await;
}
```

**`crates/sshx-server/src/web/protocol.rs`** — add to `WsServer`:
```rust
/// Instructs browsers to open a FileCard at the given canvas position.
OpenFileCard { x: f32, y: f32, path: String },
```

**`src/lib/protocol.ts`**:
```ts
// WsClient:
| { type: "OpenFileCard"; x: number; y: number; path: string }
// WsServer:
| { type: "OpenFileCard"; x: number; y: number; path: string }
```

**`src/lib/Session.svelte`** — handle incoming `OpenFileCard`:
```svelte
} else if (msg.type === "OpenFileCard") {
  // Add to sourceFiles list (same as WsServer::SourceFiles but for a single new file)
  // Trigger the same flow as creating a FileCard from the toolbar
  dispatch("openFileCard", { x: msg.x, y: msg.y, path: msg.path });
}
```

---

## Workspace Analysis

Run `sshx analyze` on your Next.js project before starting a session. This indexes all source files and populates the FileTree and FileCard widgets in the SSHX canvas.

```bash
sshx analyze --workspace /path/to/your-nextjs-app
```

Output example:
```
  sshx analyze  Scanning /home/user/my-nextjs-app…

  ✓  67 files indexed
      component    56
      config       5
      hook         1
      utility      5

  ℹ  descriptions: 0/67

  →  /home/user/my-nextjs-app/.sshx-analysis.json
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

---

## Step-by-Step MVP

### Prerequisites

- SSHX server running and accessible (e.g. `https://homa-server2.gaur-toad.ts.net:5173`)
- Next.js 15 app at `/home/homaserver2/clawd/site-ia-gen`
- A running SSHX session with its URL + key fragment

### Step 1: Create the overlay files

```bash
cd /home/homaserver2/clawd/site-ia-gen
# Copy sshx-overlay.js and sshx-connect.js to public/
```

Paste the contents from Parts 2 and 3 into `public/sshx-overlay.js` and `public/sshx-connect.js`.

### Step 2: Add script tags to layout

```bash
# Edit app/layout.tsx
```

Add the snippet from Part 2 (the `sshxUrl` conditional script injection).

### Step 3: Set the SSHX URL

```bash
# .env.local
NEXT_PUBLIC_SSHX_URL=https://homa-server2.gaur-toad.ts.net:5173/s/SESSION_ID#ENCRYPTION_KEY
```

Get your session URL by running `sshx` in a terminal in the project directory.

### Step 4: Start Next.js in dev mode

```bash
cd /home/homaserver2/clawd/site-ia-gen
npm run dev
```

Open the app in your browser. Open the browser console and verify:

```
[SSHX overlay] loaded — press Alt+I to toggle
[SSHX connect] WebSocket open
[SSHX connect] authenticated
```

Press `Alt+I` to activate the overlay. Hover a React component — you should see a badge with the component name and file path.

### Step 5: Open a FileCard

Click "Show in SSHX" in the badge. Check the browser console for:
```
[SSHX connect] OpenFileCard app/components/Header.tsx at 450 200
```

This will only create a FileCard if `WsClient::OpenFileCard` is implemented in the SSHX server (see Part 5). Without that change, the message is silently dropped.

---

## SSHX Changes Summary

| Change | Required for | Files |
|---|---|---|
| `WsClient::OpenFileCard` | "Show in SSHX" button → FileCard | `protocol.rs`, `protocol.ts`, `socket.rs`, `Session.svelte` |
| `WsServer::OpenFileCard` | Same | Same |
| `WsClient::HighlightComponent` | SSHX → highlight in overlay | `protocol.rs`, `protocol.ts`, `socket.rs` |
| `WsServer::HighlightComponent` | Same | `protocol.rs`, `protocol.ts`, `Session.svelte` |

The **unidirectional MVP** (hover + badge in dev mode only, no FileCard opening) requires **zero SSHX changes** — the overlay works entirely in the Next.js browser context.

The **FileCard integration** requires the two `OpenFileCard` protocol additions.

The **bidirectional highlight** requires the two `HighlightComponent` protocol additions.

---

## Troubleshooting

**Badge shows "no source"**

- Make sure you're running `npm run dev` — `_debugSource` is stripped in production builds and the overlay will not work
- Verify you're using React 18+ (fiber structure may differ in React 16/17)
- Check that `__reactFiber$` keys exist: in the browser console, run `Object.keys(document.querySelector('h1'))` and look for a key starting with `__reactFiber$`

**WebSocket connection fails**

- Check the session URL and key are correct (copy directly from the SSHX terminal output)
- The key is the URL fragment — it appears after `#` in the SSHX link
- Check CORS: SSHX doesn't filter by Origin, but confirm the WS URL matches the server host exactly

**argon2-browser fails to load**

- The CDN URL requires internet access; use `npm install argon2-browser` and serve from `public/` in air-gapped environments
- Alternatively, skip argon2 by using a server-configured pre-shared HMAC token (requires server changes)

**"Show in SSHX" does nothing**

- Confirm the `OpenFileCard` message is implemented in the SSHX server (see Part 5)
- Without it, the message reaches the server but has no handler and is silently ignored
- Check the SSHX server logs for unexpected CBOR decode errors
