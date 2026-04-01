# sshx Svelte Frontend — Feature & Code Reference

> Source: `src/` — SvelteKit SPA with xterm.js terminals on an infinite canvas, CBOR-X WebSocket protocol, and client-side E2E encryption.

---

## Module Map

### Core Logic (`src/lib/`)

| Module | File | Purpose |
|--------|------|---------|
| `Session.svelte` | `src/lib/Session.svelte` | Main state machine: canvas, WS handling, all user interactions (~800+ lines) |
| `protocol.ts` | `src/lib/protocol.ts` | TypeScript types for `WsServer` / `WsClient` WebSocket messages |
| `encrypt.ts` | `src/lib/encrypt.ts` | Argon2id + AES-128-CTR (Web Crypto API + argon2-browser) |
| `srocket.ts` | `src/lib/srocket.ts` | Reconnecting WebSocket with CBOR-X binary encoding |
| `settings.ts` | `src/lib/settings.ts` | Persisted user preferences (name, theme, scrollback) via `svelte-persisted-store` |
| `arrange.ts` | `src/lib/arrange.ts` | Golden-angle spiral placement for new terminal windows |
| `lock.ts` | `src/lib/lock.ts` | Queue-based async mutex for sequential per-shell encryption |
| `toast.ts` | `src/lib/toast.ts` | Toast notification store + `makeToast()` helper |
| `typeahead.ts` | `src/lib/typeahead.ts` | xterm.js addon for Mosh-like local echo prediction |
| `fonts.ts` | `src/lib/fonts.ts` | Font definitions (Inter, Merriweather, Caveat, Nunito, etc.) |
| `history.ts` | `src/lib/history.ts` | `UndoHistory` class — undo/redo stack (max 100 actions) |
| `selection.ts` | `src/lib/selection.ts` | Multi-selection utilities (rect math, selectable types) |
| `runtimeGraph.ts` | `src/lib/runtimeGraph.ts` | Build runtime graph of Claude sessions ↔ files ↔ terminals |

### Custom Svelte Actions (`src/lib/action/`)

| Action | File | Purpose |
|--------|------|---------|
| `slide` | `action/slide.ts` | CSS transform position transition (150ms, cubicOut) |
| `TouchZoom` | `action/touchZoom.ts` | Pan/zoom: scroll, pinch, ctrl+scroll, middle-drag |

### Routes (`src/routes/`)

| Route | File | Purpose |
|-------|------|---------|
| `/` | `+page.svelte` | Landing page (marketing, download links) |
| `/s/[id]` | `s/[id]/+page.svelte` | Session page — mounts `Session.svelte` |
| `+layout.svelte` | `+layout.svelte` | Root layout (ToastContainer, Tailwind CSS) |
| `+error.svelte` | `+error.svelte` | 404 / error display |

---

## UI Components (`src/lib/ui/`)

### Terminal & Canvas

| Component | Purpose |
|-----------|---------|
| `XTerm.svelte` | xterm.js terminal widget with WebGL renderer, image addon, typeahead |
| `LiveCursor.svelte` | Per-user cursor overlay on canvas |
| `DrawingLayer.svelte` | Canvas freehand drawing (pencil, highlighter) |
| `DrawingToolbar.svelte` | Drawing tool selector + color picker |
| `SlideRegion.svelte` | Presentation slide region on canvas |
| `Timeline.svelte` | Slide timeline editor |

### Workspace & Files

| Component | Purpose |
|-----------|---------|
| `FileTreePanel.svelte` | Directory tree browser widget |
| `FileCard.svelte` | Source file metadata card (imports, exports, description, image) |
| `GraphView.svelte` | Component dependency graph visualization |
| `GraphOverlay.svelte` | Overlay highlighting for component selection |
| `LibraryCard.svelte` | Library/package info card |
| `OpenFileDialog.svelte` | File open dialog |

### Rich Content

| Component | Purpose |
|-----------|---------|
| `StickyNote.svelte` | Rich-text sticky note (Tiptap editor, color, font, resize) |
| `TextBlock.svelte` | Editable rich text block on canvas |
| `ImageWidget.svelte` | Image display widget on canvas |
| `AppOverlayWidget.svelte` | Embedded iframe widget |
| `FontSelector.svelte` | Font chooser dropdown |

