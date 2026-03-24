<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import {
    ActivityIcon,
    CastIcon,
    FileTextIcon,
    FolderIcon,
    GitBranchIcon,
    GlobeIcon,
    MessageSquareIcon,
    MonitorIcon,
    PlusCircleIcon,
    SearchIcon,
    SettingsIcon,
    TerminalIcon,
    WifiIcon,
  } from "svelte-feather-icons";
  import ClaudeInstanceList from "./ClaudeInstanceList.svelte";

  import logo from "$lib/assets/logo.svg";

  export let connected: boolean;
  export let hasWriteAccess: boolean | undefined;
  export let newMessages: boolean;
  export let mode: "terminal" | "creative" = "terminal";
  export let workspaceOpen: boolean = false;
  export let claudeInstances: Map<string, { events: unknown[]; transcriptPath: string | null; sessionName: string | null; widgetName: string | null; fileMtime: number | null; closed: boolean }> = new Map();
  export let claudeActive: boolean = false;
  export let graphMode: boolean = false;

  export let isSharing: boolean = false;
  export let appOverlayOpen: boolean = false;
  export let textToolActive: boolean = false;
  /** Number of video streams dismissed (hidden) by the user. */
  export let hiddenStreamCount: number = 0;

  const dispatch = createEventDispatcher<{
    create: void;
    chat: void;
    settings: void;
    networkInfo: void;
    modeChange: "terminal" | "creative";
    createNote: void;
    toggleTextTool: void;
    toggleWorkspace: void;
    openClaudeInstance: string;
    resumeClaudeInTerminal: string;
    openGraphView: void;
    search: void;
    startScreenShare: void;
    stopScreenShare: void;
    showStreams: void;
    toggleAppOverlay: void;
  }>();

  let claudeDropdownOpen = false;
</script>

