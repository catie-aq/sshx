<script lang="ts">
  import { createEventDispatcher } from "svelte";

  export let activeTool: "pencil" | "highlighter" | null = null;
  export let color: string = "#ffffff";

  const dispatch = createEventDispatcher<{
    toolChange: "pencil" | "highlighter" | null;
    colorChange: string;
  }>();

  const COLORS = [
    "#ffffff", "#a1a1aa", // white, gray
    "#fde047", "#fb923c", // yellow, orange
    "#f87171", "#4ade80", // red, green
    "#38bdf8", "#c084fc", // cyan, purple
    "#ec4899",            // pink (highlighter default)
  ];

  function toggleTool(tool: "pencil" | "highlighter") {
    const next = activeTool === tool ? null : tool;
    activeTool = next;
    dispatch("toolChange", next);
  }

  function setColor(c: string) {
    color = c;
    dispatch("colorChange", c);
  }
</script>

{#if activeTool !== null}
  <div class="drawing-toolbar panel">
    <!-- Tool buttons -->
    <div class="tool-row">
      <!-- Cursor (deselect) -->
      <button
        class="tool-btn"
        class:active={activeTool === null}
        title="Select (Esc)"
        on:click={() => { activeTool = null; dispatch("toolChange", null); }}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M3 3l7.07 16.97 2.51-7.39 7.39-2.51L3 3z"/>
          <path d="M13 13l6 6"/>
        </svg>
      </button>

      <!-- Pencil -->
      <button
        class="tool-btn"
        class:active={activeTool === "pencil"}
        title="Pencil"
        on:click={() => toggleTool("pencil")}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z"/>
        </svg>
      </button>

      <!-- Highlighter -->
      <button
        class="tool-btn highlighter-btn"
        class:active={activeTool === "highlighter"}
        title="Highlighter"
        on:click={() => toggleTool("highlighter")}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M12 20h9"/>
          <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z"/>
          <path d="M15 5l3 3" opacity="0.5"/>
        </svg>
      </button>
    </div>

    <!-- Color dots -->
    <div class="color-row">
      {#each COLORS as c}
        <button
          class="color-dot"
          class:active={color === c}
          style:background-color={c}
          on:click={() => setColor(c)}
        />
      {/each}
    </div>
  </div>
{/if}

<style lang="postcss">
  .drawing-toolbar {
    @apply flex flex-col gap-2 px-3 py-2 border border-zinc-700;
  }

  .tool-row {
    @apply flex items-center gap-1;
  }

  .tool-btn {
    @apply p-1.5 rounded-md text-zinc-400 hover:bg-zinc-700 hover:text-white transition-colors;
  }

  .tool-btn.active {
    @apply bg-zinc-600 text-white;
  }

  .highlighter-btn.active {
    @apply bg-pink-900/60 text-pink-300;
  }

  .color-row {
    @apply flex items-center gap-1 flex-wrap;
  }

  .color-dot {
    @apply w-4 h-4 rounded-full border border-zinc-600 hover:scale-125 transition-transform cursor-pointer;
  }

  .color-dot.active {
    @apply ring-2 ring-white ring-offset-1 ring-offset-zinc-900;
  }
</style>
