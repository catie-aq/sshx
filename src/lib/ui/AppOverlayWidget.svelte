<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { WsWidget } from "../protocol";
  import CircleButton from "./CircleButton.svelte";
  import CircleButtons from "./CircleButtons.svelte";

  export let widget: WsWidget;
  export let collapsed: boolean;
  export let canWrite: boolean;

  const dispatch = createEventDispatcher<{
    startMove: MouseEvent;
    delete: void;
    collapse: boolean;
    updateSettings: { url: string; allowOpenFile: boolean; allowOpenClaude: boolean };
  }>();

  $: kind = widget.kind.type === "appOverlay" ? widget.kind : null;
  $: url = kind?.url ?? "";
  $: allowOpenFile = kind?.allowOpenFile ?? true;
  $: allowOpenClaude = kind?.allowOpenClaude ?? true;

  // Derive connection status from whether a URL is set.
  $: statusColor = url ? "bg-emerald-400" : "bg-zinc-500";
  $: statusLabel = url ? "Configured" : "No URL";

  let editUrl = url;
  $: editUrl = url;

  // Build the sshx server origin from the current page URL.
  $: sshxOrigin = typeof window !== "undefined" ? window.location.origin : "";

  // Build the snippet the user should add to their app's HTML.
  $: snippet = `<!-- Add to your app's HTML (or layout.tsx) -->\n<script src="${sshxOrigin}/sshx-overlay.js"><\/script>\n<script src="${sshxOrigin}/sshx-connect.js"><\/script>\n\n<!-- Or for Next.js, set the env var: -->\nNEXT_PUBLIC_SSHX_SERVER=${sshxOrigin}`;

  let showSnippet = false;
  let copied = false;

  function commitUrl() {
    const trimmed = editUrl.trim();
    if (trimmed !== url) {
      dispatch("updateSettings", { url: trimmed, allowOpenFile, allowOpenClaude });
    }
  }

  function toggleOpenFile() {
    dispatch("updateSettings", { url, allowOpenFile: !allowOpenFile, allowOpenClaude });
  }

  function toggleOpenClaude() {
    dispatch("updateSettings", { url, allowOpenFile, allowOpenClaude: !allowOpenClaude });
  }

  function copySnippet() {
    navigator.clipboard.writeText(snippet).then(() => {
      copied = true;
      setTimeout(() => (copied = false), 2000);
    });
  }
</script>

{#if collapsed}
  <button
    class="panel-collapsed flex items-center gap-2 px-3 py-1.5 cursor-grab active:cursor-grabbing"
    on:mousedown={(e) => { if (canWrite) dispatch("startMove", e); }}
    on:dblclick={() => dispatch("collapse", false)}
  >
    <div class="w-2 h-2 rounded-full {statusColor}" />
    <span class="text-xs text-zinc-400">App</span>
  </button>
{:else}
  <div
    class="panel-window flex flex-col select-none"
    style:width="{widget.w}px"
    style:height="{widget.h}px"
  >
    <!-- Title bar -->
    <div
      class="flex flex-shrink-0 select-none cursor-grab active:cursor-grabbing"
      on:mousedown={(e) => { if (canWrite) dispatch("startMove", e); }}
    >
      <div class="flex-1 flex items-center px-3 py-1.5">
        <CircleButtons>
          <CircleButton
            kind="red"
            on:mousedown={(e) => { if (e.button === 0) dispatch("collapse", true); }}
          />
          <CircleButton
            kind="yellow"
            on:mousedown={(e) => { if (e.button === 0) dispatch("collapse", true); }}
          />
        </CircleButtons>
      </div>
      <div class="py-2 text-sm text-zinc-300 text-center font-medium w-0 flex-grow-[4]">
        App
      </div>
      <div class="flex-1" />
    </div>

    <!-- Body -->
    <div class="flex-1 min-h-0 px-3 pb-3 flex flex-col gap-2.5 overflow-auto">
      <!-- Status -->
      <div class="flex items-center gap-2">
        <div class="w-2 h-2 rounded-full flex-shrink-0 {statusColor}" />
        <span class="text-xs text-zinc-400">{statusLabel}</span>
      </div>

      <!-- URL input -->
      <div class="flex flex-col gap-1">
        <label for="app-url" class="text-[11px] text-zinc-500 uppercase tracking-wide">App URL</label>
        <input
          id="app-url"
          type="text"
          class="url-input"
          placeholder="http://localhost:3000"
          bind:value={editUrl}
          on:blur={commitUrl}
          on:keydown={(e) => { if (e.key === "Enter") commitUrl(); }}
          disabled={!canWrite}
        />
      </div>

      <!-- Toggles -->
      <label class="toggle-row">
        <input type="checkbox" checked={allowOpenFile} on:change={toggleOpenFile} disabled={!canWrite} />
        <span class="text-xs text-zinc-300">Allow file opening</span>
      </label>
      <label class="toggle-row">
        <input type="checkbox" checked={allowOpenClaude} on:change={toggleOpenClaude} disabled={!canWrite} />
        <span class="text-xs text-zinc-300">Allow Claude Code opening</span>
      </label>

      <!-- Setup snippet -->
      <div class="border-t border-zinc-700 pt-2 mt-1">
        <button
          class="text-[11px] text-indigo-400 hover:text-indigo-300 transition-colors"
          on:click={() => (showSnippet = !showSnippet)}
        >
          {showSnippet ? "Hide" : "Show"} setup snippet
        </button>
        {#if showSnippet}
          <div class="mt-1.5 relative">
            <pre class="snippet-box">{snippet}</pre>
            <button class="copy-btn" on:click={copySnippet}>
              {copied ? "Copied!" : "Copy"}
            </button>
            <p class="text-[10px] text-zinc-500 mt-1">
              Add these script tags to your app's HTML to enable the overlay.
            </p>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style lang="postcss">
  .panel-window {
    @apply inline-flex rounded-lg border border-zinc-700 bg-zinc-800 opacity-90;
    transition: opacity 200ms;
  }
  .panel-window:hover {
    @apply opacity-100;
  }
  .panel-collapsed {
    @apply inline-flex rounded-lg border border-zinc-700 bg-zinc-800 opacity-80;
    transition: opacity 200ms;
  }
  .panel-collapsed:hover {
    @apply opacity-100;
  }
  .url-input {
    @apply w-full px-2 py-1 text-xs rounded border border-zinc-600 bg-zinc-900 text-zinc-200
           placeholder-zinc-500 focus:outline-none focus:border-indigo-500;
  }
  .toggle-row {
    @apply flex items-center gap-2 cursor-pointer;
  }
  .toggle-row input[type="checkbox"] {
    @apply w-3.5 h-3.5 rounded border-zinc-600 bg-zinc-700 text-indigo-500
           focus:ring-0 focus:ring-offset-0 cursor-pointer;
  }
  .snippet-box {
    @apply text-[10px] leading-relaxed p-2 rounded bg-zinc-900 border border-zinc-700
           text-zinc-300 font-mono whitespace-pre-wrap break-all overflow-x-auto;
  }
  .copy-btn {
    @apply absolute top-1 right-1 text-[10px] px-1.5 py-0.5 rounded
           bg-zinc-700 text-zinc-300 hover:bg-zinc-600 transition-colors;
  }
</style>
