<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { WsWidget, WsIdeState, WsEditLock, WsUser } from "../protocol";
  import CircleButton from "./CircleButton.svelte";
  import CircleButtons from "./CircleButtons.svelte";

  export let widget: WsWidget;
  export let canWrite: boolean;
  /** The session name, used to construct the IDE iframe URL. */
  export let sessionName: string;
  /** Map of IDE states, keyed by Wid (widget ID). */
  export let ideStates: Map<number, WsIdeState> = new Map();
  /** Current edit lock state. */
  export let editLock: WsEditLock | null = null;
  /** Map of users, keyed by UID, for displaying names. */
  export let users: Map<number, WsUser> = new Map();
  /** The current user's UID (kept for edit lock display). */
  export let myUid: number = 0;

  const dispatch = createEventDispatcher<{
    startMove: MouseEvent;
    delete: void;
    resize: { w: number; h: number };
    maximize: number;
  }>();

  let el: HTMLDivElement;
  let iframeEl: HTMLIFrameElement;
  let minimized = false;
  let dragging = false;
  /** Store dimensions before minimize so we can restore them. */
  let preMinW = 0;
  let preMinH = 0;

  /** Open a file in the IDE by navigating the iframe to include the file path. */
  export function openFile(path: string) {
    if (!iframeEl) return;
    // Navigate the iframe to the IDE URL with the file path appended.
    // OpenVSCode Server supports /?folder=...&file=... query params.
    const base = iframeUrl.replace(/\/$/, "");
    iframeEl.src = `${base}/?file=${encodeURIComponent(path)}`;
  }

  /** User dot colors for collaborators. */
  const DOT_COLORS = [
    "bg-blue-400",
    "bg-red-400",
    "bg-green-400",
    "bg-purple-400",
    "bg-yellow-400",
    "bg-pink-400",
  ];

  $: label =
    widget.kind.type === "ideEditor"
      ? (widget.kind.workspaceLabel ?? (widget.kind as any).workspace_label ?? "IDE")
      : "IDE";
  $: ideId =
    widget.kind.type === "ideEditor"
      ? (widget.kind.ideId ?? (widget.kind as any).ide_id ?? 0)
      : 0;
  $: iframeUrl = `/ide/s/${sessionName}/${ideId}/`;

  /** Other IDE widgets with state (excluding this widget). */
  $: collaborators = (() => {
    const result: { wid: number; name: string; activeFile: string | null; colorClass: string }[] = [];
    let idx = 0;
    for (const [wid, state] of ideStates) {
      if (wid === ideId) continue; // skip self
      result.push({
        wid,
        name: `IDE ${wid}`,
        activeFile: state.activeFile,
        colorClass: DOT_COLORS[idx % DOT_COLORS.length],
      });
      idx++;
    }
    return result;
  })();

  /** Whether another user holds the edit lock. */
  $: lockHeldByOther =
    editLock &&
    editLock.holder !== null &&
    editLock.holder !== myUid &&
    editLock.expiresAt > Date.now();

  $: lockHolderName = (() => {
    if (!editLock || editLock.holder === null) return "";
    const user = users.get(editLock.holder);
    return user?.name || `User ${editLock.holder}`;
  })();

  function handleKeydown(e: KeyboardEvent) {
    if (!canWrite) return;
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      dispatch("delete");
    }
  }

  function handleMousedown(e: MouseEvent) {
    el?.focus();
    if (canWrite) {
      dragging = true;
      dispatch("startMove", e);
    }
  }

  function handleMouseup() {
    dragging = false;
  }

  function handleMaximize() {
    if (minimized) {
      minimized = false;
      dispatch("resize", { w: preMinW || 800, h: preMinH || 500 });
    }
    dispatch("maximize", ideId);
  }

  function minimize() {
    if (!minimized) {
      preMinW = widget.w;
      preMinH = widget.h;
    }
    minimized = true;
    dispatch("resize", { w: 160, h: 36 });
  }

  function restore() {
    minimized = false;
    dispatch("resize", { w: preMinW || 800, h: preMinH || 500 });
  }

  /** Forward mousemove from inside the iframe to the parent window.
   *  Same-origin access lets us listen directly on contentWindow. */
  function setupIframeCursorBridge() {
    if (!iframeEl?.contentWindow) return;
    try {
      iframeEl.contentWindow.addEventListener("mousemove", (e: MouseEvent) => {
        const rect = iframeEl.getBoundingClientRect();
        const synth = new MouseEvent("mousemove", {
          clientX: rect.left + e.clientX,
          clientY: rect.top + e.clientY,
          bubbles: true,
        });
        window.dispatchEvent(synth);
      });
    } catch {
      // Cross-origin fallback — should not happen with allow-same-origin
    }
  }
