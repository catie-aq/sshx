# Adding a Full-Stack Feature to sshx

This guide walks through the complete process of adding a collaborative feature that spans the Rust backend and the Svelte frontend. The sticky-notes (Creative Mode) feature is used as the worked example throughout.

---

## Overview of the Stack

```
Browser (Svelte + xterm.js)
  ↕  WebSocket, CBOR-X binary, protocol defined in protocol.ts / protocol.rs
sshx-server (Axum + Tonic)
  ↕  gRPC (sshx.proto), only needed when touching CLI↔server comms
sshx CLI (PTY shell runner)
```

Most collaborative UI features only touch the **WebSocket layer** — you rarely need to modify the gRPC proto.

---

## Step 1 — Define the Data

Decide on:
- **What data the feature carries** (fields, types)
- **A unique ID type** if the feature introduces new server-side objects

### Add a new ID type (if needed)

In `crates/sshx-core/src/lib.rs`, model new IDs after `Sid` and `Uid`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Nid(pub u32);

impl Display for Nid { … }
```

Add a counter method to `IdCounter`:

```rust
pub fn next_nid(&self) -> Nid {
    Nid(self.next_nid.fetch_add(1, Ordering::Relaxed))
}
```

---

## Step 2 — Define the Protocol (Rust side)

All WebSocket messages live in `crates/sshx-server/src/web/protocol.rs`.

### Add a data struct

```rust
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WsNote {
    pub x: i32,
    pub y: i32,
    pub text: String,
    pub color: String,
    pub pinned: bool,
}
```

### Add server→client messages to `WsServer`

```rust
/// Snapshot sent on connect.
Notes(Vec<(Nid, WsNote)>),
/// Incremental diff (None = deleted).
NoteDiff(Nid, Option<WsNote>),
```

### Add client→server messages to `WsClient`

```rust
CreateNote(i32, i32),
UpdateNote(Nid, WsNote),
DeleteNote(Nid),
```

> **Serialization:** serde + ciborium (CBOR) handles everything automatically. `#[serde(rename_all = "camelCase")]` keeps field names consistent with TypeScript.

---

## Step 3 — Mirror the Protocol (TypeScript side)

`src/lib/protocol.ts` is the TypeScript twin of `protocol.rs`. Keep them in sync manually — there is no code generation.

```typescript
type Nid = number; // u32

export type WsNote = {
  x: number; y: number;
  text: string; color: string; pinned: boolean;
};

// Add to WsServer:
notes?: [Nid, WsNote][];
noteDiff?: [Nid, WsNote | null];

// Add to WsClient:
createNote?: [number, number];
updateNote?: [Nid, WsNote];
deleteNote?: Nid;
```

---

## Step 4 — Add Server-Side State

Session state lives in `crates/sshx-server/src/session.rs`.

### Add storage fields to `Session`

```rust
notes: RwLock<HashMap<Nid, WsNote>>,
notes_source: watch::Sender<Vec<(Nid, WsNote)>>,
```

Initialize both in `Session::new()`.

### Implement CRUD methods

Follow the existing shell pattern:

| Pattern | Example |
|---|---|
| Snapshot list | `pub fn list_notes(&self) -> Vec<(Nid, WsNote)>` |
| Watch stream | `pub fn subscribe_notes(&self) -> impl Stream<…>` using `WatchStream` |
| Create | insert into `RwLock<HashMap>`, push to watch sender, call `sync_now()` |
| Update | mutate in HashMap, update watch sender |
| Delete | remove from HashMap, retain-filter watch sender |

### Why `watch` vs `broadcast`?

- **`watch`** — new subscribers always get the latest snapshot. Use for state that a reconnecting client needs in full (shells, notes, users list).
- **`broadcast`** — fire-and-forget events (chat messages, latency readings). Dropped messages are acceptable.

---

## Step 5 — Wire Up the Socket Handler

In `crates/sshx-server/src/web/socket.rs`:

**On connect** — send a snapshot immediately after authentication:

```rust
send(socket, WsServer::Notes(session.list_notes())).await?;
```

**Add a stream arm** in the `tokio::select!` loop so all connected clients stay in sync:

```rust
let mut notes_stream = session.subscribe_notes();
// inside select!:
Some(notes) = notes_stream.next() => {
    send(socket, WsServer::Notes(notes)).await?;
    continue;
}
```

**Handle client messages** in the `match msg { … }` block:

```rust
WsClient::CreateNote(x, y) => {
    session.check_write_permission(user_id)?;
    let id = session.counter().next_nid();
    session.add_note(id, x, y)?;
}
WsClient::UpdateNote(id, note) => { … }
WsClient::DeleteNote(id) => { … }
```

Always check write permission before mutating state.

---

## Step 6 — Build and Verify the Backend

```bash
cargo build -p sshx-core -p sshx-server
```

Fix any type errors before touching the frontend. Rust's compiler messages are precise — missing match arms and unused imports are caught here.

---

## Step 7 — Build the UI Component

Create `src/lib/ui/YourComponent.svelte`.

