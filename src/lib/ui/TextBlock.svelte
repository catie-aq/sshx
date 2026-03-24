<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import type { WsTextBlock } from "$lib/protocol";

  export let block: WsTextBlock;
  export let canWrite: boolean;

  const dispatch = createEventDispatcher<{
    update: WsTextBlock;
    delete: void;
    startMove: MouseEvent;
  }>();

  const FONT_SIZES: Record<string, string> = {
    xs: "12px",
    sm: "16px",
    md: "24px",
    lg: "36px",
    xl: "48px",
  };

  const PALETTE = [
    "#ffffff",
    "#a1a1aa",
    "#fde047",
    "#f87171",
    "#4ade80",
    "#60a5fa",
    "#c084fc",
    "#fb923c",
  ];

  let editing = false;
  let hovered = false;
  let contentEl: HTMLDivElement;
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let autoFocus = false;

  export function focusAfterCreate() {
    autoFocus = true;
  }

  onMount(() => {
    if (autoFocus && contentEl && canWrite) {
      editing = true;
      requestAnimationFrame(() => contentEl?.focus());
    }
  });

  $: {
    // Sync content from server when not editing.
    if (contentEl && !editing && contentEl.innerHTML !== block.content) {
      contentEl.innerHTML = block.content || "";
    }
  }

  function startEditing() {
    if (!canWrite) return;
    editing = true;
    requestAnimationFrame(() => contentEl?.focus());
  }

  function flushUpdate() {
    if (debounceTimer !== null) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    const content = contentEl?.innerHTML ?? "";
    dispatch("update", { ...block, content });
  }

  function handleBlur() {
    editing = false;
    flushUpdate();
  }

  function handleInput() {
    if (debounceTimer !== null) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(flushUpdate, 500);
  }

  function handleMousedown(event: MouseEvent) {
    if (editing) return; // let text cursor work
    if (!canWrite) return;
    event.preventDefault();
    dispatch("startMove", event);
  }

  function setFontSize(size: string) {
    dispatch("update", { ...block, fontSize: size });
  }

  function setColor(color: string) {
    dispatch("update", { ...block, color });
  }

  function setAlign(align: string) {
    dispatch("update", { ...block, align });
  }

  function toggleBold() {
    document.execCommand("bold");
    handleInput();
  }

  function toggleList() {
    document.execCommand("insertUnorderedList");
    handleInput();
  }

  function insertLink() {
    const url = prompt("Enter URL:");
    if (url) {
      document.execCommand("createLink", false, url);
      handleInput();
    }
  }

  $: fontSize = FONT_SIZES[block.fontSize] ?? FONT_SIZES.md;
  $: isEmpty = !block.content && !editing;
</script>

<!-- svelte-ignore a11y-no-static-element-interactions -->
<div
  class="text-block-wrapper"
  on:pointerdown={(e) => e.stopPropagation()}
  on:mouseenter={() => (hovered = true)}
  on:mouseleave={() => (hovered = false)}
>
  <!-- Floating toolbar -->
  {#if (hovered || editing) && canWrite}
    <div class="toolbar">
      <!-- Font size -->
      <select
        class="toolbar-select"
        value={block.fontSize}
        on:change={(e) => setFontSize(e.currentTarget.value)}
      >
        <option value="xs">XS</option>
        <option value="sm">SM</option>
        <option value="md">MD</option>
        <option value="lg">LG</option>
        <option value="xl">XL</option>
      </select>

      <div class="toolbar-sep" />

      <!-- Bold -->
      <button class="toolbar-btn" title="Bold" on:click={toggleBold}>
        <strong>B</strong>
      </button>

      <!-- Link -->
      <button class="toolbar-btn" title="Insert link" on:click={insertLink}>
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" />
          <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />
        </svg>
      </button>

      <!-- List -->
      <button class="toolbar-btn" title="Bullet list" on:click={toggleList}>
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <line x1="8" y1="6" x2="21" y2="6" />
          <line x1="8" y1="12" x2="21" y2="12" />
          <line x1="8" y1="18" x2="21" y2="18" />
          <line x1="3" y1="6" x2="3.01" y2="6" />
          <line x1="3" y1="12" x2="3.01" y2="12" />
          <line x1="3" y1="18" x2="3.01" y2="18" />
        </svg>
      </button>

      <div class="toolbar-sep" />

      <!-- Color palette -->
      {#each PALETTE as c}
        <button
          class="color-dot"
          class:ring-1={block.color === c}
          class:ring-white={block.color === c}
          style:background-color={c}
          title="Color: {c}"
          on:click={() => setColor(c)}
        />
      {/each}

      <div class="toolbar-sep" />

      <!-- Alignment -->
      <button
        class="toolbar-btn"
        class:active={block.align === "left"}
        title="Align left"
        on:click={() => setAlign("left")}
      >L</button>
      <button
        class="toolbar-btn"
        class:active={block.align === "center"}
        title="Align center"
        on:click={() => setAlign("center")}
      >C</button>
      <button
        class="toolbar-btn"
        class:active={block.align === "right"}
        title="Align right"
        on:click={() => setAlign("right")}
      >R</button>

      <div class="toolbar-sep" />

      <!-- Delete -->
      <button
        class="toolbar-btn delete-btn"
        title="Delete text block"
        on:click={() => dispatch("delete")}
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <line x1="18" y1="6" x2="6" y2="18" />
          <line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>
    </div>
  {/if}

  <!-- Text content -->
  <!-- svelte-ignore a11y-click-events-have-key-events -->
  <div
    bind:this={contentEl}
    class="text-content"
    class:editing
    class:empty={isEmpty}
    style:font-size={fontSize}
    style:color={block.color}
    style:text-align={block.align}
    contenteditable={editing}
    on:mousedown={handleMousedown}
    on:dblclick={startEditing}
    on:input={handleInput}
    on:blur={handleBlur}
    role="textbox"
    tabindex="0"
  >
    {#if isEmpty}
      <span class="placeholder">Type here...</span>
    {/if}
  </div>
</div>

<style lang="postcss">
  .text-block-wrapper {
    position: relative;
    min-width: 60px;
  }

  .toolbar {
    @apply absolute flex items-center gap-1 px-2 py-1 bg-zinc-800 rounded-lg border border-zinc-700 shadow-lg;
    bottom: calc(100% + 8px);
    left: 50%;
    transform: translateX(-50%);
    white-space: nowrap;
    z-index: 10;
    pointer-events: auto;
  }

  .toolbar-select {
    @apply bg-zinc-700 text-zinc-200 text-xs rounded px-1 py-0.5 border border-zinc-600 focus:outline-none;
    cursor: pointer;
  }

  .toolbar-btn {
    @apply text-zinc-300 hover:text-white hover:bg-zinc-700 rounded px-1.5 py-0.5 text-xs transition-colors;
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

  .text-content {
    @apply outline-none leading-snug;
    cursor: grab;
    min-height: 1.2em;
    word-break: break-word;
  }

  .text-content.editing {
    cursor: text;
  }

  .text-content :global(a) {
    text-decoration: underline;
    cursor: pointer;
  }

  .text-content :global(ul) {
    list-style-type: disc;
    padding-left: 1.2em;
  }

  .text-content :global(ol) {
    list-style-type: decimal;
    padding-left: 1.2em;
  }

  .placeholder {
    @apply text-zinc-500 italic pointer-events-none select-none;
  }
</style>