</script>

<svelte:window on:mouseup={handleMouseup} />

<!-- svelte-ignore a11y-no-noninteractive-tabindex -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div
  bind:this={el}
  class="ide-widget"
  class:minimized
  style:width="{widget.w}px"
  style:height="{widget.h}px"
  tabindex="0"
  on:keydown={handleKeydown}
  on:dblclick={() => { if (minimized) restore(); }}
>
  {#if minimized}
    <!-- Minimized pill: just a small bar with "VS Code" -->
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div
      class="flex select-none items-center justify-center w-full h-full cursor-grab active:cursor-grabbing"
      on:mousedown={handleMousedown}
    >
      <span class="text-xs text-zinc-300 font-mono">VS Code</span>
      {#if collaborators.length > 0}
        <span class="ml-1.5 flex gap-0.5">
          {#each collaborators as collab}
            <span
              class="w-2 h-2 rounded-full {collab.colorClass}"
              title="{collab.name}: {collab.activeFile || 'no file'}"
            />
          {/each}
        </span>
      {/if}
    </div>
  {:else}
    <!-- Title bar -->
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div
      class="flex select-none flex-shrink-0 cursor-grab active:cursor-grabbing items-center border-b border-zinc-700 bg-zinc-800/80"
      on:mousedown={handleMousedown}
    >
      <div class="flex-1 flex items-center px-2 py-1">
        {#if canWrite}
          <CircleButtons>
            <CircleButton
              kind="red"
              on:mousedown={(e) => e.button === 0 && dispatch("delete")}
            />
            <CircleButton
              kind="yellow"
              on:mousedown={(e) => {
                if (e.button === 0) minimize();
              }}
            />
            <CircleButton
              kind="green"
              on:mousedown={(e) => {
                if (e.button === 0) handleMaximize();
              }}
            />
          </CircleButtons>
        {/if}
      </div>
      <div
        class="w-0 flex-grow-[4] overflow-hidden whitespace-nowrap text-ellipsis text-center text-sm text-zinc-300 flex items-center justify-center gap-1.5"
      >
        <span>{label || "IDE"}</span>
        <!-- Collaborator dots -->
        {#if collaborators.length > 0}
          <span class="flex gap-0.5 items-center">
            {#each collaborators as collab}
              <span
                class="w-2 h-2 rounded-full {collab.colorClass} inline-block"
                title="{collab.name}: {collab.activeFile || 'no file'}"
              />
            {/each}
          </span>
        {/if}
        <!-- Edit lock badge -->
        {#if lockHeldByOther}
          <span
            class="text-xs text-red-400 font-mono ml-1"
            title="{lockHolderName} is editing {editLock?.file || ''}"
          >
            locked
          </span>
        {/if}
      </div>
      <div class="flex-1" />
    </div>

    <!-- IDE iframe -->
    <div class="relative flex-1 min-h-0">
      {#if dragging}
        <!-- Transparent overlay during drag to prevent iframe stealing mouse events -->
        <div class="absolute inset-0 z-10" />
      {/if}
      <iframe
        bind:this={iframeEl}
        src={iframeUrl}
        title="VS Code IDE"
        class="w-full h-full border-none bg-zinc-900"
        sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-downloads"
        on:load={setupIframeCursorBridge}
      />
    </div>
  {/if}
</div>

<style>
  .ide-widget {
    display: flex;
    flex-direction: column;
    border: 1px solid theme("colors.zinc.700");
    border-radius: theme("borderRadius.lg");
    background: theme("colors.zinc.900");
    overflow: hidden;
    opacity: 0.95;
    transition: opacity 200ms;
  }

  .ide-widget:hover,
  .ide-widget:focus-within {
    opacity: 1;
  }

  .ide-widget.minimized {
    border-radius: theme("borderRadius.md");
    background: theme("colors.zinc.800");
  }
</style>
