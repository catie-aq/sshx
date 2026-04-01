<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from "svelte";
  import type { WsTextBlock } from "$lib/protocol";
  import { Editor } from "@tiptap/core";
  import StarterKit from "@tiptap/starter-kit";
  import Link from "@tiptap/extension-link";
  import { FONTS, getFontFamily, DEFAULT_FONT } from "$lib/fonts";

  export let block: WsTextBlock;
  export let canWrite: boolean;
  export let autoFocus: boolean = false;

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

  // Three states: idle → selected → editing
  let selected = false;
  let editing = false;
  let hovered = false;
  let editorEl: HTMLDivElement;
  let wrapperEl: HTMLDivElement;
  let editor: Editor | null = null;
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  // Toolbar active state
  let isBold = false;
  let isList = false;
  let isLink = false;

  // Link dialog state
  let showLinkDialog = false;
  let linkUrl = "";
  let linkInputEl: HTMLInputElement;

  onMount(() => {
    editor = new Editor({
      element: editorEl,
      extensions: [
        StarterKit,
        Link.configure({ openOnClick: false }),
      ],
      content: block.content || "",
      editable: false,
      onTransaction: () => {
        // Force Svelte reactivity
        editor = editor;
        isBold = editor!.isActive("bold");
        isList = editor!.isActive("bulletList");
        isLink = editor!.isActive("link");
      },
      onUpdate: () => handleInput(),
    });

    if (autoFocus && canWrite) {
      selected = true;
      editing = true;
      editor.setEditable(true);
      requestAnimationFrame(() => editor?.commands.focus());
    }

    document.addEventListener("mousedown", handleDocumentClick);
    document.addEventListener("keydown", handleKeydown);
  });

  onDestroy(() => {
    editor?.destroy();
    document.removeEventListener("mousedown", handleDocumentClick);
    document.removeEventListener("keydown", handleKeydown);
  });

  // Sync content from server when not editing
  $: if (editor && !editing && block.content !== editor.getHTML()) {
    editor.commands.setContent(block.content || "");
  }

  // Toggle editable based on editing state
  $: if (editor) editor.setEditable(editing);

  // Click outside → deselect
  function handleDocumentClick(e: MouseEvent) {
    if (!wrapperEl) return;
    if (!wrapperEl.contains(e.target as Node)) {
      if (editing) {
        editing = false;
        flushUpdate();
      }
      selected = false;
      showLinkDialog = false;
    }
  }

  // Escape: editing → selected, selected → idle
  function handleKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    if (showLinkDialog) {
      e.stopPropagation();
      cancelLink();
    } else if (editing) {
      e.stopPropagation();
      editing = false;
      flushUpdate();
      editor?.commands.blur();
      // Stay selected
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
    const content = editor?.getHTML() ?? "";
    localContent = content;
    dispatch("update", { ...block, content });
  }

  function handleInput() {
    if (debounceTimer !== null) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(flushUpdate, 500);
  }

  // Click on the text content area
  function handleContentClick(event: MouseEvent) {
    if (editing) return; // let text cursor work normally

    if (!selected) {
      // First click: select
      selected = true;
      event.preventDefault();
    } else {
      // Second click (already selected): enter editing
      startEditing();
    }
  }

  // Mousedown for drag-move (only when selected but not editing)
  function handleMousedown(event: MouseEvent) {
    if (editing) return; // let text cursor work
    if (!canWrite) return;
    if (!selected) return; // must be selected first to drag
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
    editor?.chain().focus().toggleBold().run();
    handleInput();
  }

  function toggleList() {
    editor?.chain().focus().toggleBulletList().run();
    handleInput();
  }

  function openLinkDialog() {
    const attrs = editor?.getAttributes("link");
    linkUrl = attrs?.href ?? "";
    showLinkDialog = true;
    requestAnimationFrame(() => linkInputEl?.focus());
  }

  function confirmLink() {
    if (linkUrl.trim()) {
      editor?.chain().focus().setLink({ href: linkUrl.trim() }).run();
    } else {
      editor?.chain().focus().unsetLink().run();
    }
    showLinkDialog = false;
    linkUrl = "";
    handleInput();
  }

  function cancelLink() {
    showLinkDialog = false;
    linkUrl = "";
    editor?.commands.focus();
  }

  // Track local content so isEmpty stays correct between editing and server roundtrip.
  let localContent = block.content;
  $: localContent = block.content || localContent;

  $: fontSize = FONT_SIZES[block.fontSize] ?? FONT_SIZES.md;
  $: fontFamily = getFontFamily(block.font || DEFAULT_FONT);
  $: isEmpty = !localContent && !editing;

  let fontDropdownOpen = false;

  function setFont(fontId: string) {
    fontDropdownOpen = false;
    dispatch("update", { ...block, font: fontId });
  }
