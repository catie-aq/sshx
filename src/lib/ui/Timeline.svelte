<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { WsSlide } from "$lib/protocol";

  export let slides: Map<number, WsSlide>;
  export let currentIndex: number = 0;
  export let playing: boolean = false;
  export let canWrite: boolean = false;

  export let exporting: boolean = false;

  const dispatch = createEventDispatcher<{
    navigate: number;
    create: void;
    reorder: [number, number][];
    play: void;
    stop: void;
    delete: number;
    exportPdf: void;
  }>();

  $: sortedSlides = [...slides.entries()]
    .sort(([, a], [, b]) => a.order - b.order);

  let dragIndex: number | null = null;
  let dropIndex: number | null = null;

  function handleDragStart(e: DragEvent, index: number) {
    dragIndex = index;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = "move";
    }
  }

  function handleDragOver(e: DragEvent, index: number) {
    e.preventDefault();
    dropIndex = index;
  }

  function handleDrop(e: DragEvent, toIndex: number) {
    e.preventDefault();
    if (dragIndex === null || dragIndex === toIndex) {
      dragIndex = null;
      dropIndex = null;
      return;
    }
    // Reorder: assign new sequential orders
    const items = [...sortedSlides];
    const [moved] = items.splice(dragIndex, 1);
    items.splice(toIndex, 0, moved);
    const reorder: [number, number][] = items.map(([slid], i) => [slid, i + 1]);
    dispatch("reorder", reorder);
    dragIndex = null;
    dropIndex = null;
  }

  function handleDragEnd() {
    dragIndex = null;
    dropIndex = null;
  }
</script>

<div class="timeline-panel panel">
  <div class="timeline-header">
    <span class="text-xs text-zinc-400 font-medium uppercase tracking-wide">Slides</span>
    <div class="flex gap-1">
      {#if playing}
        <button class="control-btn stop" on:click={() => dispatch("stop")} title="Exit play mode">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="1"/></svg>
        </button>
      {:else}
        <button class="control-btn play" on:click={() => dispatch("play")} title="Play slideshow">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><polygon points="5,3 19,12 5,21"/></svg>
        </button>
      {/if}
      <!-- Export PDF -->
      {#if sortedSlides.length > 0}
        <button
          class="control-btn export"
          on:click={() => dispatch("exportPdf")}
          title="Export PDF"
          disabled={exporting}
        >
          {#if exporting}
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" class="animate-spin">
              <path d="M12 2v4m0 12v4m-7.07-3.93l2.83-2.83m8.48-8.48l2.83-2.83M2 12h4m12 0h4m-3.93 7.07l-2.83-2.83M7.76 7.76L4.93 4.93"/>
            </svg>
          {:else}
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/>
              <polyline points="14 2 14 8 20 8"/>
              <line x1="12" y1="18" x2="12" y2="12"/>
              <line x1="9" y1="15" x2="12" y2="18"/>
              <line x1="15" y1="15" x2="12" y2="18"/>
            </svg>
          {/if}
        </button>
      {/if}
      {#if canWrite}
        <button class="control-btn" on:click={() => dispatch("create")} title="Add slide">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round">
            <line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>
          </svg>
        </button>
      {/if}
    </div>
  </div>

  <div class="slide-list">
    {#each sortedSlides as [slid, slide], i (slid)}
      <!-- svelte-ignore a11y-no-static-element-interactions -->
      <div
        class="slide-item"
        class:active={i === currentIndex && playing}
        class:drop-target={dropIndex === i}
        draggable={canWrite}
        on:dragstart={(e) => handleDragStart(e, i)}
        on:dragover={(e) => handleDragOver(e, i)}
        on:drop={(e) => handleDrop(e, i)}
        on:dragend={handleDragEnd}
        on:click={() => dispatch("navigate", i)}
      >
        <span class="slide-num">{slide.order}</span>
        <span class="slide-name">{slide.label || `Slide ${slide.order}`}</span>
        {#if canWrite}
          <button class="slide-delete" on:click|stopPropagation={() => dispatch("delete", slid)}>
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        {/if}
      </div>
    {/each}
  </div>

  {#if exporting}
    <div class="export-status">
      <span class="animate-pulse">Exporting slides...</span>
    </div>
  {/if}
</div>

<style lang="postcss">
  .timeline-panel {
    @apply flex flex-col w-48 max-h-96 border border-zinc-700;
  }

  .timeline-header {
    @apply flex items-center justify-between px-2 py-1.5 border-b border-zinc-700;
  }

  .control-btn {
    @apply p-1 rounded text-zinc-400 hover:text-white hover:bg-zinc-700 transition-colors;
  }

  .control-btn.play {
    @apply text-emerald-400 hover:text-emerald-300;
  }

  .control-btn.stop {
    @apply text-red-400 hover:text-red-300;
  }

  .control-btn.export {
    @apply text-indigo-400 hover:text-indigo-300;
  }

  .control-btn:disabled {
    @apply opacity-40 cursor-not-allowed;
  }

  .slide-list {
    @apply flex-1 overflow-y-auto p-1;
  }

  .slide-item {
    @apply flex items-center gap-2 px-2 py-1.5 rounded cursor-pointer hover:bg-zinc-700 transition-colors text-sm;
  }

  .slide-item.active {
    @apply bg-indigo-900/50 text-indigo-300;
  }

  .slide-item.drop-target {
    @apply border-t-2 border-indigo-400;
  }

  .slide-num {
    @apply w-5 h-5 rounded-full bg-zinc-700 text-zinc-300 text-xs flex items-center justify-center flex-shrink-0;
  }

  .slide-item.active .slide-num {
    @apply bg-indigo-600 text-white;
  }

  .slide-name {
    @apply flex-1 truncate text-zinc-300;
  }

  .slide-delete {
    @apply opacity-0 text-zinc-500 hover:text-red-400 transition-all;
  }

  .slide-item:hover .slide-delete {
    @apply opacity-100;
  }

  .export-status {
    @apply px-2 py-1.5 text-xs text-indigo-300 bg-indigo-900/30 border-t border-zinc-700 text-center;
  }
</style>
