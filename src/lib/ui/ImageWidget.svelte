<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { WsWidget } from "../protocol";
  import CircleButton from "./CircleButton.svelte";
  import CircleButtons from "./CircleButtons.svelte";

  export let widget: WsWidget;
  export let canWrite: boolean;

  const dispatch = createEventDispatcher<{
    startMove: MouseEvent;
    delete: void;
    rename: string;
  }>();

  let el: HTMLDivElement;
  let editing = false;
  let editValue = "";

  $: imageUrl = widget.kind.type === "image" ? widget.kind.url : "";
  $: imageAlt = widget.kind.type === "image" ? widget.kind.alt : "";
  $: displayName = widget.name ?? "Image";

  function handleKeydown(e: KeyboardEvent) {
    if (!canWrite) return;
    if (editing) return;
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      dispatch("delete");
    }
  }

  function handleMousedown(e: MouseEvent) {
    el?.focus();
    if (canWrite) dispatch("startMove", e);
  }

  function startEditing() {
    if (!canWrite) return;
    editing = true;
    editValue = widget.name ?? "";
  }

  function commitEdit() {
    editing = false;
    const name = editValue.trim();
    if (name && name !== (widget.name ?? "")) {
      dispatch("rename", name);
    }
  }

  function cancelEdit() {
    editing = false;
  }

  function handleEditKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      commitEdit();
    } else if (e.key === "Escape") {
      cancelEdit();
    }
  }
</script>

<!-- svelte-ignore a11y-no-noninteractive-tabindex -->
<div
  bind:this={el}
  class="image-widget"
  style:width="{widget.w}px"
  tabindex="0"
  on:keydown={handleKeydown}
>
  <!-- Title bar -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div
    class="flex select-none flex-shrink-0 cursor-grab active:cursor-grabbing items-center border-b border-zinc-700 bg-zinc-800/80"
    on:mousedown={handleMousedown}
  >
    <div class="flex-1 flex items-center px-2 py-1">
      <CircleButtons>
        <CircleButton
          kind="red"
          on:mousedown={(event) => event.button === 0 && dispatch("delete")}
        />
      </CircleButtons>
    </div>
    <div
      class="py-1 text-sm text-zinc-300 text-center overflow-hidden whitespace-nowrap text-ellipsis w-0 flex-grow-[4]"
    >
      {#if editing}
        <!-- svelte-ignore a11y-autofocus -->
        <input
          type="text"
          class="bg-zinc-700 text-zinc-200 text-sm text-center rounded px-1 w-full outline-none"
          bind:value={editValue}
          on:blur={commitEdit}
          on:keydown={handleEditKeydown}
          autofocus
        />
      {:else}
        <!-- svelte-ignore a11y-no-static-element-interactions -->
        <span
          class="cursor-text"
          on:dblclick|stopPropagation={startEditing}
          title="Double-click to rename"
        >{displayName}</span>
      {/if}
    </div>
    <div class="flex-1" />
  </div>

  <!-- Image -->
  <div class="flex-1 overflow-hidden min-h-0">
    {#if imageUrl}
      <img
        src={imageUrl}
        alt={imageAlt}
        class="w-full h-full object-contain pointer-events-none"
        draggable="false"
      />
    {/if}
  </div>
</div>

<style lang="postcss">
  .image-widget {
    @apply inline-flex flex-col rounded border border-zinc-600 bg-zinc-900 overflow-hidden cursor-default;
    outline: none;
  }
  .image-widget:focus {
    @apply ring-2 ring-indigo-400;
  }
</style>
