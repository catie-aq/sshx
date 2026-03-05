<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { MapPinIcon, XIcon, EditIcon, EyeIcon } from "svelte-feather-icons";
  import type { WsNote } from "$lib/protocol";
  import { marked } from "marked";

  export let note: WsNote;
  export let canWrite: boolean;

  const dispatch = createEventDispatcher<{
    update: WsNote;
    delete: void;
    startMove: MouseEvent;
  }>();

  const COLORS: Record<string, string> = {
    yellow: "#fef08a",
    pink: "#fbcfe8",
    blue: "#bfdbfe",
    green: "#bbf7d0",
    purple: "#e9d5ff",
  };

  // Slightly darker foreground variants for rendered markdown text.
  const TEXT_COLORS: Record<string, string> = {
    yellow: "#78350f",
    pink: "#9d174d",
    blue: "#1e3a8a",
    green: "#14532d",
    purple: "#581c87",
  };

  let text = note.text;
  let editing = false; // viewing → editing toggle
  let collapsed = false;
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  function toggleCollapse() {
    if (editing) return;
    collapsed = !collapsed;
  }

  $: collapsedPreview = text
    .replace(/^#{1,6}\s+/gm, '')
    .replace(/[*_`~]/g, '')
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .trim()
    .slice(0, 80);

  $: {
    // Sync text when note prop changes from server (e.g., remote update).
    if (text !== note.text && document.activeElement !== textareaEl) {
      text = note.text;
    }
  }

  let textareaEl: HTMLTextAreaElement;

  // Rendered markdown HTML.
  $: renderedHtml = (() => {
    try {
      return marked.parse(text || "", { async: false }) as string;
    } catch {
      return `<p>${text}</p>`;
    }
  })();

  function startEditing() {
    if (!canWrite) return;
    editing = true;
  }

  function stopEditing() {
    editing = false;
    // Flush any pending debounce immediately.
    if (debounceTimer !== null) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    dispatch("update", { ...note, text });
  }

  function handleTextInput() {
    if (debounceTimer !== null) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      dispatch("update", { ...note, text });
    }, 500);
  }

  function setColor(color: string) {
    dispatch("update", { ...note, color });
  }

  function togglePin() {
    dispatch("update", { ...note, pinned: !note.pinned });
  }

  function handleDragMousedown(event: MouseEvent) {
    if (!canWrite || note.pinned) return;
    event.preventDefault();
    dispatch("startMove", event);
  }
</script>

<!-- svelte-ignore a11y-no-static-element-interactions -->
<div
  class="sticky-note flex flex-col rounded shadow-lg select-none"
  style:width="260px"
  style:min-height={collapsed ? "0px" : "160px"}
  on:pointerdown={(e) => e.stopPropagation()}
>
  <!-- Header bar -->
  <div
    class="flex items-center gap-1 px-2 py-1 rounded-t flex-shrink-0"
    style:background-color={COLORS[note.color] ?? COLORS.yellow}
  >
    <!-- Drag handle -->
    <div
      class="flex-1 cursor-grab active:cursor-grabbing mr-1"
      class:cursor-default={note.pinned || !canWrite}
      class:active:cursor-default={note.pinned || !canWrite}
      on:mousedown={handleDragMousedown}
      on:dblclick={toggleCollapse}
      title={note.pinned ? "Note is pinned" : "Drag to move"}
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="currentColor"
        class="text-zinc-600 opacity-60"
      >
        <circle cx="9" cy="6" r="1.5" />
        <circle cx="15" cy="6" r="1.5" />
        <circle cx="9" cy="12" r="1.5" />
        <circle cx="15" cy="12" r="1.5" />
        <circle cx="9" cy="18" r="1.5" />
        <circle cx="15" cy="18" r="1.5" />
      </svg>
    </div>

    <!-- Color swatches -->
    {#each Object.entries(COLORS) as [colorKey, colorVal]}
      <button
        class="w-3.5 h-3.5 rounded-full border border-zinc-400 transition-transform hover:scale-125 focus:outline-none"
        style:background-color={colorVal}
        class:ring-1={note.color === colorKey}
        class:ring-zinc-600={note.color === colorKey}
        disabled={!canWrite}
        title="Set color: {colorKey}"
        on:click={() => setColor(colorKey)}
      />
    {/each}

    <!-- Edit / Preview toggle -->
    {#if canWrite}
      {#if editing}
        <button
          class="p-0.5 rounded hover:bg-black/10 transition-colors text-zinc-600"
          title="Preview"
          on:click={stopEditing}
        >
          <EyeIcon size="13" strokeWidth={1.5} />
        </button>
      {:else}
        <button
          class="p-0.5 rounded hover:bg-black/10 transition-colors text-zinc-600"
          title="Edit"
          on:click={startEditing}
        >
          <EditIcon size="13" strokeWidth={1.5} />
        </button>
      {/if}
    {/if}

    <!-- Pin button -->
    <button
      class="p-0.5 rounded hover:bg-black/10 transition-colors"
      class:text-zinc-800={note.pinned}
      class:text-zinc-500={!note.pinned}
      disabled={!canWrite}
      title={note.pinned ? "Unpin note" : "Pin note (lock position)"}
      on:click={togglePin}
    >
      <MapPinIcon size="13" strokeWidth={note.pinned ? 2.5 : 1.5} />
    </button>

    <!-- Delete button -->
    <button
      class="p-0.5 rounded hover:bg-black/10 transition-colors text-zinc-600"
      disabled={!canWrite}
      title="Delete note"
      on:click={() => dispatch("delete")}
    >
      <XIcon size="13" strokeWidth={2} />
    </button>
  </div>

  <!-- Body: collapsed preview or full edit/view -->
  {#if collapsed}
    <div class="px-2.5 py-1.5 text-sm rounded-b leading-snug"
      style:background-color="rgba(255,255,255,0.85)"
      style:color={TEXT_COLORS[note.color] ?? TEXT_COLORS.yellow}
    >
      {collapsedPreview || '…'}{text.trim().length > 80 ? '…' : ''}
    </div>
  {:else if editing}
    <textarea
      bind:this={textareaEl}
      bind:value={text}
      class="flex-1 w-full resize-none bg-white/80 text-zinc-900 text-sm p-2 rounded-b focus:outline-none placeholder-zinc-400"
      style="min-height: 120px;"
      placeholder="Type markdown here… (**bold**, *italic*, # heading, - list)"
      on:input={handleTextInput}
      on:blur={stopEditing}
    />
  {:else}
    <!-- svelte-ignore a11y-click-events-have-key-events -->
    <div
      class="markdown-body flex-1 bg-white/80 text-sm p-2.5 rounded-b overflow-y-auto cursor-text"
      style:color={TEXT_COLORS[note.color] ?? TEXT_COLORS.yellow}
      style="min-height: 120px;"
      on:click={startEditing}
      title={canWrite ? "Click to edit" : ""}
    >
      {#if text.trim()}
        {@html renderedHtml}
      {:else}
        <span class="text-zinc-400 italic text-xs">
          {canWrite ? "Click to write…" : "Empty note"}
        </span>
      {/if}
    </div>
  {/if}
</div>

<style lang="postcss">
  .sticky-note {
    @apply overflow-hidden;
    filter: drop-shadow(0 2px 8px rgba(0, 0, 0, 0.25));
  }

  textarea {
    font-family: inherit;
  }

  /* Markdown rendered output styles */
  .markdown-body :global(h1) {
    font-size: 1.1em;
    font-weight: 700;
    margin-bottom: 0.3em;
    line-height: 1.3;
  }
  .markdown-body :global(h2) {
    font-size: 1em;
    font-weight: 700;
    margin-bottom: 0.25em;
    line-height: 1.3;
  }
  .markdown-body :global(h3) {
    font-size: 0.9em;
    font-weight: 600;
    margin-bottom: 0.2em;
  }
  .markdown-body :global(p) {
    margin-bottom: 0.4em;
    line-height: 1.5;
  }
  .markdown-body :global(p:last-child) {
    margin-bottom: 0;
  }
  .markdown-body :global(strong) {
    font-weight: 700;
  }
  .markdown-body :global(em) {
    font-style: italic;
  }
  .markdown-body :global(ul),
  .markdown-body :global(ol) {
    padding-left: 1.2em;
    margin-bottom: 0.4em;
  }
  .markdown-body :global(li) {
    margin-bottom: 0.1em;
    line-height: 1.5;
  }
  .markdown-body :global(ul) {
    list-style-type: disc;
  }
  .markdown-body :global(ol) {
    list-style-type: decimal;
  }
  .markdown-body :global(code) {
    font-family: "Fira Code", monospace;
    font-size: 0.85em;
    background: rgba(0, 0, 0, 0.08);
    border-radius: 3px;
    padding: 0.1em 0.3em;
  }
  .markdown-body :global(pre) {
    background: rgba(0, 0, 0, 0.08);
    border-radius: 4px;
    padding: 0.5em 0.75em;
    overflow-x: auto;
    margin-bottom: 0.4em;
  }
  .markdown-body :global(pre code) {
    background: none;
    padding: 0;
    font-size: 0.8em;
  }
  .markdown-body :global(blockquote) {
    border-left: 3px solid rgba(0, 0, 0, 0.2);
    padding-left: 0.6em;
    margin: 0.3em 0;
    opacity: 0.8;
  }
  .markdown-body :global(hr) {
    border: none;
    border-top: 1px solid rgba(0, 0, 0, 0.15);
    margin: 0.4em 0;
  }
  .markdown-body :global(a) {
    text-decoration: underline;
    opacity: 0.85;
  }
</style>
