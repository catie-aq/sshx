<!-- @component Spotlight-style command palette for navigating terminals and file cards -->
<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { SearchIcon } from "svelte-feather-icons";
  import type { SearchItem } from "$lib/protocol";

  type SearchItemType = SearchItem["type"];

  export let open: boolean = false;
  export let items: SearchItem[] = [];

  const dispatch = createEventDispatcher<{
    close: void;
    navigate: { x: number; y: number };
  }>();

  let query = "";
  let activeIndex = 0;

  $: filtered = items.filter((item) => {
    const q = query.toLowerCase();
    return (
      item.label.toLowerCase().includes(q) ||
      item.sublabel.toLowerCase().includes(q) ||
      (item.description ?? "").toLowerCase().includes(q)
    );
  });

  $: if (open) {
    query = "";
    activeIndex = 0;
  }

  $: if (activeIndex >= filtered.length) {
    activeIndex = Math.max(0, filtered.length - 1);
  }

  function select(item: SearchItem) {
    dispatch("navigate", { x: item.x, y: item.y });
    dispatch("close");
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      dispatch("close");
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      activeIndex = Math.min(activeIndex + 1, filtered.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      activeIndex = Math.max(activeIndex - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (filtered[activeIndex]) select(filtered[activeIndex]);
    }
  }

  function badgeClass(type: SearchItemType): string {
    if (type === "terminal") return "bg-indigo-500/20 text-indigo-400";
    if (type === "fileTree") return "bg-yellow-500/20 text-yellow-300";
    return "bg-emerald-500/20 text-emerald-400";
  }

  function badgeLabel(type: SearchItemType): string {
    if (type === "terminal") return "Terminal";
    if (type === "fileTree") return "Tree";
    return "File";
  }
</script>

{#if open}
  <!-- Backdrop -->
  <div
    class="fixed inset-0 bg-black/40 backdrop-blur-sm z-50 flex items-start justify-center pt-24"
    on:mousedown|self={() => dispatch("close")}
    on:keydown={handleKeydown}
    role="dialog"
    aria-modal="true"
    aria-label="Command palette"
  >
    <div
      class="w-full max-w-lg bg-zinc-900 border border-zinc-700 rounded-lg shadow-2xl overflow-hidden"
      on:mousedown|stopPropagation
    >
      <!-- Search input -->
      <div class="flex items-center gap-2 px-3 py-2.5 border-b border-zinc-700">
        <SearchIcon size="14" class="text-zinc-500 flex-shrink-0" strokeWidth={1.5} />
        <input
          autofocus
          bind:value={query}
          on:keydown={handleKeydown}
          class="flex-1 bg-transparent text-sm text-zinc-200 placeholder-zinc-500 outline-none"
          placeholder="Search terminals and files…"
          autocomplete="off"
          spellcheck="false"
        />
        <kbd class="text-[10px] text-zinc-500 border border-zinc-700 rounded px-1 py-0.5 leading-none">Esc</kbd>
      </div>

      <!-- Results list -->
      <div class="max-h-80 overflow-y-auto">
        {#if filtered.length === 0}
          <div class="px-4 py-6 text-center text-sm text-zinc-500">
            No results for "{query}"
          </div>
        {:else}
          {#each filtered as item, i (item.type + item.id)}
            <button
              class="w-full flex items-start gap-3 px-3 py-2 text-left transition-colors"
              class:bg-zinc-800={i === activeIndex}
              on:mouseenter={() => (activeIndex = i)}
              on:click={() => select(item)}
            >
              <span class="flex-shrink-0 mt-0.5 text-[10px] font-medium px-1.5 py-0.5 rounded {badgeClass(item.type)}">
                {badgeLabel(item.type)}
              </span>
              <div class="flex-1 min-w-0">
                <div class="flex items-baseline gap-2">
                  <span class="text-sm truncate" class:text-zinc-200={i === activeIndex} class:text-zinc-300={i !== activeIndex}>{item.label}</span>
                  {#if item.sublabel}
                    <span class="text-[11px] text-zinc-500 flex-shrink-0">{item.sublabel}</span>
                  {/if}
                </div>
                {#if item.description}
                  <div class="text-xs text-zinc-500 truncate mt-0.5">{item.description}</div>
                {/if}
              </div>
            </button>
          {/each}
        {/if}
      </div>
    </div>
  </div>
{/if}
