<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import {
    ActivityIcon,
    CastIcon,
    CodeIcon,
    EditIcon,
    FileTextIcon,
    FolderIcon,
    GlobeIcon,
    MessageSquareIcon,
    MonitorIcon,
    PlayCircleIcon,
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
  export let workspaceOpen: boolean = false;
  export let claudeInstances: Map<string, { events: unknown[]; transcriptPath: string | null; sessionName: string | null; widgetName: string | null; fileMtime: number | null; closed: boolean }> = new Map();
  export let claudeActive: boolean = false;

  export let isSharing: boolean = false;
  export let appOverlayOpen: boolean = false;
  export let textToolActive: boolean = false;
  export let drawingTool: "pencil" | "highlighter" | null = null;
  export let drawingColor: string = "#ffffff";
  /** Number of video streams dismissed (hidden) by the user. */
  export let hiddenStreamCount: number = 0;

  /** Major mode: "none" | "edition" | "slides" — only one active at a time. */
  export let majorMode: "none" | "edition" | "slides" = "none";

  /** Whether the CLI client has IDE (OpenVSCode Server) support available. */
  export let ideAvailable: boolean = false;
  /** Active IDE editor widgets: [wid, label]. */
  export let ideEditors: [number, string][] = [];

  const dispatch = createEventDispatcher<{
    create: void;
    chat: void;
    settings: void;
    networkInfo: void;
    createNote: void;
    toggleTextTool: void;
    toggleDrawingTool: "pencil" | "highlighter" | null;
    colorChange: string;
    toggleWorkspace: void;
    openClaudeInstance: string;
    resumeClaudeInTerminal: string;
    search: void;
    startScreenShare: void;
    stopScreenShare: void;
    showStreams: void;
    toggleAppOverlay: void;
    openIde: void;
    focusIde: number;
    majorModeChange: "none" | "edition" | "slides";
  }>();

  const DRAWING_COLORS = [
    "#ffffff", "#a1a1aa", "#fde047", "#fb923c",
    "#f87171", "#4ade80", "#38bdf8", "#c084fc", "#ec4899",
  ];

  let claudeDropdownOpen = false;
  let ideDropdownOpen = false;

  function handleWindowClick() {
    if (ideDropdownOpen) ideDropdownOpen = false;
  }

  function setMajorMode(mode: "none" | "edition" | "slides") {
    if (majorMode === mode) {
      dispatch("majorModeChange", "none");
    } else {
      dispatch("majorModeChange", mode);
    }
  }
</script>

<svelte:window on:click={handleWindowClick} />

<div class="toolbar-container">
  <!-- Primary bar -->
  <div class="panel inline-block px-3 py-2">
    <div class="flex items-center select-none">
      <a href="/" class="flex-shrink-0"
        ><img src={logo} alt="sshx logo" class="h-10" /></a
      >
      <p class="ml-1.5 mr-2 font-medium">sshx</p>

      <div class="v-divider" />

      <!-- Group 1: Windowed elements -->
      <div class="flex space-x-1">
        <!-- Terminal: new terminal -->
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
          <TerminalIcon strokeWidth={1.5} class="p-0.5" />
        </button>

        <!-- Edition mode toggle -->
        <button
          class="icon-button"
          class:mode-active={majorMode === "edition"}
          on:click={() => setMajorMode("edition")}
          disabled={!connected}
          title={majorMode === "edition" ? "Exit edition mode" : "Edition mode"}
        >
          <EditIcon strokeWidth={1.5} class="p-0.5" />
        </button>

        <!-- IDE editor (OpenVSCode Server) -->
        <div class="relative">
          <button
            class="icon-button"
            on:click|stopPropagation={() => { if (ideAvailable) ideDropdownOpen = !ideDropdownOpen; }}
            disabled={!connected || hasWriteAccess === false || !ideAvailable}
            title={ideAvailable ? "VS Code editors" : "IDE not available (start sshx with --ide to enable)"}
          >
            <CodeIcon strokeWidth={1.5} class="p-0.5" />
            {#if ideEditors.length > 0}
              <span class="ide-badge">{ideEditors.length}</span>
            {/if}
          </button>
          {#if ideDropdownOpen && ideAvailable}
            <!-- svelte-ignore a11y-click-events-have-key-events -->
            <!-- svelte-ignore a11y-no-static-element-interactions -->
            <div class="ide-dropdown" on:click|stopPropagation>
              {#if ideEditors.length > 0}
                {#each ideEditors as [wid, editorLabel]}
                  <button
                    class="ide-dropdown-item"
                    on:click={() => { dispatch("focusIde", wid); ideDropdownOpen = false; }}
                  >
                    <MonitorIcon size="14" strokeWidth={1.5} />
                    <span class="truncate">{editorLabel || "VS Code"}</span>
                  </button>
                {/each}
                <div class="border-t border-zinc-700 my-1" />
              {/if}
              <button
                class="ide-dropdown-item text-emerald-400"
                on:click={() => { dispatch("openIde"); ideDropdownOpen = false; }}
              >
                <PlusCircleIcon size="14" strokeWidth={1.5} />
                <span>New editor</span>
              </button>
            </div>
          {/if}
        </div>
      </div>

      <div class="v-divider" />

      <!-- Group 2: Information elements -->
      <div class="flex space-x-1">
        <!-- Search -->
        <button class="icon-button" on:click={() => dispatch("search")} title="Search (Ctrl+K)">
          <SearchIcon strokeWidth={1.5} class="p-0.5" />
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

        <!-- Workspace toggle -->
        <button
          class="icon-button"
          class:workspace-active={workspaceOpen}
          on:click={() => dispatch("toggleWorkspace")}
          disabled={!connected}
          title={workspaceOpen ? "Close workspace" : "Open workspace"}
        >
          <FolderIcon strokeWidth={1.5} class="p-0.5" />
        </button>

      </div>

      <div class="v-divider" />

      <!-- Group 3: Social elements -->
      <div class="flex space-x-1">
        <!-- Chat -->
        <button class="icon-button" on:click={() => dispatch("chat")} title="Chat">
          <MessageSquareIcon strokeWidth={1.5} class="p-0.5" />
          {#if newMessages}
            <div class="activity" />
          {/if}
        </button>

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

        <!-- Restore hidden streams -->
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

        <!-- Slides mode toggle -->
        <button
          class="icon-button"
          class:mode-active={majorMode === "slides"}
          on:click={() => setMajorMode("slides")}
          disabled={!connected}
          title={majorMode === "slides" ? "Exit slideshow mode" : "Slideshow mode"}
        >
          <PlayCircleIcon strokeWidth={1.5} class="p-0.5" />
        </button>
      </div>

      <div class="v-divider" />

      <!-- Group 4: Settings / Network -->
      <div class="flex space-x-1">
        <button class="icon-button" on:click={() => dispatch("settings")} title="Settings">
          <SettingsIcon strokeWidth={1.5} class="p-0.5" />
        </button>
        <button class="icon-button" on:click={() => dispatch("networkInfo")} title="Network info">
          <WifiIcon strokeWidth={1.5} class="p-0.5" />
        </button>
      </div>
    </div>
  </div>

  <!-- Second bar: Edition tools (visible when edition mode is active) -->
  {#if majorMode === "edition"}
    <div class="panel inline-block px-3 py-1.5 mt-1">
      <div class="flex items-center space-x-1 select-none">
        <!-- Text tool (T) -->
        <button
          class="sub-button"
          class:sub-active={textToolActive}
          on:click={() => dispatch("toggleTextTool")}
          disabled={!connected || !hasWriteAccess}
          title={textToolActive ? "Cancel text tool (Esc)" : "Text (T)"}
        >
          <span class="text-sm font-bold leading-none px-0.5">T</span>
          <span class="sub-label">Text</span>
        </button>

        <!-- Sticky note (N) -->
        <button
          class="sub-button"
          on:click={() => dispatch("createNote")}
          disabled={!connected || !hasWriteAccess}
          title="Add sticky note (N)"
        >
          <FileTextIcon strokeWidth={1.5} size="14" />
          <span class="sub-label">Note</span>
        </button>

        <div class="v-divider-sm" />

        <!-- Pencil (L) -->
        <button
          class="sub-button"
          class:sub-active={drawingTool === "pencil"}
          on:click={() => dispatch("toggleDrawingTool", drawingTool === "pencil" ? null : "pencil")}
          disabled={!connected || !hasWriteAccess}
          title={drawingTool === "pencil" ? "Stop drawing" : "Pencil (L)"}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
            <path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z"/>
          </svg>
          <span class="sub-label">Pencil</span>
        </button>

        <!-- Highlighter (H) -->
        <button
          class="sub-button"
          class:sub-active={drawingTool === "highlighter"}
          on:click={() => dispatch("toggleDrawingTool", drawingTool === "highlighter" ? null : "highlighter")}
          disabled={!connected || !hasWriteAccess}
          title={drawingTool === "highlighter" ? "Stop highlighting" : "Highlighter (H)"}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">
            <path d="M12 20h9"/><path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z"/>
          </svg>
          <span class="sub-label">Highlight</span>
        </button>

        <!-- Inline color dots (visible when a drawing tool is active) -->
        {#if drawingTool}
          <div class="v-divider-sm" />
          {#each DRAWING_COLORS as c}
            <button
              class="drawing-color-dot"
              class:active={drawingColor === c}
              style:background-color={c}
              on:click={() => dispatch("colorChange", c)}
            />
          {/each}
        {/if}
      </div>
    </div>
  {/if}
</div>

<style lang="postcss">
  .toolbar-container {
    @apply flex flex-col items-center;
  }

  .v-divider {
    @apply h-5 mx-2 border-l-4 border-zinc-800;
  }

  .v-divider-sm {
    @apply h-4 mx-1 border-l-2 border-zinc-700;
  }

  .icon-button {
    @apply relative rounded-md p-1 hover:bg-zinc-700 active:bg-indigo-700 transition-colors;
    @apply disabled:opacity-50 disabled:bg-transparent;
  }

  .mode-active {
    @apply bg-indigo-800 text-indigo-200 hover:bg-indigo-700;
  }

  .workspace-active {
    @apply bg-indigo-800 text-indigo-200 hover:bg-indigo-700;
  }

  .claude-active {
    @apply bg-emerald-900 text-emerald-200 hover:bg-emerald-800;
  }

  .share-active {
    @apply bg-red-900 text-red-200 hover:bg-red-800;
  }

  .app-active {
    @apply bg-purple-900 text-purple-200 hover:bg-purple-800;
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

  .ide-badge {
    @apply absolute -top-1 -right-1 text-[10px] leading-none px-[4px] py-[2px]
           bg-indigo-500 text-white rounded-full pointer-events-none;
  }

  .ide-dropdown {
    @apply absolute top-full left-1/2 -translate-x-1/2 mt-2
           bg-zinc-800 border border-zinc-700 rounded-lg shadow-lg
           py-1 min-w-[160px] z-50;
  }

  .ide-dropdown-item {
    @apply flex items-center gap-2 w-full px-3 py-1.5 text-sm text-zinc-300
           hover:bg-zinc-700 transition-colors text-left;
  }

  .claude-dot {
    @apply bg-emerald-400 animate-pulse;
  }

  /* Sub-bar buttons */
  .sub-button {
    @apply flex items-center gap-1 px-2 py-1 rounded-md text-zinc-300 hover:bg-zinc-700 transition-colors text-xs;
    @apply disabled:opacity-50 disabled:bg-transparent;
  }

  .sub-active {
    @apply bg-zinc-600 text-white;
  }

  .sub-label {
    @apply text-[11px] leading-none;
  }

  .drawing-color-dot {
    @apply w-3.5 h-3.5 rounded-full border border-zinc-600 hover:scale-125 transition-transform cursor-pointer;
  }

  .drawing-color-dot.active {
    @apply ring-2 ring-white ring-offset-1 ring-offset-zinc-900;
  }
</style>
