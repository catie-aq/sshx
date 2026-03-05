<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import CircleButton from "./CircleButton.svelte";
  import CircleButtons from "./CircleButtons.svelte";

  /** npm package name, e.g. "next", "lucide-react" */
  export let name: string;
  /** Local source file paths that import this library. */
  export let importedBy: string[] = [];

  export let w: number = 240;
  export let h: number = 220;

  const dispatch = createEventDispatcher<{
    startMove: MouseEvent;
    startResize: PointerEvent;
    delete: void;
    openFile: string;
  }>();
</script>

<div class="library-card flex flex-col select-none relative" style:width="{w}px" style:height="{h}px">
  <!-- Header / drag handle -->
  <div
    class="flex select-none flex-shrink-0 cursor-grab active:cursor-grabbing"
    on:mousedown={(e) => dispatch("startMove", e)}
  >
    <div class="flex-1 flex items-center px-3 py-1.5">
      <CircleButtons>
        <CircleButton
          kind="red"
          on:mousedown={(e) => e.button === 0 && dispatch("delete")}
        />
      </CircleButtons>
    </div>
    <div
      class="py-2 text-sm text-zinc-300 text-center font-medium font-mono w-0 flex-grow-[4] overflow-hidden whitespace-nowrap text-ellipsis"
    >
      {name}
    </div>
    <div class="flex-1 flex items-center justify-end pr-2">
      <span class="text-[9px] font-semibold uppercase tracking-wide text-amber-600 border border-amber-800 rounded px-1 py-px">ext</span>
    </div>
  </div>

  <!-- Body -->
  <div
    class="bg-zinc-800 rounded-b-lg px-3 py-2 flex flex-col gap-1.5 flex-1 overflow-y-auto min-h-0"
    on:wheel={(e) => { if (!e.ctrlKey && !e.metaKey && !e.altKey) e.stopPropagation(); }}
  >
    <p class="text-[10px] text-zinc-500 italic leading-tight flex-shrink-0">
      External package — read-only, no code preview.
    </p>

    {#if importedBy.length > 0}
      <div class="flex flex-col gap-0.5">
        <span class="text-[10px] text-zinc-500 flex-shrink-0">imported by:</span>
        {#each importedBy as imp (imp)}
          <button
            class="text-xs text-zinc-400 hover:text-indigo-300 text-left px-1 py-0.5 rounded hover:bg-zinc-700 font-mono truncate flex-shrink-0"
            on:click={() => dispatch("openFile", imp)}
            on:mousedown|stopPropagation
            title={imp}
          >{imp}</button>
        {/each}
      </div>
    {:else}
      <p class="text-xs text-zinc-600 italic">No local imports found.</p>
    {/if}
  </div>

  <!-- Resize handle -->
  <div
    class="absolute bottom-0 right-0 w-3 h-3 cursor-nwse-resize"
    on:pointerdown|stopPropagation={(e) => dispatch("startResize", e)}
  />
</div>

<style lang="postcss">
  .library-card {
    /* Slightly lighter than regular FileCard (zinc-800 panel) to signal read-only */
    @apply inline-flex rounded-lg border border-zinc-600 bg-zinc-700 opacity-90;
    transition: opacity 200ms;
  }

  .library-card:hover {
    @apply opacity-100;
  }
</style>
