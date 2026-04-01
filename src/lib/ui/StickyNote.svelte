<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from "svelte";
  import { MapPinIcon, XIcon } from "svelte-feather-icons";
  import type { WsNote } from "$lib/protocol";
  import { FONTS, getFontFamily, DEFAULT_FONT } from "$lib/fonts";
  import { Editor } from "@tiptap/core";
  import StarterKit from "@tiptap/starter-kit";
  import Link from "@tiptap/extension-link";

  export let note: WsNote;
  export let canWrite: boolean;

  const dispatch = createEventDispatcher<{
    update: WsNote;
    delete: void;
    startMove: MouseEvent;
    startResize: MouseEvent;
  }>();

  const COLORS: Record<string, { bg: string; text: string }> = {
    yellow: { bg: "#fef9c3", text: "#78350f" },
    pink:   { bg: "#fce7f3", text: "#9d174d" },
    blue:   { bg: "#dbeafe", text: "#1e3a8a" },
    green:  { bg: "#dcfce7", text: "#14532d" },
    purple: { bg: "#f3e8ff", text: "#581c87" },
  };

  // Swatch display colors (slightly more saturated for visibility in the dark toolbar)
  const SWATCH_COLORS: Record<string, string> = {
    yellow: "#fde047",
    pink:   "#f9a8d4",
    blue:   "#93c5fd",
    green:  "#86efac",
    purple: "#d8b4fe",
  };

  $: colors = COLORS[note.color] ?? COLORS.yellow;
  $: noteW = note.w || 260;
  $: noteH = note.h || 0;
  $: fontFamily = getFontFamily(note.font || DEFAULT_FONT);

  let selected = false;
  let editing = false;
  let hovered = false;
  let editorEl: HTMLDivElement;
  let editor: Editor | null = null;
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let fontDropdownOpen = false;

  onMount(() => {
    editor = new Editor({
      element: editorEl,
      extensions: [
        StarterKit,
        Link.configure({ openOnClick: false }),
      ],
      content: note.text || "",
      editable: false,
      onUpdate: () => debouncedUpdate(),
    });

    document.addEventListener("mousedown", handleDocClick);
    document.addEventListener("keydown", handleKeydown);
  });

  onDestroy(() => {
    editor?.destroy();
    document.removeEventListener("mousedown", handleDocClick);
    document.removeEventListener("keydown", handleKeydown);
  });

  // Sync from server when not editing
  $: if (editor && !editing && note.text !== editor.getHTML()) {
    editor.commands.setContent(note.text || "");
  }

  $: if (editor) editor.setEditable(editing);

  let wrapperEl: HTMLDivElement;

  function handleDocClick(e: MouseEvent) {
    if (wrapperEl && !wrapperEl.contains(e.target as Node)) {
      if (editing) {
        editing = false;
        flushUpdate();
      }
      selected = false;
      fontDropdownOpen = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    if (editing) {
      e.stopPropagation();
      editing = false;
      flushUpdate();
      editor?.commands.blur();
    } else if (selected) {
      e.stopPropagation();
      selected = false;
    }
  }

  function startEditing() {
    if (!canWrite) return;
    editing = true;
    requestAnimationFrame(() => editor?.commands.focus());
  }

  function flushUpdate() {
    if (debounceTimer !== null) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    const text = editor?.getHTML() ?? "";
    dispatch("update", { ...note, text });
  }

  function debouncedUpdate() {
    if (debounceTimer !== null) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(flushUpdate, 500);
  }

  function setColor(color: string) {
    dispatch("update", { ...note, color });
  }

  function setFont(fontId: string) {
    fontDropdownOpen = false;
    dispatch("update", { ...note, font: fontId });
  }

  function togglePin() {
    dispatch("update", { ...note, pinned: !note.pinned });
  }

  function handleBodyMousedown(event: MouseEvent) {
    if (editing) return; // let text cursor work
    if (selected && !editing && canWrite && !note.pinned) {
      event.preventDefault();
      dispatch("startMove", event);
    }
  }

  function handleBodyClick(event: MouseEvent) {
    if (editing) return; // let text cursor work normally
    if (!selected) {
      selected = true;
      event.preventDefault();
    } else {
      startEditing();
    }
  }

  function handleResizeMousedown(event: MouseEvent) {
    if (!canWrite || note.pinned) return;
    event.preventDefault();
    dispatch("startResize", event);
  }
</script>

<!-- svelte-ignore a11y-no-static-element-interactions -->
<div
  bind:this={wrapperEl}
  class="sticky-note flex flex-col rounded-lg select-none"
  style:width="{noteW}px"
  style:min-height={noteH ? `${noteH}px` : "120px"}
  style:background-color={colors.bg}
  on:pointerdown={(e) => e.stopPropagation()}
  on:mouseenter={() => (hovered = true)}
  on:mouseleave={() => { hovered = false; fontDropdownOpen = false; }}
>
  <!-- Floating toolbar: appears when selected or editing -->
  {#if (selected || editing) && canWrite}
    <div class="toolbar">
      <!-- Color swatches -->
      {#each Object.keys(COLORS) as colorKey}
        <button
          class="color-dot"
          class:ring-1={note.color === colorKey}
          class:ring-white={note.color === colorKey}
          style:background-color={SWATCH_COLORS[colorKey]}
          on:click|stopPropagation={() => setColor(colorKey)}
        />
      {/each}

      <div class="toolbar-sep" />

      <!-- Font selector -->
      <div class="relative" on:mousedown|stopPropagation>
        <button
          class="toolbar-btn font-picker-btn"
          style:font-family={fontFamily}
          title="Font"
          on:click={() => (fontDropdownOpen = !fontDropdownOpen)}
        >Aa</button>
        {#if fontDropdownOpen}
          <div class="font-dropdown">
            {#each FONTS as f}
              <button
                class="font-dropdown-item"
                class:active={(note.font || DEFAULT_FONT) === f.id}
                style:font-family={f.family}
                on:click={() => setFont(f.id)}
              >{f.label}</button>
            {/each}
          </div>
        {/if}
      </div>

      <div class="toolbar-sep" />

      <!-- Pin -->
      <button
        class="toolbar-btn"
        class:active={note.pinned}
        title={note.pinned ? "Unpin" : "Pin"}
        on:click|stopPropagation={togglePin}
      >
        <MapPinIcon size="12" strokeWidth={note.pinned ? 2.5 : 1.5} />
      </button>

      <!-- Delete -->
      <button
        class="toolbar-btn delete-btn"
        title="Delete note"
        on:click|stopPropagation={() => dispatch("delete")}
      >
        <XIcon size="12" strokeWidth={2} />
      </button>
    </div>
  {/if}

  <!-- Body: TipTap rich text editor (entire note is the body) -->
  <!-- svelte-ignore a11y-click-events-have-key-events -->
  <div
    bind:this={editorEl}
    class="note-body flex-1 p-3 overflow-y-auto"
    class:editing
    class:draggable={selected && !editing && canWrite && !note.pinned}
    style:color={colors.text}
    style:font-family={fontFamily}
    style:min-height={noteH ? `${noteH}px` : "120px"}
    on:mousedown={handleBodyMousedown}
    on:click={handleBodyClick}
  />

  <!-- Resize handle (bottom-right corner) -->
  {#if canWrite && !note.pinned}
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div
      class="resize-handle"
      on:mousedown={handleResizeMousedown}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" fill="none" class="opacity-40">
        <line x1="9" y1="1" x2="1" y2="9" stroke="currentColor" stroke-width="1"/>
        <line x1="9" y1="4" x2="4" y2="9" stroke="currentColor" stroke-width="1"/>
        <line x1="9" y1="7" x2="7" y2="9" stroke="currentColor" stroke-width="1"/>
      </svg>
    </div>
  {/if}
</div>

<style lang="postcss">
  .sticky-note {
    position: relative;
    filter: drop-shadow(0 2px 8px rgba(0, 0, 0, 0.18));
    overflow: visible;
  }

  /* Floating toolbar above the note */
  .toolbar {
    @apply absolute flex items-center gap-1 px-2 py-1 bg-zinc-800 rounded-lg border border-zinc-700 shadow-lg;
    bottom: calc(100% + 8px);
    left: 50%;
    transform: translateX(-50%);
    white-space: nowrap;
    z-index: 10;
    pointer-events: auto;
  }

  .toolbar-btn {
    @apply text-zinc-300 hover:text-white hover:bg-zinc-700 rounded px-1.5 py-0.5 text-xs transition-colors flex items-center;
  }

  .toolbar-btn.active {
    @apply bg-zinc-600 text-white;
  }

  .delete-btn {
    @apply text-red-400 hover:text-red-300 hover:bg-red-900/50;
  }

  .toolbar-sep {
    @apply w-px h-4 bg-zinc-600 mx-0.5;
  }

  .color-dot {
    @apply w-3.5 h-3.5 rounded-full border border-zinc-500 hover:scale-125 transition-transform cursor-pointer;
  }

  .font-picker-btn {
    @apply text-xs px-1.5;
    min-width: 24px;
  }

  .font-dropdown {
    @apply absolute bg-zinc-800 border border-zinc-700 rounded-lg shadow-xl overflow-hidden;
    top: calc(100% + 4px);
    left: 0;
    z-index: 20;
    min-width: 130px;
  }

  .font-dropdown-item {
    @apply w-full px-2 py-1 text-left text-sm text-zinc-300 hover:bg-zinc-700 transition-colors;
  }

  .font-dropdown-item.active {
    @apply bg-zinc-600 text-white;
  }

  .note-body {
    cursor: text;
    font-size: 14px;
    line-height: 1.5;
    border-radius: 0.5rem;
  }

  .note-body.draggable {
    cursor: grab;
  }

  .note-body.draggable:active {
    cursor: grabbing;
  }

  .note-body :global(.tiptap) {
    outline: none;
    min-height: 1.2em;
  }

  .note-body :global(.tiptap p) {
    margin: 0 0 0.3em;
  }

  .note-body :global(.tiptap p:last-child) {
    margin-bottom: 0;
  }

  .note-body :global(.tiptap p.is-editor-empty:first-child::before) {
    content: "Write something...";
    opacity: 0.4;
    pointer-events: none;
    float: left;
    height: 0;
    font-style: italic;
  }

  .note-body :global(strong) { font-weight: 700; }
  .note-body :global(em) { font-style: italic; }

  .note-body :global(ul) {
    list-style-type: disc;
    padding-left: 1.2em;
    margin-bottom: 0.3em;
  }

  .note-body :global(ol) {
    list-style-type: decimal;
    padding-left: 1.2em;
    margin-bottom: 0.3em;
  }

  .note-body :global(li) {
    margin-bottom: 0.1em;
  }

  .note-body :global(code) {
    font-family: "Fira Code", monospace;
    font-size: 0.85em;
    background: rgba(0, 0, 0, 0.06);
    border-radius: 3px;
    padding: 0.1em 0.3em;
  }

  .note-body :global(blockquote) {
    border-left: 3px solid rgba(0, 0, 0, 0.15);
    padding-left: 0.6em;
    margin: 0.3em 0;
    opacity: 0.8;
  }

  .note-body :global(a) {
    text-decoration: underline;
    opacity: 0.85;
  }

  .note-body :global(h1) { font-size: 1.3em; font-weight: 700; margin-bottom: 0.3em; }
  .note-body :global(h2) { font-size: 1.1em; font-weight: 700; margin-bottom: 0.25em; }
  .note-body :global(h3) { font-size: 1em; font-weight: 600; margin-bottom: 0.2em; }

  .resize-handle {
    position: absolute;
    bottom: 2px;
    right: 2px;
    cursor: nwse-resize;
    padding: 2px;
  }
</style>
