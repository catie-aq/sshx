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
  }>();

  $: imageUrl = widget.kind.type === "image" ? widget.kind.url : "";
  $: imageAlt = widget.kind.type === "image" ? widget.kind.alt : "";
  $: title = imageAlt || imageUrl.split("/").pop()?.replace(/^[a-z0-9]{8}_/, "") || "Image";
</script>

<div
  class="panel-window flex flex-col select-none"
  style:width="{widget.w}px"
  style:height="{widget.h}px"
>
  <!-- Title bar / drag handle -->
  <div
    class="flex flex-shrink-0 select-none cursor-grab active:cursor-grabbing"
    on:mousedown={(e) => { if (canWrite) dispatch("startMove", e); }}
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
      class="py-2 text-sm text-zinc-300 text-center font-medium overflow-hidden whitespace-nowrap text-ellipsis w-0 flex-grow-[4] font-mono"
    >
      {title}
    </div>
    <div class="flex-1" />
  </div>

  <!-- Image body -->
  <div class="flex-1 min-h-0 bg-zinc-900 rounded-b-lg overflow-hidden">
    {#if imageUrl}
      <img
        src={imageUrl}
        alt={imageAlt}
        class="w-full h-full object-contain"
        draggable="false"
      />
    {/if}
  </div>
</div>

<style lang="postcss">
  .panel-window {
    @apply inline-flex rounded-lg border border-zinc-700 bg-zinc-800 opacity-90;
    transition: opacity 200ms;
  }
  .panel-window:hover {
    @apply opacity-100;
  }
</style>
