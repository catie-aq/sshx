<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { WsWidget } from "../protocol";

  export let widget: WsWidget;
  export let canWrite: boolean;

  const dispatch = createEventDispatcher<{
    startMove: MouseEvent;
    delete: void;
  }>();

  let el: HTMLDivElement;

  $: imageUrl = widget.kind.type === "image" ? widget.kind.url : "";
  $: imageAlt = widget.kind.type === "image" ? widget.kind.alt : "";

  function handleKeydown(e: KeyboardEvent) {
    if (!canWrite) return;
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      dispatch("delete");
    }
  }

  function handleMousedown(e: MouseEvent) {
    el?.focus();
    if (canWrite) dispatch("startMove", e);
  }
</script>

<!-- svelte-ignore a11y-no-noninteractive-tabindex -->
<div
  bind:this={el}
  class="image-widget"
  style:width="{widget.w}px"
  style:height="{widget.h}px"
  tabindex="0"
  on:keydown={handleKeydown}
  on:mousedown={handleMousedown}
>
  {#if imageUrl}
    <img
      src={imageUrl}
      alt={imageAlt}
      class="w-full h-full object-contain pointer-events-none"
      draggable="false"
    />
  {/if}
</div>

<style lang="postcss">
  .image-widget {
    @apply inline-flex rounded border border-zinc-600 bg-zinc-900 overflow-hidden cursor-grab active:cursor-grabbing;
    outline: none;
  }
  .image-widget:focus {
    @apply border-indigo-500;
  }
</style>
