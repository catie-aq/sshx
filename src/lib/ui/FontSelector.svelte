<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { FONTS, getFontFamily } from "$lib/fonts";

  export let value: string = "inter";

  const dispatch = createEventDispatcher<{ change: string }>();

  let open = false;

  function select(fontId: string) {
    value = fontId;
    open = false;
    dispatch("change", fontId);
  }
</script>

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="font-selector" on:mousedown|stopPropagation>
  <button
    class="trigger"
    title="Font"
    on:click={() => (open = !open)}
  >
    <span style:font-family={getFontFamily(value)} class="font-preview">
      {FONTS.find((f) => f.id === value)?.label ?? "Font"}
    </span>
    <svg width="8" height="5" viewBox="0 0 8 5" fill="currentColor" class="ml-0.5 opacity-60">
      <path d="M0 0l4 5 4-5z" />
    </svg>
  </button>

  {#if open}
    <div class="dropdown">
      {#each FONTS as font}
        <button
          class="font-option"
          class:active={value === font.id}
          on:click={() => select(font.id)}
        >
          <span style:font-family={font.family} class="font-option-label">{font.label}</span>
          <span class="font-option-cat">{font.category}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style lang="postcss">
  .font-selector {
    position: relative;
  }

  .trigger {
    @apply flex items-center gap-0.5 bg-zinc-700 text-zinc-200 text-xs rounded px-1.5 py-0.5 border border-zinc-600 hover:border-zinc-500 transition-colors;
    cursor: pointer;
    min-width: 64px;
  }

  .font-preview {
    @apply text-xs leading-none truncate;
    max-width: 56px;
  }

  .dropdown {
    @apply absolute bg-zinc-800 border border-zinc-700 rounded-lg shadow-xl overflow-hidden;
    top: calc(100% + 4px);
    left: 0;
    z-index: 20;
    min-width: 140px;
  }

  .font-option {
    @apply flex items-center justify-between w-full px-2 py-1.5 text-left text-zinc-300 hover:bg-zinc-700 transition-colors;
  }

  .font-option.active {
    @apply bg-zinc-600 text-white;
  }

  .font-option-label {
    @apply text-sm truncate;
  }

  .font-option-cat {
    @apply text-[10px] text-zinc-500 ml-2;
  }
</style>
