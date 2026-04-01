<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { WsSlide } from "$lib/protocol";

  export let slide: WsSlide;
  export let canWrite: boolean;
  export let playing: boolean = false;

  const dispatch = createEventDispatcher<{
    startMove: MouseEvent;
    startResize: MouseEvent;
    delete: void;
  }>();

  const BRACKET_SIZE = 28;
  const BRACKET_STROKE = 3.5;

  function handleDrag(e: MouseEvent) {
    if (!canWrite || playing) return;
    e.preventDefault();
    dispatch("startMove", e);
  }

  function handleResize(e: MouseEvent) {
    if (!canWrite || playing) return;
    e.preventDefault();
    e.stopPropagation();
    dispatch("startResize", e);
  }
</script>

<!-- svelte-ignore a11y-no-static-element-interactions -->
{#if !playing}
  <div
    class="slide-region"
    style:width="{slide.w}px"
    style:height="{slide.h}px"
    on:mousedown={handleDrag}
    on:pointerdown={(e) => { if (e.button === 0) e.stopPropagation(); }}
  >
    <!-- Subtle dashed border for slide area -->
    <div class="absolute inset-0 border border-dashed border-current opacity-20 rounded pointer-events-none" />

    <!-- Corner brackets -->
    <svg class="absolute inset-0 w-full h-full pointer-events-none" overflow="visible">
      <!-- Top-left -->
      <polyline points="{BRACKET_SIZE},0 0,0 0,{BRACKET_SIZE}" fill="none" stroke="currentColor" stroke-width={BRACKET_STROKE} stroke-linecap="round" />
      <!-- Top-right -->
      <polyline points="{slide.w - BRACKET_SIZE},0 {slide.w},0 {slide.w},{BRACKET_SIZE}" fill="none" stroke="currentColor" stroke-width={BRACKET_STROKE} stroke-linecap="round" />
      <!-- Bottom-left -->
      <polyline points="{BRACKET_SIZE},{slide.h} 0,{slide.h} 0,{slide.h - BRACKET_SIZE}" fill="none" stroke="currentColor" stroke-width={BRACKET_STROKE} stroke-linecap="round" />
      <!-- Bottom-right -->
      <polyline points="{slide.w - BRACKET_SIZE},{slide.h} {slide.w},{slide.h} {slide.w},{slide.h - BRACKET_SIZE}" fill="none" stroke="currentColor" stroke-width={BRACKET_STROKE} stroke-linecap="round" />
    </svg>

    <!-- Order badge (top-left) -->
    <div class="order-badge">{slide.order}</div>

    <!-- Label (centered) -->
    {#if slide.label}
      <div class="slide-label">{slide.label}</div>
    {/if}

    <!-- Resize handle (bottom-right) -->
    {#if canWrite}
      <!-- svelte-ignore a11y-no-static-element-interactions -->
      <div class="resize-handle" on:mousedown={handleResize}>
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
          <line x1="9" y1="1" x2="1" y2="9" stroke="currentColor" stroke-width="1"/>
          <line x1="9" y1="4" x2="4" y2="9" stroke="currentColor" stroke-width="1"/>
          <line x1="9" y1="7" x2="7" y2="9" stroke="currentColor" stroke-width="1"/>
        </svg>
      </div>
    {/if}

    <!-- Delete button -->
    {#if canWrite}
      <button
        class="delete-btn"
        on:click|stopPropagation={() => dispatch("delete")}
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
          <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>
    {/if}
  </div>
{/if}

<style lang="postcss">
  .slide-region {
    position: relative;
    color: rgba(129, 140, 248, 0.8);
    cursor: grab;
  }

  .slide-region:hover {
    color: rgba(129, 140, 248, 1);
  }

  .order-badge {
    @apply absolute -top-3 -left-3 w-6 h-6 rounded-full bg-indigo-500 text-white text-xs flex items-center justify-center font-bold;
  }

  .slide-label {
    @apply absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 text-indigo-400 text-sm opacity-60 pointer-events-none;
  }

  .resize-handle {
    @apply absolute bottom-1 right-1 cursor-nwse-resize opacity-40 hover:opacity-80;
    color: currentColor;
  }

  .delete-btn {
    @apply absolute -top-3 -right-3 w-5 h-5 rounded-full bg-red-500 text-white flex items-center justify-center opacity-0 hover:opacity-100 transition-opacity;
  }

  .slide-region:hover .delete-btn {
    @apply opacity-60;
  }
</style>