### AI & IDE

| Component | Purpose |
|-----------|---------|
| `ClaudeActivityFeed.svelte` | Claude Code session activity viewer |
| `ClaudeExecutionGraph.svelte` | Claude runtime graph visualization |
| `ClaudeInstanceList.svelte` | Dropdown to select Claude session |
| `IdeEditorWidget.svelte` | IDE editor canvas widget |

### Chrome & Navigation

| Component | Purpose |
|-----------|---------|
| `Toolbar.svelte` | Top navigation bar (create shell, chat, settings, workspace, screen share) |
| `Chat.svelte` | Chat panel with message history |
| `Settings.svelte` | User preferences (theme, scrollback) |
| `NameList.svelte` | Connected user list |
| `ChooseName.svelte` | Name input dialog |
| `Avatars.svelte` | Stacked avatar circles in toolbar |
| `NetworkInfo.svelte` | Latency display panel |
| `CommandPalette.svelte` | Ctrl+K quick search/command |
| `ContextMenu.svelte` | Right-click context menu |
| `OverlayMenu.svelte` | Slide-in menu panel |

### Primitives

| Component | Purpose |
|-----------|---------|
| `CircleButton.svelte` | Red/yellow/green macOS-style circle button |
| `CircleButtons.svelte` | Circle button group wrapper |
| `Toast.svelte` | Single toast notification |
| `ToastContainer.svelte` | Toast mount point |
| `ScreenShareWidget.svelte` | Video stream canvas widget (WebRTC) |
| `DownloadLink.svelte` | Platform download button |
| `CopyableCode.svelte` | Code block with copy button |
| `TeaserVideo.svelte` | Demo video embed |

### Terminal Themes

| File | Purpose |
|------|---------|
| `themes.ts` | xterm.js color theme definitions |

---

## Session.svelte — State Overview

### Connection
- `encrypt: Encrypt` — E2E encryption instance
- `srocket: Srocket<WsServer, WsClient>` — WebSocket connection
- `connected: boolean`, `exitReason: string | null`
- `userId: number` — assigned by server `Hello` message

### Terminal State
- `shells: [Sid, WsWinsize][]` — active shell windows
- `writers: Record<Sid, (data) => void>` — xterm.js write callbacks
- `subscriptions: Set<Sid>` — subscribed shell IDs
- `shellNames: Map<Sid, string>` — user-set labels
- `chunknums: Record<Sid, number>` — last received chunk index
- `locks: Record<Sid, Lock>` — per-shell encryption mutex

### Canvas Objects
- `notes: Map<Nid, WsNote>` — sticky notes
- `textBlocks: Map<Tid, WsTextBlock>` — rich text blocks
- `drawings: Map<Did, WsDrawing>` — freehand strokes
- `slides: Map<Slid, WsSlide>` — presentation regions
- `widgets: Map<Wid, WsWidget>` — canvas widgets (7 kinds)

### Workspace
- `sourceFiles: Map<string, WsSourceFile>` — file metadata
- `componentGraph: WsComponentGraph | null` — dependency graph
- `workspaceRootName: string` — project folder name

### Collaboration
- `users: [Uid, WsUser][]` — connected users
- `chatMessages: ChatMessage[]` — chat history
- `videoStreams: Map<Vid, WsVideoStream>` — screen shares / browsers
- `browserControllers: Map<Vid, Uid | null>` — who controls each browser
- `ideStates: Map<Uid, WsIdeState>` — per-user IDE state

### UI Modes
- `majorMode: "none" | "edition" | "slides"` — active editing mode
- `graphMode: boolean` — component graph visualization
- `textToolActive`, `drawingTool`, `drawingColor` — tool state
- `commandPaletteOpen`, `settingsOpen`, `showChat`, `showNetworkInfo`

### History & Selection
- `undoHistory: UndoHistory` — undo/redo (max 100 actions)
- `selectedItems: SelectionItem[]` — multi-selected canvas objects
- `clipboard: {type, data}[]` — copy/paste buffer

---

## Keyboard Shortcuts (Session.svelte `onMount`)

