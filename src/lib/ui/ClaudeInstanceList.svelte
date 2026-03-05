<script lang="ts">
  import { createEventDispatcher } from "svelte";

  export let instances: Map<string, { events: unknown[]; transcriptPath: string | null; sessionName: string | null; fileMtime: number | null }>;

  const dispatch = createEventDispatcher<{
    openInstance: string;
    close: void;
  }>();

  /** Svelte action: dispatch "close" when a click occurs outside this element. */
  function clickOutside(node: HTMLElement) {
    function handle(e: MouseEvent) {
      if (!node.contains(e.target as Node)) dispatch("close");
    }
    document.addEventListener("click", handle, true);
    return { destroy() { document.removeEventListener("click", handle, true); } };
  }

  function formatMtime(mtime: number | null): string {
    if (mtime == null) return "";
    const d = new Date(mtime * 1000);
    const now = new Date();
    const isToday = d.toDateString() === now.toDateString();
    if (isToday) {
      return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    }
    return d.toLocaleDateString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  }
</script>

<!-- svelte-ignore a11y-no-static-element-interactions -->
<div
  class="absolute top-full right-0 mt-1 z-50 w-72 rounded-lg border border-zinc-700 bg-zinc-900 shadow-xl"
  use:clickOutside
  on:keydown={(e) => e.key === "Escape" && dispatch("close")}
>
  <div class="px-3 py-2 text-xs font-medium text-zinc-400 border-b border-zinc-800 select-none">
    ✦ Claude Sessions
  </div>
  {#if instances.size === 0}
    <div class="px-3 py-3 text-xs text-zinc-500 text-center">No Claude sessions detected.</div>
  {:else}
    {#each [...instances.entries()] as [sid, inst] (sid)}
      <div class="flex items-center gap-2 px-3 py-2 hover:bg-zinc-800">
        <span
          class="w-2 h-2 rounded-full flex-shrink-0 mt-0.5"
          class:bg-emerald-400={inst.events.length > 0}
          class:animate-pulse={inst.events.length > 0}
          class:bg-zinc-600={inst.events.length === 0}
          title={inst.events.length > 0 ? "Active" : "Idle"}
        ></span>
        <div class="flex-1 min-w-0">
          <div class="text-xs text-zinc-200 truncate" title={inst.sessionName ?? sid}>
            {inst.sessionName ?? sid.slice(0, 8)}
          </div>
          {#if inst.fileMtime != null}
            <div class="text-[10px] text-zinc-500 mt-0.5">{formatMtime(inst.fileMtime)}</div>
          {/if}
        </div>
        <span class="text-xs text-zinc-500 flex-shrink-0">{inst.events.length}</span>
        <button
          class="text-xs px-2 py-0.5 rounded bg-zinc-700 hover:bg-zinc-600 text-zinc-300 transition-colors flex-shrink-0"
          on:click|stopPropagation={() => dispatch("openInstance", sid)}
        >open</button>
      </div>
    {/each}
  {/if}
</div>
