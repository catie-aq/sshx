# src/lib/ui/ — UI Components

This directory contains all presentational and interactive UI components used in the sshx frontend.

## Component Catalog

### Terminal

#### `XTerm.svelte` ★
The core terminal widget. Wraps **xterm.js** (using the `sshx-xterm` fork).

**Props:**
```typescript
export let sid: Sid;              // Shell ID
export let winsize: WsWinsize;    // Position + size (x, y, rows, cols)
export let focused: boolean;      // Highlight border when focused
export let canWrite: boolean;     // Enable/disable input
```

**Events:**
```typescript
on:data       // User typed something → (Uint8Array)
on:close      // User clicked close button → ()
on:resize     // Terminal resized → ({ rows, cols })
on:focus      // Terminal clicked/focused → ()
on:drag       // Window drag started → ()
```

**Responsibilities:**
- Creates and manages xterm.js `Terminal` instance
- Loads **Fira Code** variable font (deduplicated across multiple instances)
- Applies color theme from `settings.theme`
- Attaches `TypeAheadAddon` for local echo prediction
- Handles keyboard shortcuts (Ctrl/Cmd-based)
- Exposes a `write(data: Uint8Array)` method (called by `Session.svelte` on decrypted chunks)
- Fires resize events when the wrapper div is resized (ResizeObserver)

**Note:** Font loading is async and deduplicated — subsequent XTerm instances wait for the first load to complete.

---

### User Presence

#### `LiveCursor.svelte`
Renders a single user's live cursor on the canvas.

**Props:** `uid: Uid`, `user: WsUser`, `transform: string` (CSS canvas transform)

- Cursor color derived from `uid` via FNV hash → HSL hue
- Shows user's name on hover
- Auto-hides after **1.5 seconds** of no movement
- Uses `perfect-cursors` for smooth interpolation between position updates

#### `Avatars.svelte`
Displays stacked avatar circles in the top-right corner (one per connected user).

**Props:** `users: Map<Uid, WsUser>`, `myUid: Uid`

#### `NameList.svelte`
A list of all connected user names. Used in the overlay menu.

**Props:** `users: Map<Uid, WsUser>`

#### `ChooseName.svelte`
Modal dialog for setting your display name before joining as a named participant.

**Events:** `on:submit (name: string)`

---

### Chat

#### `Chat.svelte`
Full chat panel — message history + input field.

**Props:** `messages: ChatMessage[]`, `users: Map<Uid, WsUser>`, `myUid: Uid`

**Events:** `on:send (text: string)`

- Groups consecutive messages from the same sender
- Auto-scrolls to the latest message
- Submit with **Enter** key (Shift+Enter for newline)
- Displays sender's colored avatar dot

---

### Navigation & Menus

#### `Toolbar.svelte`
Top navigation bar with action buttons.

**Props:** `canWrite: boolean`, `shells: Map<Sid, WsWinsize>`

**Events:** `on:newTerminal`, `on:openChat`, `on:openSettings`, `on:openMenu`

Buttons: Add Terminal (+), Chat, Settings, Info/Menu

#### `OverlayMenu.svelte`
Slide-in menu panel (for NetworkInfo, NameList, etc.).

**Props:** `open: boolean`

**Events:** `on:close`

---

### Settings & Info

#### `Settings.svelte`
User preferences panel.

**Props:** (reads/writes `settings` store directly)

Controls:
- Display name input
- Theme selector (maps to `themes.ts` entries)
- Scrollback buffer size

#### `NetworkInfo.svelte`
Displays connection latency statistics.

**Props:** `serverLatencies: number[]`, `shellLatencies: number[]`

Shows: average ping, shell latency, connection status indicator.

---

### Reusable Primitives

#### `CircleButton.svelte`
A single circular icon button.

**Props:** `icon: SvelteComponent`, `label: string`, `active?: boolean`

**Events:** `on:click`

#### `CircleButtons.svelte`
A group of `CircleButton` components with consistent spacing.

#### `CopyableCode.svelte`
Displays a code string with a copy-to-clipboard button.

**Props:** `code: string`, `lang?: string`

#### `DownloadLink.svelte`
A styled download link for a platform binary.

**Props:** `platform: string`, `url: string`, `label: string`

#### `TeaserVideo.svelte`
Lazy-loaded embedded demo video for the landing page.

**Props:** `src: string`

---

### Notifications

#### `Toast.svelte`
A single toast notification (auto-dismisses after timeout).

**Props:** `message: string`, `type: 'success' | 'error' | 'info'`

#### `ToastContainer.svelte`
Mounts at the root layout. Subscribes to the `toast` store from `toast.ts` and renders active toasts.

No props — reads from global store.

---

### `themes.ts`

```typescript
export const themes: Record<string, Theme> = { ... }
export type Theme = { background, foreground, cursor, ... }
```

Defines color themes for xterm.js. Each theme is a map of terminal color names to hex values. Keys are used by `settings.ts` for the theme selector.

Current themes: `dark` (default, zinc-based), and potentially others.

---

## Component Interaction Map

```
Session.svelte
├── XTerm.svelte           (one per shell)
│   └── [typeahead addon]
├── LiveCursor.svelte      (one per user)
├── Chat.svelte
├── Toolbar.svelte
├── OverlayMenu.svelte
│   ├── NameList.svelte
│   └── NetworkInfo.svelte
├── Settings.svelte
├── Avatars.svelte
└── ChooseName.svelte      (shown before first SetName)

routes/+layout.svelte
└── ToastContainer.svelte
```

---

## Styling Notes

- All components use **Tailwind CSS** utility classes
- Dark theme only: `zinc-800/900` backgrounds, `zinc-100/200` text
- Borders: `zinc-700` subtle borders
- Focus/active states: `ring-2 ring-blue-500`
- Scrollbars: custom styled via Tailwind plugin or thin variant
- No external component library — all custom-built

## Adding a New Component

1. Create `src/lib/ui/MyComponent.svelte`
2. Follow existing patterns: Tailwind classes, typed props, typed `createEventDispatcher`
3. Import and use in `Session.svelte` or appropriate parent
4. If it shows a notification, use `toast.ts` helpers instead of custom state

## See Also

- `src/lib/README.md` — core logic overview
- `src/README.md` — frontend overview
- Root `CLAUDE.md` — full project guide