| Key | Action |
|-----|--------|
| `Ctrl/Cmd+K` | Command palette |
| `t` | Toggle text tool |
| `n` | Create sticky note |
| `l` | Toggle pencil |
| `h` | Toggle highlighter |
| `Escape` | Close tool / exit slideshow |
| `Arrow keys` | Navigate slides (in slideshow) |
| `Ctrl/Cmd+C` | Copy selected |
| `Ctrl/Cmd+V` | Paste |
| `Ctrl/Cmd+Z` | Undo |
| `Ctrl/Cmd+Shift+Z` | Redo |
| `Delete` | Delete selected |

---

## Canvas Interaction (TouchZoom)

| Gesture | Action |
|---------|--------|
| Scroll | Pan vertically |
| Shift+Scroll | Pan horizontally (non-macOS) |
| Ctrl/Cmd+Scroll | Zoom (centered on cursor) |
| Alt+Scroll | Zoom |
| Pinch | Zoom |
| Middle-mouse drag | Pan |

Zoom range: `0.35` – `2.0` (clamped), default `1.0`.

---

## WebSocket Protocol (`src/lib/protocol.ts`)

### ID Types
```typescript
type Sid = number   // Shell ID
type Uid = number   // User ID
type Nid = number   // Note ID
type Wid = number   // Widget ID
type Vid = number   // Video stream ID
type Tid = number   // Text block ID
type Did = number   // Drawing ID
type Slid = number  // Slide ID
```

### Key Data Structures

**WsWinsize**: `{ x, y, rows, cols }`
**WsUser**: `{ name, cursor, focus, canWrite }`
**WsNote**: `{ x, y, text, color, pinned, w, h, font }`
**WsTextBlock**: `{ x, y, content, fontSize, color, align, font }`
**WsDrawing**: `{ tool, points, color, width, opacity }`
**WsSlide**: `{ x, y, w, h, order, label }`
**WsWidget**: `{ x, y, w, h, kind, collapsed, name? }`
**WsSourceFile**: `{ path, kind, localImports, libraries, exports, importedBy, description, lineCount, lastModified, content, imagePath, widgetW, widgetH }`
**WsClaudeEvent**: `{ kind, tool, content, timestamp, sessionId, inputTokens?, outputTokens? }`
**WsIdeState**: `{ openFiles, activeFile, cursors, selections, visibleRanges, sidebarVisible, panelVisible, workspaceFolder }`

### WsServer — 35+ message types (server → browser)
State snapshots on connect: `Users`, `Shells`, `Notes`, `TextBlocks`, `Drawings`, `Slides`, `Widgets`, `SourceFiles`, `ComponentGraph`, `ClaudeEvent` (×200 replay), `VideoStreams`, `IdeStates`, `EditLock`, `IceServers`, `IdeAvailable`.

Live diffs: `UserDiff`, `NoteDiff`, `TextBlockDiff`, `DrawingDiff`, `SlideDiff`, `WidgetDiff`, `VideoStreamDiff`, `IdeStateDiff`.

Terminal: `Chunks`, `ShellLatency`, `Hear` (chat).

WebRTC: `RtcOffer`, `RtcAnswer`, `RtcIce`, `BrowserControlStatus`, `BrowserFrame`.

### WsClient — 40+ message types (browser → server)
Auth: `Authenticate`. User: `SetName`, `SetCursor`, `SetFocus`. Shells: `Create`, `Close`, `Move`, `Data`, `Subscribe`. Notes/Text/Drawings/Slides: CRUD operations. Widgets: `Open*`, `Move`, `Resize`, `Close`, `SetCollapsed`, `SetName`. Video: `StartScreenShare`, `StopScreenShare`, `WatchStream`, WebRTC signaling. Browser: `BrowserInput`, `RequestBrowserControl`. IDE: `UpdateIdeState`, `RequestEditLock`. Misc: `Chat`, `Ping`.

---

## Encryption (`src/lib/encrypt.ts`)

```typescript
class Encrypt {
  static async new(key: string): Promise<Encrypt>  // Argon2id → AES key
  async zeros(): Promise<Uint8Array>                // Auth proof (16 encrypted zeros)
  async segment(stream: bigint, offset: bigint, data: Uint8Array): Promise<Uint8Array>
}
```

- **KDF**: Argon2id (19×1024 KiB memory, 2 iterations, 1 thread)
- **Cipher**: AES-128-CTR via Web Crypto API
- **Key source**: URL fragment `#<14-char-key>[,<write-password>]`

