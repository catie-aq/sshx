<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from "svelte";

  export let x: number = 0;
  export let y: number = 0;
  export let files: string[] = [];

  const dispatch = createEventDispatcher<{ open: string; close: void }>();

  let query = "";
  let selectedIndex = 0;
  let inputEl: HTMLInputElement;

  $: filtered = files
    .filter((f) => f.toLowerCase().includes(query.toLowerCase()))
    .slice(0, 8);

  $: if (query) selectedIndex = 0;

  $: clampedX = Math.min(x, window.innerWidth - 280);
  $: clampedY = Math.min(y, window.innerHeight - 240);

  function confirm() {
    const path = filtered[selectedIndex];
    if (path) dispatch("open", path);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      dispatch("close");
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      selectedIndex = Math.min(selectedIndex + 1, filtered.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selectedIndex = Math.max(selectedIndex - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      confirm();
    }
  }

  function handleClickOutside(e: MouseEvent) {
    const el = document.getElementById("open-file-dialog");
    if (el && !el.contains(e.target as Node)) dispatch("close");
  }

  onMount(() => {
    inputEl?.focus();
    setTimeout(() => document.addEventListener("mousedown", handleClickOutside), 0);
  });

  onDestroy(() => {
    document.removeEventListener("mousedown", handleClickOutside);
  });
</script>

<div
  id="open-file-dialog"
  class="dialog"
  style:left="{clampedX}px"
  style:top="{clampedY}px"
>
  <input
    bind:this={inputEl}
    bind:value={query}
    on:keydown={handleKeydown}
    placeholder="File path…"
    class="file-input"
  />
  {#if filtered.length > 0}
    <ul class="completions">
      {#each filtered as file, i}
        <li
          class="completion-item"
          class:active={i === selectedIndex}
          on:mousedown|preventDefault={() => { selectedIndex = i; confirm(); }}
          on:mousemove={() => (selectedIndex = i)}
        >
          {file}
        </li>
      {/each}
    </ul>
  {:else if query}
    <div class="no-results">No matches</div>
  {/if}
</div>

<style>
  .dialog {
    position: fixed;
    z-index: 9999;
    width: 260px;
    background: rgb(39 39 42); /* zinc-800 */
    border: 1px solid rgb(63 63 70); /* zinc-700 */
    border-radius: 0.5rem;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    overflow: hidden;
  }

  .file-input {
    width: 100%;
    padding: 7px 10px;
    font-size: 0.8125rem;
    color: rgb(212 212 216);
    background: none;
    border: none;
    border-bottom: 1px solid rgb(63 63 70);
    outline: none;
    box-sizing: border-box;
  }

  .file-input::placeholder {
    color: rgb(113 113 122);
  }

  .completions {
    list-style: none;
    margin: 0;
    padding: 4px 0;
    max-height: 200px;
    overflow-y: auto;
  }

  .completion-item {
    padding: 5px 10px;
    font-size: 0.8125rem;
    font-family: monospace;
    color: rgb(212 212 216);
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .completion-item.active {
    background: rgb(63 63 70);
  }

  .no-results {
    padding: 6px 10px;
    font-size: 0.8125rem;
    color: rgb(113 113 122);
  }
</style>