Key conventions:
- **Props** via `export let` for data flowing in
- **Events** via `createEventDispatcher` for actions flowing out (keeps the component pure)
- **Styling** with Tailwind utility classes; dark theme uses the `zinc` palette
- Check what icons are available: `ls node_modules/svelte-feather-icons/src/icons/`
- Dispatch a `startMove` event (with the raw `MouseEvent`) rather than handling drag internally — `Session.svelte` owns all global mouse tracking

```svelte
<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { WsNote } from "$lib/protocol";

  export let note: WsNote;
  export let canWrite: boolean;

  const dispatch = createEventDispatcher<{
    update: WsNote;
    delete: void;
    startMove: MouseEvent;
  }>();
</script>
```

---

## Step 8 — Integrate into Session.svelte

`src/lib/Session.svelte` is the main state machine. Changes here follow a consistent pattern:

### Add reactive state

```typescript
let notes = new Map<number, WsNote>();
let movingNote: number | null = null;
let movingNotePos: { x: number; y: number } | null = null;
```

### Handle incoming messages

Add `else if` branches in the `onMessage` callback:

```typescript
} else if (message.notes) {
    notes = new Map(message.notes);
} else if (message.noteDiff) {
    const [nid, note] = message.noteDiff;
    if (note === null) notes.delete(nid);
    else notes.set(nid, note);
    notes = notes; // reassign to trigger Svelte reactivity
}
```

### Render on the canvas

Objects on the canvas use the `slide` action for smooth pan/zoom transforms:

```svelte
{#each [...notes] as [nid, note] (nid)}
  {@const pos = nid === movingNote ? movingNotePos ?? note : note}
  <div
    class="absolute"
    style:left={OFFSET_LEFT_CSS}
    style:top={OFFSET_TOP_CSS}
    style:transform-origin={OFFSET_TRANSFORM_ORIGIN_CSS}
    transition:fade|local
    use:slide={{ x: pos.x, y: pos.y, center, zoom, immediate: nid === movingNote }}
  >
    <YourComponent {note} {canWrite}
      on:update={({ detail }) => srocket?.send({ updateNote: [nid, detail] })}
      on:delete={() => srocket?.send({ deleteNote: nid })}
      on:startMove={({ detail: event }) => {
        const [x, y] = normalizePosition(event);
        movingNote = nid;
        movingNotePos = { x: note.x, y: note.y };
      }}
    />
  </div>
{/each}
```

### Hook into global mouse handling

The `handleMouse` / `handleMouseEnd` functions in the `onMount` block manage all drag state:

```typescript
// In handleMouse:
if (movingNote !== null && movingNotePos) {
    const [x, y] = normalizePosition(event);
    movingNotePos = { x: Math.round(x - originDx), y: Math.round(y - originDy) };
    sendMove({ updateNote: [movingNote, { ...notes.get(movingNote)!, ...movingNotePos }] });
}

// In handleMouseEnd:
if (movingNote !== null && movingNotePos) {
    sendMove.cancel();
    srocket?.send({ updateNote: [movingNote, { ...notes.get(movingNote)!, ...movingNotePos }] });
    movingNote = null;
    movingNotePos = null;
}
```

Use `throttle` (via lodash-es) on the move message for smooth real-time sync without flooding the server.

---

## Step 9 — Type-Check the Frontend

```bash
npm run check
```

The two pre-existing errors in `vite.config.ts` can be ignored. Fix any errors in your new files before testing.

---

## Step 10 — Manual End-to-End Test

```bash
mprocs   # starts sshx-server + vite dev + optionally the CLI client
```

Open the session URL in **two browser tabs** and verify:

1. Creating an object appears in both tabs
2. Editing is reflected after the debounce period
3. Instant interactions (color change, pin toggle) are immediate
4. Dragging syncs position to all tabs (throttled)
5. Deleting removes the object everywhere
6. **Reconnect test:** close and reopen one tab — verify the snapshot (`Notes` / `Shells`) restores full state

---

## Checklist

```
[ ] New ID type in sshx-core (if needed)
[ ] Data struct + WsServer/WsClient variants in protocol.rs
[ ] Matching types in protocol.ts
[ ] Storage fields + CRUD methods in session.rs
[ ] Socket handler: snapshot on connect + stream arm + message handlers
[ ] cargo build -p sshx-server  →  no errors
[ ] New .svelte UI component in src/lib/ui/
[ ] State + message handlers + canvas rendering in Session.svelte
[ ] npm run check  →  no new errors
[ ] Manual two-tab test with connect/disconnect cycle
```

---

## Design Notes

- **Write permission** — always call `session.check_write_permission(user_id)` before any mutation.
- **Watch vs broadcast** — use `watch` for snapshot-able state, `broadcast` for ephemeral events.
- **No persistence in v1** — `sync_now()` triggers a storage sync, but the proto `SerializedSession` must be extended separately to survive server restarts.
- **Mode is local only** — UI modes (terminal / creative) are per-client state; nothing is sent to the server.
- **`slide` action** — handles all pan/zoom math; pass `immediate: true` while dragging to skip the CSS transition.