<div class="panel inline-block px-3 py-2">
  <div class="flex items-center select-none">
    <a href="/" class="flex-shrink-0"
      ><img src={logo} alt="sshx logo" class="h-10" /></a
    >
    <p class="ml-1.5 mr-2 font-medium">sshx</p>

    <div class="v-divider" />

    <!-- Mode tabs — icon only -->
    <div class="flex space-x-1 mr-1">
      <button
        class="mode-tab"
        class:active={mode === "terminal"}
        title="Terminal"
        on:click={() => dispatch("modeChange", "terminal")}
      >
        <TerminalIcon strokeWidth={1.5} size="15" />
      </button>
      <button
        class="mode-tab"
        class:active={mode === "creative"}
        title="Notes"
        on:click={() => dispatch("modeChange", "creative")}
      >
        <FileTextIcon strokeWidth={1.5} size="15" />
      </button>
    </div>

    <div class="v-divider" />

    <div class="flex space-x-1">
      <!-- Mode action button -->
      {#if mode === "terminal"}
        <button
          class="icon-button"
          on:click={() => dispatch("create")}
          disabled={!connected || !hasWriteAccess}
          title={!connected
            ? "Not connected"
            : hasWriteAccess === false
            ? "No write access"
            : "New terminal"}
        >
          <PlusCircleIcon strokeWidth={1.5} class="p-0.5" />
        </button>
      {:else if mode === "creative"}
        <button
          class="icon-button"
          on:click={() => dispatch("createNote")}
          disabled={!connected || !hasWriteAccess}
          title={!connected
            ? "Not connected"
            : hasWriteAccess === false
            ? "No write access"
            : "Add sticky note"}
        >
          <PlusCircleIcon strokeWidth={1.5} class="p-0.5" />
        </button>
        <button
          class="icon-button"
          class:text-tool-active={textToolActive}
          on:click={() => dispatch("toggleTextTool")}
          disabled={!connected || !hasWriteAccess}
          title={textToolActive ? "Cancel text tool (Esc)" : "Add text block (T)"}
        >
          <span class="text-sm font-bold leading-none px-0.5">T</span>
        </button>
      {/if}

      <!-- Workspace toggle — independent of mode -->
      <button
        class="icon-button"
        class:workspace-active={workspaceOpen}
        on:click={() => dispatch("toggleWorkspace")}
        disabled={!connected}
        title={workspaceOpen ? "Close workspace" : "Open workspace"}
      >
        <FolderIcon strokeWidth={1.5} class="p-0.5" />
      </button>

      <!-- Claude activity dropdown toggle -->
      <div class="relative">
        <button
          class="icon-button"
          class:claude-active={claudeDropdownOpen}
          on:click={() => (claudeDropdownOpen = !claudeDropdownOpen)}
          disabled={!connected}
          title={claudeDropdownOpen ? "Close Claude sessions" : "Claude sessions"}
        >
          <ActivityIcon strokeWidth={1.5} class="p-0.5" />
          {#if claudeActive}
            <div class="activity claude-dot" />
          {/if}
        </button>
        {#if claudeDropdownOpen}
          <ClaudeInstanceList
            instances={claudeInstances}
            on:openInstance={({ detail: sid }) => {
              dispatch("openClaudeInstance", sid);
              claudeDropdownOpen = false;
            }}
            on:resumeInTerminal={({ detail: sid }) => {
              dispatch("resumeClaudeInTerminal", sid);
              claudeDropdownOpen = false;
            }}
            on:close={() => (claudeDropdownOpen = false)}
          />
        {/if}
      </div>

      <!-- Screen share toggle -->
      <button
        class="icon-button"
        class:share-active={isSharing}
        on:click={() => isSharing ? dispatch("stopScreenShare") : dispatch("startScreenShare")}
        disabled={!connected || hasWriteAccess === false}
        title={isSharing ? "Stop screen share" : "Share your screen"}
      >
        <CastIcon strokeWidth={1.5} class="p-0.5" />
      </button>

      <!-- App overlay toggle -->
      <button
        class="icon-button"
        class:app-active={appOverlayOpen}
        on:click={() => dispatch("toggleAppOverlay")}
        disabled={!connected}
        title={appOverlayOpen ? "App overlay open" : "App overlay"}
      >
        <GlobeIcon strokeWidth={1.5} class="p-0.5" />
      </button>

      <!-- Restore hidden streams (visible only when streams are dismissed) -->
      {#if hiddenStreamCount > 0}
        <button
          class="icon-button streams-hidden"
          on:click={() => dispatch("showStreams")}
          title="{hiddenStreamCount} hidden stream{hiddenStreamCount > 1 ? 's' : ''} — click to show"
        >
          <MonitorIcon strokeWidth={1.5} class="p-0.5" />
          <span class="stream-badge">{hiddenStreamCount}</span>
        </button>
      {/if}

      <!-- Graph mode toggle -->
      <button
        class="icon-button"
        class:graph-active={graphMode}
        on:click={() => dispatch("openGraphView")}
        disabled={!connected}
        title={graphMode ? "Graph mode ON — click to disable" : "Graph mode — show import connections"}
      >
        <GitBranchIcon strokeWidth={1.5} class="p-0.5" />
      </button>

      <button class="icon-button" on:click={() => dispatch("search")} title="Search (Ctrl+K)">
        <SearchIcon strokeWidth={1.5} class="p-0.5" />
      </button>
      <button class="icon-button" on:click={() => dispatch("chat")}>
        <MessageSquareIcon strokeWidth={1.5} class="p-0.5" />
        {#if newMessages}
          <div class="activity" />
        {/if}
      </button>
      <button class="icon-button" on:click={() => dispatch("settings")}>
        <SettingsIcon strokeWidth={1.5} class="p-0.5" />
      </button>
    </div>

    <div class="v-divider" />

    <div class="flex space-x-1">
      <button class="icon-button" on:click={() => dispatch("networkInfo")}>
        <WifiIcon strokeWidth={1.5} class="p-0.5" />
      </button>
    </div>
  </div>
</div>

<style lang="postcss">
  .v-divider {
    @apply h-5 mx-2 border-l-4 border-zinc-800;
  }

  .icon-button {
    @apply relative rounded-md p-1 hover:bg-zinc-700 active:bg-indigo-700 transition-colors;
    @apply disabled:opacity-50 disabled:bg-transparent;
  }

  .workspace-active {
    @apply bg-indigo-800 text-indigo-200 hover:bg-indigo-700;
  }

  .claude-active {
    @apply bg-emerald-900 text-emerald-200 hover:bg-emerald-800;
  }

  .graph-active {
    @apply bg-cyan-900 text-cyan-200 hover:bg-cyan-800;
  }

  .share-active {
    @apply bg-red-900 text-red-200 hover:bg-red-800;
  }

  .app-active {
    @apply bg-purple-900 text-purple-200 hover:bg-purple-800;
  }

  .text-tool-active {
    @apply bg-amber-900 text-amber-200 hover:bg-amber-800;
  }

  .streams-hidden {
    @apply bg-zinc-700 text-zinc-200 hover:bg-indigo-700;
  }

  .stream-badge {
    @apply absolute -top-1 -right-1 text-[10px] leading-none px-[4px] py-[2px]
           bg-indigo-500 text-white rounded-full pointer-events-none;
  }

  .activity {
    @apply absolute top-1 right-0.5 text-xs p-[4.5px] bg-red-500 rounded-full;
  }

  .claude-dot {
    @apply bg-emerald-400 animate-pulse;
  }

  .mode-tab {
    @apply flex items-center px-2 py-1 rounded-md text-zinc-400 hover:bg-zinc-700 transition-colors;
  }

  .mode-tab.active {
    @apply bg-zinc-700 text-white border-b-2 border-indigo-400;
  }
</style>