---

## Reconnecting WebSocket (`src/lib/srocket.ts`)

```typescript
class Srocket<T, U> {
  constructor(url: string, options: SrocketOptions<T>)
  send(message: U): void
  dispose(): void
  get connected(): boolean
}
```

- Binary CBOR-X encoding
- Auto-reconnect after 500ms
- Buffers up to 64 messages while disconnected
- Close codes: `4404` = session not found, `4500` = server error

---

## Key Svelte Stores

| Store | Type | Location | Purpose |
|-------|------|----------|---------|
| `settings` | `Readable<Settings>` | `settings.ts` | User prefs (name, theme, scrollback) |
| `storedSettings` | `Persisted<Settings>` | `settings.ts` | localStorage backing store |
| `toastStore` | `Writable<Toast[]>` | `toast.ts` | Active toast notifications |

---

## Dependencies (Key Packages)

| Package | Purpose |
|---------|---------|
| `svelte` + `@sveltejs/kit` | Framework + metaframework |
| `sshx-xterm` | Custom xterm.js fork |
| `xterm-addon-webgl` / `web-links` / `image` | Terminal addons |
| `cbor-x` | Binary WebSocket serialization |
| `argon2-browser` | Client-side Argon2id KDF |
| `tailwindcss` | Utility CSS |
| `monaco-editor` | Code editor for IDE widget |
| `@tiptap/core` + `starter-kit` | Rich text editor (sticky notes) |
| `@use-gesture/vanilla` | Touch/mouse gestures |
| `perfect-cursors` | Smooth cursor interpolation |
| `marked` | Markdown rendering |
| `carta-md` | Markdown viewer (file cards) |
| `html2canvas` + `jspdf` | PDF export |
| `fontfaceobserver` | Font loading detection |

---

## Build & Config

### `vite.config.ts`
- Version: `__APP_VERSION__` = `0.5.0-{git-hash}`
- Dev server port: `5173` with HTTPS
- Proxy: `/api`, `/uploads`, `/ide` → `http://127.0.0.1:8051`
- Optimized deps: Tiptap bundle

### `svelte.config.js`
- Static adapter with SPA fallback (`spa.html`)
- Precompressed output (gzip + brotli)
- PostCSS preprocessing (Tailwind)

### `src/app.css`
- Fira Code VF variable font (woff2)
- `.panel` utility: `border-zinc-800 bg-zinc-900/90 backdrop-blur-sm rounded-xl`
- `.canvas-focus-ring`: `ring-2 ring-indigo-400 rounded-lg`
- Dark color scheme

---

## Tests (`tests/e2e/`)

| File | Purpose |
|------|---------|
| `screenshots.spec.ts` | Screenshot capture for all UI components |
| `p2p-screen-share.spec.ts` | P2P screen sharing protocol |
| `server-stream.spec.ts` | Server-to-browser streaming |
| `text-block.spec.ts` | Text block creation/editing |
| `helpers/grpc-client.ts` | gRPC client for test session creation |
| `helpers/encrypt.ts` | Encryption helper |
| `helpers/color-server.ts` | Simple test server |
| `helpers/generate-vp8-frames.ts` | VP8 frame generation |

Framework: Playwright (headless Chromium, baseURL `http://[::1]:18051`).

---

## Window Chrome Pattern

All floating elements share macOS-style title bars:
```
┌─────────────────────────────────────────┐
│  ● ● ●  │   centered title   │  (info)  │
│ circle  │   text-sm zinc-300  │  flex-1  │
│ buttons │   flex-grow-[4]     │  spacer  │
└─────────────────────────────────────────┘
```

Used in: `XTerm`, `FileCard`, `FileTreePanel`, `ImageWidget`, `IdeEditorWidget`.
Exception: `StickyNote` uses colored post-it aesthetic.

### Color Palette
- Backgrounds: `zinc-900`, `zinc-800`
- Borders: `zinc-700`, `zinc-800`
- Text: `zinc-300`
- Status: `emerald-400` (ok), `red-400` (error), `indigo-400` (AI), `yellow-300` (tools)
- Panel opacity: `opacity-90` idle, `opacity-100` focused
