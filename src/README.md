# src/ — Svelte Frontend

**Framework:** SvelteKit (SSR disabled — fully static/SPA)
**Language:** TypeScript
**Styling:** Tailwind CSS (dark theme, zinc palette)
**Build output:** `../build/` (committed, served directly by `sshx-server`)

## What It Does

The frontend is a real-time collaborative terminal viewer running entirely in the browser. It:

1. Connects to the server via WebSocket (CBOR-X binary protocol)
2. Decrypts terminal output in-browser using the URL fragment as the encryption key
3. Renders xterm.js terminal emulators on an infinite panning/zooming canvas
4. Shows real-time cursors, chat, and user presence for all connected collaborators

## Directory Structure

```
src/
├── lib/
│   ├── Session.svelte     # ★ Main session component (state machine + canvas)
│   ├── encrypt.ts         # Client-side Argon2id + AES-CTR encryption
│   ├── srocket.ts         # Reconnecting WebSocket with CBOR-X
│   ├── protocol.ts        # TypeScript types mirroring the WS protocol
│   ├── settings.ts        # Persisted user settings (localStorage)
│   ├── arrange.ts         # Algorithm for placing new terminal windows
│   ├── lock.ts            # Async mutex for sequential crypto operations
│   ├── toast.ts           # Toast notification helpers
│   ├── typeahead.ts       # xterm.js typeahead addon
│   ├── action/            # Svelte custom actions
│   │   ├── slide.ts       # Slide transition action
│   │   └── touchZoom.ts   # Pinch-to-zoom gesture action
│   ├── ui/                # All UI components
│   └── assets/            # SVG images (logo, landing graphics)
└── routes/
    ├── +layout.svelte     # Root layout
    ├── +error.svelte      # Error page
    ├── +page.svelte       # Landing page
    └── s/[id]/
        └── +page.svelte   # Session view (mounts Session.svelte)
```

## Core Libraries

### `Session.svelte` — Central State Machine

The most important file in the frontend. It:
- Owns the WebSocket connection (`srocket`)
- Owns the `Encrypt` instance
- Manages all session state: shells, users, chat, subscriptions
- Handles all incoming `WsServer` messages
- Dispatches all outgoing `WsClient` messages
- Renders the infinite canvas with drag/drop, pan, zoom
- Coordinates XTerm instances with encryption and input routing

See `src/lib/README.md` for full details.

### `encrypt.ts` — Client-Side Encryption

```typescript
class Encrypt {
  async zeros(): Promise<Uint8Array>       // Proof of key ownership
  async segment(stream, offset, data)      // Encrypt/decrypt a data chunk
}
```

Uses:
- `argon2-browser` (WASM) for Argon2id KDF
- `crypto.subtle` (Web Crypto API) for AES-128-CTR

Parameters match Rust exactly: same salt, same Argon2 memory/iterations, same stream numbering.

### `srocket.ts` — Reconnecting WebSocket

```typescript
class Srocket<ServerMsg, ClientMsg> {
  send(msg: ClientMsg): void
  onmessage: (msg: ServerMsg) => void
  onopen: () => void
  onclose: () => void
}
```

- Auto-reconnects after 500ms on disconnect
- Buffers up to 64 outgoing messages while disconnected
- Serializes/deserializes with CBOR-X

### `protocol.ts` — Type Definitions

TypeScript types that exactly mirror `crates/sshx-server/src/web/protocol.rs`:
- `WsWinsize`, `WsUser`
- `WsServer` (tagged union) — all server→client messages
- `WsClient` (tagged union) — all client→server messages
- `Sid`, `Uid` type aliases (`number`)

### `settings.ts` — User Preferences

Backed by `svelte-persisted-store` (localStorage). Settings:
- `name`: Display name for cursors/chat
- `theme`: Terminal color theme key
- `scrollback`: xterm.js scrollback buffer size

### `arrange.ts` — Window Placement

`arrangeNewTerminal(existing)` finds a non-overlapping canvas position for a new terminal window using a golden-angle spiral search.

### `lock.ts` — Async Mutex

Simple queue-based async lock. Used in `Session.svelte` to ensure encryption operations for a given shell are processed sequentially (not interleaved).

## Routes

### `routes/+page.svelte` — Landing Page

Marketing/documentation page. Shows:
- Hero section with demo video
- Feature list
- Install instructions (curl + platform download links)
- `DownloadLink` components for each platform binary

### `routes/s/[id]/+page.svelte` — Session View

Minimal wrapper that:
1. Extracts `id` from the URL parameter
2. Reads encryption key from URL fragment (`#<key>`)
3. Mounts `<Session>` component
4. Updates page `<title>` reactively

### `routes/+layout.svelte`

Root layout: wraps all pages, mounts `<ToastContainer>`.

## Build & Development

```bash
# Development server (hot reload)
npm run dev

# Production build → ../build/
npm run build

# Type check
npm run check

# Lint
npm run lint
```

The built `build/` directory is committed to the repo and served directly by `sshx-server` (no separate web server needed).

## Styling Conventions

- **Tailwind CSS** throughout — no plain CSS except in component `<style>` blocks for xterm overrides
- **Dark theme by default** — `zinc` color palette, `bg-zinc-900` backgrounds
- **No light mode** — the app is terminal-centric, dark only
- Custom fonts: **Inter** (UI), **Fira Code** (terminals, loaded by XTerm.svelte)

## Key Dependencies

| Package | Purpose |
|---|---|
| `svelte` / `@sveltejs/kit` | UI framework + routing |
| `xterm` / `sshx-xterm` | Terminal emulation |
| `cbor-x` | Binary WebSocket serialization |
| `argon2-browser` | WASM Argon2id for key derivation |
| `perfect-cursors` | Smooth cursor interpolation |
| `svelte-persisted-store` | localStorage-backed stores |
| `svelte-feather-icons` | Icon set |
| `lodash-es` | Utility functions |
| `tailwindcss` | Utility-first CSS |

## See Also

- `src/lib/README.md` — detailed lib/ breakdown
- `src/lib/ui/README.md` — UI component catalog
- Root `CLAUDE.md` — full architecture and extension guide
