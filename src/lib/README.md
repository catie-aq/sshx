# src/lib/ — Core Frontend Logic

This directory contains the core logic of the sshx frontend: the main session component, encryption, WebSocket communication, and reusable utilities.

## File Overview

```
lib/
├── Session.svelte     # ★ Main container: state machine + infinite canvas
├── encrypt.ts         # Client-side E2E encryption (Argon2id + AES-CTR)
├── srocket.ts         # Reconnecting WebSocket client with CBOR-X
├── protocol.ts        # TypeScript types for the WebSocket protocol
├── settings.ts        # Persisted user settings (localStorage)
├── arrange.ts         # Window placement algorithm
├── lock.ts            # Async mutex
├── toast.ts           # Toast notification helpers
├── typeahead.ts       # xterm.js typeahead prediction addon
├── action/            # Svelte custom actions
│   ├── slide.ts       # Slide-in/out transition
│   └── touchZoom.ts   # Pinch-to-zoom gesture handler
├── ui/                # UI components (see ui/README.md)
└── assets/            # SVG images
```

---

## `Session.svelte` — The Heart of the App

This is the most complex file in the project (~800+ lines). It is the stateful entry point for any active session.

### Props

```typescript
export let name: string;          // Session ID from URL
export let encryptKey: string;    // Encryption key from URL fragment
export let userId: Uid | null;    // Assigned by server after auth
```

### State

| Variable | Type | Purpose |
|---|---|---|
| `encrypt` | `Encrypt \| null` | E2E encryption instance (null while initializing) |
| `srocket` | `Srocket` | WebSocket connection |
| `shells` | `Map<Sid, WsWinsize>` | Active terminal windows and their positions |
| `users` | `Map<Uid, WsUser>` | Connected users (name, cursor, focus) |
| `chatMessages` | `ChatMessage[]` | Chat history |
| `writers` | `Map<Sid, WriteFn>` | Functions to write data into each XTerm |
| `subscriptions` | `Set<Sid>` | Which shells have been subscribed to |
| `chunknums` | `Map<Sid, number>` | Last received chunk index per shell |
| `locks` | `Map<Sid, Lock>` | Per-shell async mutex for encryption |
| `moving` | `Sid \| null` | Shell being dragged |
| `resizing` | `Sid \| null` | Shell being resized |
| `serverLatencies` | `number[]` | Round-trip ping samples |
| `shellLatencies` | `number[]` | Backend-to-shell latency samples |

### Initialization Flow

```
1. Derive encryption key (Argon2id KDF) → create Encrypt instance
2. Connect WebSocket (Srocket)
3. Send Authenticate message (encrypted_zeros + optional write password)
4. Receive Hello → store userId, session name
5. Receive Shells → populate shell map, subscribe to each
6. Receive Users → populate user map
7. For each subscribed shell: receive Chunks → decrypt → write to XTerm
```

### Incoming Message Handlers (`WsServer`)

| Message | Action |
|---|---|
| `Hello` | Set `userId`, page title, send `SetName` |
| `InvalidAuth` | Show error toast |
| `Users` | Replace full user map |
| `UserDiff` | Add/update/remove one user |
| `Shells` | Replace full shell map, subscribe to new ones |
| `Chunks` | Decrypt output, write to correct XTerm |
| `Hear` | Append to chat history |
| `ShellLatency` | Record latency sample |
| `Pong` | Record ping latency |
| `Error` | Show error toast |

### Outgoing Actions (`WsClient`)

| Action | Trigger |
|---|---|
| `Create` | User clicks "+" button |
| `Close` | User closes a terminal window |
| `Move` | User drags or resizes a window |
| `Data` | User types in an XTerm |
| `Subscribe` | New shell appears, request its output |
| `SetCursor` | Mouse moves (debounced) |
| `SetFocus` | User clicks on a terminal |
| `Chat` | User submits a chat message |
| `Ping` | Every few seconds for latency measurement |

### Canvas Interaction

The canvas uses CSS `transform` for pan and zoom. Mouse coordinates are converted to canvas space for accurate window placement and cursor broadcast.

Custom Svelte actions:
- `use:touchZoom` — handles pinch-to-zoom on touch devices
- `use:slide` — slide transition for panels

---

## `encrypt.ts`

```typescript
class Encrypt {
  constructor(key: string)
  async zeros(): Promise<Uint8Array>
  async segment(streamNum: bigint, offset: number, data: Uint8Array): Promise<Uint8Array>
}
```

- `zeros()`: Returns `AES-CTR(key, stream=1, 16 zero bytes)` — used to authenticate without revealing the key
- `segment()`: Encrypts or decrypts `data` using AES-CTR at the given stream position

Stream number format: `BigInt` where upper 32 bits identify the stream, lower 32 bits are the byte offset. This matches the Rust implementation exactly.

**Important:** This is symmetric — the same call encrypts and decrypts.

---

## `srocket.ts`

```typescript
class Srocket<S, C> {
  constructor(url: string, opts: { onopen, onmessage, onclose })
  send(msg: C): void
  close(): void
}
```

Internally:
1. Opens a native `WebSocket`
2. Encodes outgoing messages with `cbor-x` encode
3. Decodes incoming `ArrayBuffer` messages with `cbor-x` decode
4. On disconnect: waits 500ms, reconnects
5. Buffers up to 64 outgoing messages while disconnected (queued, sent on reconnect)

---

## `protocol.ts`

Pure type definitions — no runtime code. Defines:

```typescript
type Sid = number;
type Uid = number;

type WsWinsize = { x: number; y: number; rows: number; cols: number };
type WsUser = { name: string; cursor: [number, number] | null; focus: Sid | null; canWrite: boolean };

// Tagged union — each variant is an object with one key
type WsServer = { Hello: [Uid, string] } | { Chunks: [Sid, number, Uint8Array[]] } | ...
type WsClient = { Authenticate: [Uint8Array, string | null] } | { Data: [Sid, Uint8Array, number] } | ...
```

These must stay in sync with `crates/sshx-server/src/web/protocol.rs`.

---

## `settings.ts`

```typescript
export const settings = persisted<Settings>("sshx-settings", defaults);
```

Settings object:
```typescript
{
  name: string;       // Display name
  theme: string;      // Key into themes.ts
  scrollback: number; // xterm.js scrollback lines
}
```

Uses `svelte-persisted-store` — automatically syncs to `localStorage`.

---

## `arrange.ts`

```typescript
function arrangeNewTerminal(existing: WsWinsize[]): { x: number; y: number }
```

Finds a non-overlapping position on the infinite canvas for a new terminal window. Uses a golden-angle spiral (angle ≈ 1.94161 radians) to spread windows outward from the origin.

---

## `lock.ts`

```typescript
class Lock {
  async run<T>(fn: () => Promise<T>): Promise<T>
}
```

A simple queue-based mutex. Ensures that async operations (e.g., decrypt+write for a shell) don't interleave even if chunks arrive rapidly.

---

## `action/touchZoom.ts`

Svelte action that handles pinch-to-zoom and two-finger pan gestures on the canvas. Emits synthetic `zoom` and `pan` events consumed by `Session.svelte`.

## `action/slide.ts`

Svelte action for a slide-in/out CSS transition, used for panels (chat, settings).

---

## See Also

- `src/lib/ui/README.md` — UI component catalog
- `src/README.md` — frontend overview
- Root `CLAUDE.md` — full architecture guide