</script>

<!-- svelte-ignore a11y-no-static-element-interactions -->
<div
  bind:this={wrapperEl}
  class="text-block-wrapper"
  class:selected
  class:editing
  on:pointerdown={(e) => e.stopPropagation()}
  on:mouseenter={() => (hovered = true)}
  on:mouseleave={() => (hovered = false)}
>
  <!-- Floating toolbar: show when selected or editing -->
  {#if (selected || editing) && canWrite}
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

      <!-- Font family -->
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
                class:active={(block.font || DEFAULT_FONT) === f.id}
                style:font-family={f.family}
                on:click={() => setFont(f.id)}
              >{f.label}</button>
            {/each}
          </div>
        {/if}
      </div>

      <div class="toolbar-sep" />

      <!-- Bold -->
      <button class="toolbar-btn" class:active={isBold} title="Bold" on:click={toggleBold}>
        <strong>B</strong>
      </button>

      <!-- Link -->
      <button class="toolbar-btn" class:active={isLink} title="Insert link" on:click={openLinkDialog}>
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
      <button class="toolbar-btn" class:active={isList} title="Bullet list" on:click={toggleList}>
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

    <!-- Link dialog -->
    {#if showLinkDialog}
      <!-- svelte-ignore a11y-no-static-element-interactions -->
      <div class="link-dialog" on:mousedown|stopPropagation>
        <input
          bind:this={linkInputEl}
          bind:value={linkUrl}
          type="url"
          placeholder="https://..."
          on:keydown={(e) => {
            if (e.key === "Enter") { e.preventDefault(); confirmLink(); }
            if (e.key === "Escape") { e.preventDefault(); cancelLink(); }
          }}
        />
        <button class="toolbar-btn" on:click={cancelLink}>✕</button>
        <button class="toolbar-btn active" on:click={confirmLink}>↵</button>
      </div>
    {/if}
  {/if}

  <!-- TipTap mount point -->
  <!-- svelte-ignore a11y-click-events-have-key-events -->
  <div
    bind:this={editorEl}
    class="text-content"
    class:selected={selected && !editing}
    class:editing
    class:empty={isEmpty}
    style:font-size={fontSize}
    style:font-family={fontFamily}
    style:color={block.color}
    style:text-align={block.align}
    on:mousedown={handleMousedown}
    on:click={handleContentClick}
  />
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

  .link-dialog {
    @apply absolute flex items-center gap-1 px-2 py-1 bg-zinc-800 rounded-lg border border-zinc-700 shadow-lg;
    top: calc(100% + 8px);
    left: 50%;
    transform: translateX(-50%);
    z-index: 10;
    white-space: nowrap;
  }

  .link-dialog input {
    @apply bg-zinc-700 text-zinc-200 text-xs rounded px-2 py-0.5 border border-zinc-600 focus:outline-none focus:border-indigo-500;
    width: 200px;
  }

  .text-content {
    @apply outline-none leading-snug;
    cursor: default;
    min-height: 1.2em;
    word-break: break-word;
    padding: 4px 6px;
    border: 2px solid transparent;
    border-radius: 4px;
    transition: border-color 0.15s ease;
  }

  .text-content.selected {
    border-color: #60a5fa;
    cursor: grab;
  }

  .text-content.editing {
    border-color: #818cf8;
    cursor: text;
  }

  .text-content :global(.tiptap) {
    outline: none;
    min-height: 1.2em;
  }

  .text-content :global(.tiptap p) {
    margin: 0;
  }

  .text-content :global(.tiptap p.is-editor-empty:first-child::before) {
    content: "Type here...";
    @apply text-zinc-500 italic;
    pointer-events: none;
    float: left;
    height: 0;
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
</style>
