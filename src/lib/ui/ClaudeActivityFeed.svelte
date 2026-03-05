<script lang="ts">
  import { createEventDispatcher, afterUpdate } from "svelte";
  import { CopyIcon } from "svelte-feather-icons";
  import CircleButton from "./CircleButton.svelte";
  import CircleButtons from "./CircleButtons.svelte";
  import type { WsClaudeEvent, WsWidget } from "../protocol";

  export let widget: WsWidget;
  export let collapsed: boolean = false;
  export let events: WsClaudeEvent[];
  export let claudeActive: boolean = false;
  export let transcriptPath: string | null = null;
  export let autoOpenCards: boolean = false;
  export let claudePid: string | null = null;
  export let claudePidDead: boolean = false;
  /** Session UUID — displayed in the title bar (first 8 chars) as fallback. */
  export let sessionId: string | null = null;
  /** Human-readable name derived from the first user prompt, if available. */
  export let sessionName: string | null = null;

  const dispatch = createEventDispatcher<{
    startMove: MouseEvent;
    delete: void;
    collapse: boolean;
    highlightFile: string;
    toggleAutoOpen: void;
  }>();

  let displayMode: "compact" | "full" = "full";
  let expandedEvents = new Set<number>();
  let listEl: HTMLDivElement;
  let copyFeedback = false;

  afterUpdate(() => {
    if (!collapsed && listEl) {
      listEl.scrollTop = listEl.scrollHeight;
    }
  });

  function scrollToLatest() {
    if (listEl) listEl.scrollTop = listEl.scrollHeight;
  }

  const kindIcon: Record<string, string> = {
    tool_use: "⚙",
    tool_result: "✓",
    user_message: "👤",
    assistant_message: "✦",
  };

  const kindColor: Record<string, string> = {
    tool_use: "text-yellow-300",
    tool_result: "text-green-400",
    user_message: "text-blue-300",
    assistant_message: "text-zinc-300",
  };

  function extractFilePath(event: WsClaudeEvent): string | null {
    if (event.kind !== "tool_use") return null;
    try {
      const input = JSON.parse(event.content);
      for (const key of ["path", "file_path", "filename", "file"]) {
        if (typeof input[key] === "string") return input[key];
      }
    } catch {
      const m = event.content.match(/"(?:path|file_path|filename)"\s*:\s*"([^"]+)"/);
      if (m) return m[1];
    }
    return null;
  }

  function getActionLabel(event: WsClaudeEvent): string {
    if (event.kind === "tool_use") return event.tool ?? "Tool";
    if (event.kind === "tool_result") return "Result";
    if (event.kind === "user_message") return "User";
    if (event.kind === "assistant_message") return "Claude";
    return event.kind;
  }

  function getSimpleDescription(event: WsClaudeEvent): string {
    let parsed: Record<string, unknown> | null = null;
    try {
      parsed = JSON.parse(event.content);
    } catch {
      // use raw string fallback
    }

    if (event.kind === "tool_use") {
      const tool = event.tool ?? "";
      if (["Read", "Edit", "Write"].includes(tool)) {
        const fp = (parsed?.file_path ?? parsed?.path ?? parsed?.filename) as string | undefined;
        if (fp) return fp.split("/").pop() ?? fp;
      }
      if (tool === "Bash") {
        const desc = parsed?.description as string | undefined;
        if (desc) return desc.slice(0, 60);
        const cmd = parsed?.command as string | undefined;
        if (cmd) return cmd.slice(0, 60);
      }
      if (tool === "Glob") {
        const pat = parsed?.pattern as string | undefined;
        if (pat) return pat;
      }
      if (tool === "Grep") {
        const pat = parsed?.pattern as string | undefined;
        const path = parsed?.path as string | undefined;
        if (pat) return path ? `${pat} in ${path}` : pat;
      }
      if (tool === "WebFetch") {
        const url = parsed?.url as string | undefined;
        if (url) {
          try { return new URL(url).hostname; } catch { return url.slice(0, 60); }
        }
      }
      if (tool === "WebSearch") {
        const q = parsed?.query as string | undefined;
        if (q) return q.slice(0, 60);
      }
      if (tool === "TodoWrite") {
        const todos = parsed?.todos as unknown[] | undefined;
        if (todos) return `${todos.length} todo${todos.length === 1 ? "" : "s"}`;
      }
      if (tool === "ExitPlanMode") return "Exit plan mode";
      return event.content.slice(0, 60);
    }

    if (event.kind === "tool_result") {
      const lines = (event.content.match(/\n/g)?.length ?? 0) + 1;
      return `${lines} line${lines === 1 ? "" : "s"}`;
    }

    return event.content.slice(0, 80);
  }

  function getTokenLabel(event: WsClaudeEvent): string | null {
    if (event.inputTokens == null) return null;
    return `${event.inputTokens}↑${event.outputTokens ?? 0}↓`;
  }

  function toggleExpand(i: number) {
    if (expandedEvents.has(i)) {
      expandedEvents.delete(i);
    } else {
      expandedEvents.add(i);
    }
    expandedEvents = expandedEvents;
  }

  function copyTranscriptPath() {
    if (!transcriptPath) return;
    navigator.clipboard.writeText(transcriptPath).then(() => {
      copyFeedback = true;
      setTimeout(() => (copyFeedback = false), 1500);
    });
  }
