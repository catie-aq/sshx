<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from "svelte";
  import { fade } from "svelte/transition";

  export let x: number = 0;
  export let y: number = 0;
  export let visible: boolean = false;

  const dispatch = createEventDispatcher<{ close: void; select: string }>();

  type MenuItem =
    | { type: "item"; label: string; key: string; shortcut?: string; disabled?: boolean }
    | { type: "separator" };

  const items: MenuItem[] = [
    { type: "item", label: "New Terminal", key: "new-terminal", shortcut: "Ctrl+Shift+N" },
    { type: "item", label: "New Sticky Note", key: "new-note" },
    { type: "separator" },
    { type: "item", label: "Paste", key: "paste", shortcut: "Ctrl+V" },
    { type: "item", label: "Select All", key: "select-all", shortcut: "Ctrl+A", disabled: true },
    { type: "separator" },
    { type: "item", label: "Fit to Screen", key: "fit-screen" },
    { type: "item", label: "Reset Zoom", key: "reset-zoom" },
    { type: "separator" },
    { type: "item", label: "Toggle File Tree", key: "toggle-file-tree" },
    { type: "item", label: "Toggle Chat", key: "toggle-chat" },
    { type: "item", label: "Command Palette", key: "command-palette", shortcut: "Ctrl+K" },
  ];

  let menuEl: HTMLDivElement;

  function handleClick(item: MenuItem) {
    if (item.type === "separator") return;
    if (item.disabled) return;
    dispatch("select", item.key);
    dispatch("close");
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      dispatch("close");
    }
  }

  function handleClickOutside(e: MouseEvent) {
    if (menuEl && !menuEl.contains(e.target as Node)) {
      dispatch("close");
    }
  }

  onMount(() => {
    document.addEventListener("keydown", handleKeydown);
    // Delay so the contextmenu click itself doesn't immediately close
    setTimeout(() => document.addEventListener("mousedown", handleClickOutside), 0);
  });

  onDestroy(() => {
    document.removeEventListener("keydown", handleKeydown);
    document.removeEventListener("mousedown", handleClickOutside);
  });

  // Clamp position so menu doesn't overflow viewport
  $: clampedX = Math.min(x, window.innerWidth - 200);
  $: clampedY = Math.min(y, window.innerHeight - 320);
</script>

{#if visible}
  <div
    bind:this={menuEl}
    class="context-menu"
    style:left="{clampedX}px"
    style:top="{clampedY}px"
    transition:fade={{ duration: 100 }}
  >
    {#each items as item}
      {#if item.type === "separator"}
        <div class="separator" />
      {:else}
        <button
          class="menu-item"
          class:disabled={item.disabled}
          on:click={() => handleClick(item)}
        >
          <span>{item.label}</span>
          {#if item.shortcut}
            <span class="shortcut">{item.shortcut}</span>
          {/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .context-menu {
    position: fixed;
    z-index: 9999;
    min-width: 180px;
    background: rgb(39 39 42); /* zinc-800 */
    border: 1px solid rgb(63 63 70); /* zinc-700 */
    border-radius: 0.5rem;
    padding: 4px 0;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }

  .menu-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 6px 12px;
    font-size: 0.8125rem;
    color: rgb(212 212 216); /* zinc-300 */
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
  }

  .menu-item:hover:not(.disabled) {
    background: rgb(63 63 70); /* zinc-700 */
  }

  .menu-item.disabled {
    color: rgb(113 113 122); /* zinc-500 */
    cursor: default;
  }

  .shortcut {
    margin-left: 24px;
    font-size: 0.6875rem;
    color: rgb(113 113 122); /* zinc-500 */
  }

  .separator {
    height: 1px;
    margin: 4px 8px;
    background: rgb(63 63 70); /* zinc-700 */
  }
</style>