</script>

<div
  class="panel-window flex flex-col"
  style:width={collapsed ? "280px" : `${widget.w}px`}
  style:height={collapsed ? "auto" : `${widget.h}px`}
>
  <!-- Title bar -->
  <div
    class="flex select-none flex-shrink-0 cursor-grab active:cursor-grabbing"
    on:mousedown={(e) => dispatch("startMove", e)}
    on:dblclick={() => dispatch("collapse", !collapsed)}
  >
    <div class="flex items-center px-3 py-1.5 bg-zinc-800 rounded-t border-b border-zinc-700 w-full gap-2">
      <!-- Left: CircleButtons -->
      <div class="flex-1 flex items-center">
        <CircleButtons>
          <CircleButton kind="red" on:click={() => dispatch("delete")} />
          <CircleButton kind="yellow" on:click={() => dispatch("collapse", !collapsed)} />
          <CircleButton kind="green" on:click={scrollToLatest} />
        </CircleButtons>
      </div>

      <!-- Center: title -->
      <div class="w-0 flex-grow-[4] text-sm text-zinc-300 text-center font-medium overflow-hidden whitespace-nowrap text-ellipsis" title={sessionName ?? sessionId ?? undefined}>
        {#if sessionName}
          ✦ <span class="text-zinc-200">{sessionName}</span>
        {:else if sessionId}
          ✦ Claude <span class="font-mono text-xs text-indigo-300">{sessionId.slice(0, 8)}</span>
        {:else}
          ✦ Claude Activity
        {/if}
      </div>

      <!-- Right: status -->
      <div class="flex-1 flex items-center justify-end gap-1">
        {#if claudeActive}
          <span class="inline-block w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" title="Watching transcript"></span>
        {/if}
        {#if claudePid}
          <span
            class="text-xs font-mono"
            class:text-emerald-400={!claudePidDead}
            class:text-red-400={claudePidDead}
            title={claudePidDead ? `PID ${claudePid} has exited` : `claude PID ${claudePid}`}
          >
            {claudePid}{claudePidDead ? "✗" : ""}
          </span>
        {/if}
      </div>
    </div>
  </div>

  <!-- Collapsed summary: recent events as pills -->
  {#if collapsed && events.length > 0}
    {@const recent = events.slice(-10)}
    <div class="px-2 py-1.5 flex flex-wrap gap-1 bg-zinc-900 rounded-b-lg">
      {#each recent as ev}
        {#if ev.kind === "tool_use" && ev.tool}
          <span class="text-[10px] px-1.5 py-0.5 rounded bg-zinc-800 text-yellow-300 font-mono leading-none">{ev.tool}</span>
        {:else if ev.kind === "assistant_message"}
          <span class="text-[10px] px-1.5 py-0.5 rounded bg-zinc-800 text-zinc-400 leading-none">Response</span>
        {/if}
      {/each}
    </div>
  {/if}

  {#if !collapsed}
    <!-- Toggle row -->
    <div class="flex items-center gap-1.5 px-2 py-1 border-b border-zinc-800 bg-zinc-900">
      <button
        class="text-xs px-2 py-0.5 rounded-full transition-colors"
        class:bg-indigo-700={autoOpenCards}
        class:text-indigo-200={autoOpenCards}
        class:bg-zinc-700={!autoOpenCards}
        class:text-zinc-400={!autoOpenCards}
        on:click={() => dispatch("toggleAutoOpen")}
        title={autoOpenCards ? "Auto-open files: ON" : "Auto-open files: OFF"}
      >
        auto-open files
      </button>
      <button
        class="text-xs px-2 py-0.5 rounded-full transition-colors"
        class:bg-indigo-700={displayMode === "compact"}
        class:text-indigo-200={displayMode === "compact"}
        class:bg-zinc-800={displayMode === "full"}
        class:text-zinc-500={displayMode === "full"}
        on:click={() => (displayMode = displayMode === "compact" ? "full" : "compact")}
        title={displayMode === "compact" ? "Compact mode: ON (click for full)" : "Full mode (click for compact)"}
      >
        compact
      </button>
      <span class="ml-auto text-xs text-zinc-600">{events.length}</span>
    </div>

    <!-- Transcript path (debug, full mode only) -->
    {#if transcriptPath && displayMode === "full"}
      <div class="flex items-center gap-1 px-2 py-1 bg-zinc-800 border-b border-zinc-700 text-xs text-zinc-500">
        <span class="truncate flex-1 font-mono" title={transcriptPath}>{transcriptPath}</span>
        <button
          class="flex-shrink-0 p-0.5 rounded transition-colors"
          class:text-emerald-400={copyFeedback}
          class:hover:bg-zinc-600={!copyFeedback}
          on:click={copyTranscriptPath}
          title="Copy transcript path"
        >
          <CopyIcon size="11" />
        </button>
      </div>
    {/if}

    <!-- Event list -->
    <div
      class="overflow-y-auto flex-1 bg-zinc-900 rounded-b text-xs"
      bind:this={listEl}
    >
      {#if events.length === 0}
        <div class="px-3 py-4 text-zinc-500 text-center">No Claude activity yet.</div>
      {:else}
        {#each events as event, i (i)}
          {@const filePath = extractFilePath(event)}
          {@const actionLabel = getActionLabel(event)}
          {@const isExpanded = expandedEvents.has(i)}

          {@const desc = displayMode === "full" ? getSimpleDescription(event) : ""}
          {@const tokenLabel = displayMode === "full" ? getTokenLabel(event) : null}
          {#if displayMode === "compact"}
            <!-- Compact row: icon + action label only -->
            <div class="flex items-center gap-1 px-2 py-0.5 border-b border-zinc-800">
              <span class="flex-shrink-0 {kindColor[event.kind] ?? 'text-zinc-400'}">
                {kindIcon[event.kind] ?? "·"}
              </span>
              <span class="font-mono {kindColor[event.kind] ?? 'text-zinc-400'}">{actionLabel}</span>
            </div>
          {:else}
            <!-- Full row: icon + action + description + tokens, clickable to expand -->
            <!-- svelte-ignore a11y-no-static-element-interactions -->
            <div
              class="border-b border-zinc-800"
              on:click={() => { toggleExpand(i); if (filePath) dispatch("highlightFile", filePath); }}
              on:keydown={(e) => { if (e.key === "Enter") { toggleExpand(i); if (filePath) dispatch("highlightFile", filePath); } }}
              role="button"
              tabindex="0"
              title={filePath ? `Click to expand · opens card for ${filePath}` : "Click to expand"}
            >
              <div class="flex items-start gap-1 px-2 py-1 hover:bg-zinc-800 cursor-pointer">
                <span class="flex-shrink-0 {kindColor[event.kind] ?? 'text-zinc-400'}">
                  {kindIcon[event.kind] ?? "·"}
                </span>
                <div class="flex-1 min-w-0 flex items-baseline gap-1 overflow-hidden">
                  <span class="font-mono {kindColor[event.kind] ?? 'text-zinc-400'} flex-shrink-0">{actionLabel}</span>
                  {#if desc}
                    <span class="text-zinc-500 flex-shrink-0">·</span>
                    <span class="text-zinc-400 truncate min-w-0">{desc}</span>
                  {/if}
                  {#if tokenLabel}
                    <span class="text-zinc-600 text-[10px] flex-shrink-0 ml-auto pl-1">{tokenLabel}</span>
                  {/if}
                </div>
              </div>
              {#if isExpanded}
                <div class="px-2 pb-1">
                  <pre class="text-zinc-300 bg-zinc-800 rounded p-2 text-[10px] leading-relaxed max-h-48 overflow-y-auto whitespace-pre-wrap break-all">{event.content}</pre>
                </div>
              {/if}
            </div>
          {/if}
        {/each}
      {/if}
    </div>
  {/if}
</div>
