<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { EditIcon, CheckIcon, UploadCloudIcon, XCircleIcon } from "svelte-feather-icons";
  import type * as MonacoType from "monaco-editor";
  import type { WsWidget, WsSourceFile, WsFileMetadataUpdate } from "../protocol";
  import CircleButton from "./CircleButton.svelte";
  import CircleButtons from "./CircleButtons.svelte";
  import { Carta, CartaEditor, CartaViewer } from "carta-md";

  const carta = new Carta({});

  export let widget: WsWidget;
  export let file: WsSourceFile | null;
  export let canWrite: boolean;
  export let sessionId: string = "";
  export let ideAvailable: boolean = false;
  export let ideEditors: [number, string][] = [];

  const dispatch = createEventDispatcher<{
    startMove: MouseEvent;
    delete: void;
    openFile: string;
    openLibrary: string;
    describe: string;
    updateMetadata: { path: string; update: WsFileMetadataUpdate };
    openInClaude: { path: string; question: string };
    collapse: boolean;
    loadImports: void;
    loadDependents: void;
    openInVSCode: { path: string; wid?: number };
  }>();

  export let collapsed: boolean = false;
  export let highlighted: boolean = false;

  /** Which export name is currently expanded to show its importers. */
  let selectedExport: string | null = null;

  let describing = false;
  let importedByExpanded = false;

  // Description editing state
  let editingDescription = false;
  let editDescriptionText = "";

  // Ask Claude state
  let askingClaude = false;
  let claudeQuestion = "";

  // Image drag-over state
  let dragOver = false;

  // Image name input state
  let imageName = "";

  // Monaco editor state
  let editorInstance: MonacoType.editor.IStandaloneCodeEditor | null = null;
  let editorFontSize = 12;
  let selectedLines: { start: number; end: number } | null = null;
  let editorAskingClaude = false;
  let editorClaudeQuestion = "";

  function changeFontSize(delta: number) {
    editorFontSize = Math.max(8, Math.min(28, editorFontSize + delta));
    editorInstance?.updateOptions({ fontSize: editorFontSize });
  }

  function submitEditorQuestion() {
    if (!editorClaudeQuestion.trim() || !file) return;
    const ctx = selectedLines
      ? `[Lines ${selectedLines.start}–${selectedLines.end}] `
      : "";
    dispatch("openInClaude", { path: file.path, question: ctx + editorClaudeQuestion.trim() });
    editorClaudeQuestion = "";
    editorAskingClaude = false;
  }

  function getLanguage(path: string): string {
    const ext = path.split(".").pop()?.toLowerCase() ?? "";
    const langMap: Record<string, string> = {
      ts: "typescript", tsx: "typescript",
      js: "javascript", jsx: "javascript",
      svelte: "html", rs: "rust", py: "python",
      json: "json", md: "markdown", css: "css",
      html: "html", yml: "yaml", yaml: "yaml",
      proto: "protobuf", toml: "ini", sh: "shell",
      go: "go", java: "java", c: "c", cpp: "cpp",
      cs: "csharp", rb: "ruby", php: "php",
      scss: "scss", less: "less", graphql: "graphql",
    };
    return langMap[ext] ?? "plaintext";
  }

  function mountEditor(node: HTMLDivElement) {
    let cancelled = false;

    (async () => {
      if (typeof window !== "undefined" && !(window as any).__monacoEnvSet) {
        (window as any).__monacoEnvSet = true;
        const [{ default: EditorWorker }, { default: TsWorker }] = await Promise.all([
          import("monaco-editor/esm/vs/editor/editor.worker?worker"),
          import("monaco-editor/esm/vs/language/typescript/ts.worker?worker"),
        ]);
        (window as any).MonacoEnvironment = {
          getWorker: function (_: unknown, label: string) {
            if (label === "typescript" || label === "javascript") {
              return new TsWorker();
            }
            return new EditorWorker();
          },
        };
      }
      const monaco = await import("monaco-editor");
      if (cancelled) return;
      editorInstance = monaco.editor.create(node, {
        value: file?.content ?? "",
        language: getLanguage(file?.path ?? ""),
        theme: "vs-dark",
        readOnly: true,
        minimap: { enabled: true, side: "right" },
        scrollBeyondLastLine: false,
        automaticLayout: true,
        fontSize: editorFontSize,
        fontFamily: '"Fira Code VF", monospace',
        lineNumbers: "on",
        folding: true,
        renderLineHighlight: "line",
        overviewRulerLanes: 0,
        scrollbar: {
          verticalScrollbarSize: 6,
          horizontalScrollbarSize: 6,
          verticalSliderSize: 4,
          horizontalSliderSize: 4,
        },
      });
      editorInstance.onDidChangeCursorSelection(() => {
        const sel = editorInstance!.getSelection();
        if (sel && !sel.isEmpty()) {
          selectedLines = { start: sel.startLineNumber, end: sel.endLineNumber };
        } else {
          selectedLines = null;
        }
      });
    })();

    return {
      destroy() {
        cancelled = true;
        editorInstance?.dispose();
        editorInstance = null;
        selectedLines = null;
      },
    };
  }

  // Clear loading state when description arrives.
  $: if (file?.description) describing = false;

  // Use saved widget dimensions if available, overriding the default.
  $: effectiveW = file?.widgetW && file.widgetW > 0 ? file.widgetW : widget.w;
  $: effectiveH = file?.widgetH && file.widgetH > 0 ? file.widgetH : widget.h;

  const kindColors: Record<string, string> = {
    component: "bg-blue-800 text-blue-100",
    hook: "bg-purple-800 text-purple-100",
    utility: "bg-zinc-700 text-zinc-100",
    config: "bg-yellow-800 text-yellow-100",
    type: "bg-green-800 text-green-100",
    other: "bg-zinc-700 text-zinc-200",
  };

  // Color for line count display
  $: sizeColor = !file ? "text-zinc-500"
    : file.lineCount < 100 ? "text-emerald-400"
    : file.lineCount < 500 ? "text-indigo-400"
    : file.lineCount < 1500 ? "text-yellow-400"
    : "text-red-400";

  $: barColor = !file ? "bg-zinc-600"
    : file.lineCount < 100 ? "bg-emerald-500"
    : file.lineCount < 500 ? "bg-indigo-500"
    : file.lineCount < 1500 ? "bg-yellow-500"
    : "bg-red-500";

  $: filename = file?.path?.split("/").pop() ?? (widget.kind.type === "fileCard" ? widget.kind.path.split("/").pop() : "file") ?? "file";

  function startDescriptionEdit() {
    if (!file) return;
    editDescriptionText = file.description;
    editingDescription = true;
  }

  function saveDescription() {
    if (!file) return;
    const newDesc = editDescriptionText.trim();
    editingDescription = false;
    if (newDesc !== file.description) {
      dispatch("updateMetadata", { path: file.path, update: { description: newDesc } });
    }
  }

  function cancelDescriptionEdit() {
    editingDescription = false;
  }

  async function uploadImage(fileBlob: File) {
    if (!file || !sessionId) return;
    const formData = new FormData();
    formData.append("file", fileBlob);
    try {
      const res = await fetch(`/api/s/${sessionId}/upload`, {
        method: "POST",
        body: formData,
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const { url } = await res.json();
      const origName = fileBlob.name || "image.png";
      dispatch("updateMetadata", { path: file.path, update: { imagePath: url, imageName: origName } });
      imageName = origName;
    } catch (e) {
      console.error("Image upload failed:", e);
    }
  }

  function handleFileInput(event: Event) {
    const input = event.target as HTMLInputElement;
    const picked = input?.files?.[0];
    if (picked) uploadImage(picked);
  }

  function handleDrop(event: DragEvent) {
    event.preventDefault();
    dragOver = false;
    const dropped = event.dataTransfer?.files?.[0];
    if (dropped && dropped.type.startsWith("image/")) {
      uploadImage(dropped);
    }
  }

  function clearImage() {
    if (!file) return;
    dispatch("updateMetadata", { path: file.path, update: { imagePath: "" } });
  }

  function submitImageName() {
    if (!file || !imageName.trim()) return;
    dispatch("updateMetadata", { path: file.path, update: { imageName: imageName.trim() } });
  }

  let showCode = true;

  function handleOpenInVSCode() {
    if (!file || !ideAvailable) return;
    if (ideEditors.length === 1) {
      dispatch("openInVSCode", { path: file.path, wid: ideEditors[0][0] });
    } else if (ideEditors.length > 1) {
      dispatch("openInVSCode", { path: file.path });
    }
  }
</script>

<div
  class="panel-window flex flex-col select-none"
  class:highlighted
  style:width={collapsed ? "300px" : `${effectiveW}px`}
  style:height={collapsed ? "auto" : `${effectiveH}px`}
>
  <!-- Header bar -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div
    class="flex select-none flex-shrink-0 cursor-grab active:cursor-grabbing"
    on:mousedown={(e) => dispatch("startMove", e)}
    on:dblclick={() => dispatch("collapse", !collapsed)}
  >
    <div class="flex-1 flex items-center px-3 py-1.5">
      <CircleButtons>
        <CircleButton
          kind="red"
          on:mousedown={(event) => event.button === 0 && dispatch("delete")}
        />
      </CircleButtons>
    </div>
    <div
      class="py-2 text-sm text-zinc-300 text-center font-medium overflow-hidden whitespace-nowrap text-ellipsis w-0 flex-grow-[4] font-mono"
    >
      File · {filename}
    </div>
    <div class="flex-1 flex items-center justify-end gap-2 pr-2">
      {#if !collapsed}
        <!-- svelte-ignore a11y-no-static-element-interactions -->
        <button
          class="text-zinc-500 hover:text-zinc-300 text-[11px] px-1 py-0.5 rounded hover:bg-zinc-700 transition-colors leading-none font-mono"
          on:click|stopPropagation={() => (showCode = !showCode)}
          on:mousedown|stopPropagation
          title="{showCode ? 'Hide code' : 'Show code'}"
        >{showCode ? '‹/›' : '</>'}</button>
      {/if}
      <span class="text-zinc-500 text-[10px]">{collapsed ? '▴' : '▾'}</span>
    </div>
  </div>

  {#if collapsed}
    <div class="px-3 py-2 bg-zinc-900 rounded-b-lg">
      {#if file?.description}
        <p class="text-sm text-zinc-300 leading-snug description-clamp">
          {file.description.replace(/[#*`_[\]]/g, '').slice(0, 200)}
        </p>
      {:else if file}
        <p class="text-sm text-zinc-500 italic">No description yet.</p>
      {:else}
        <p class="text-sm text-zinc-600 italic">No metadata.</p>
      {/if}
    </div>
  {:else}
  <!-- Card body: horizontal split -->
  <div class="flex-1 overflow-hidden bg-zinc-900 rounded-b-lg flex flex-row min-h-0">

    <!-- LEFT PANEL: metadata -->
    <div class="flex flex-col overflow-y-auto" style="min-width:300px; width:300px; border-right: 1px solid theme('colors.zinc.800');">
      {#if file}
        <div class="p-3 flex flex-col gap-3 text-xs flex-1">

          <!-- Large filename title -->
          <h2 class="text-xl font-mono font-semibold text-zinc-100 leading-tight truncate" title={file.path}>
            {filename}
          </h2>

          <!-- Illustration image -->
          {#if file.imagePath}
            <div class="relative group rounded overflow-hidden bg-zinc-800">
              <img
                src={file.imagePath}
                alt="Illustration"
                class="w-full object-contain max-h-48"
              />
              {#if canWrite}
                <button
                  class="absolute top-1 right-1 p-0.5 bg-black/60 rounded opacity-0 group-hover:opacity-100 transition-opacity text-zinc-300 hover:text-white"
                  on:click={clearImage}
                  on:mousedown|stopPropagation
                  title="Remove image"
                >
                  <XCircleIcon size="14" />
                </button>
              {/if}
            </div>
            {#if canWrite}
              <div class="flex items-center gap-1.5" on:mousedown|stopPropagation>
                <input
                  type="text"
                  bind:value={imageName}
                  placeholder="Save as… (e.g. screenshot.png)"
                  class="flex-1 bg-zinc-800 border border-zinc-700 rounded px-2 py-0.5 text-[11px] text-zinc-300 placeholder-zinc-600 focus:outline-none focus:border-indigo-500"
                  on:keydown={(e) => e.key === "Enter" && submitImageName()}
                  on:blur={submitImageName}
                />
              </div>
            {/if}
          {:else if canWrite}
            <!-- Image drop zone -->
            <!-- svelte-ignore a11y-no-static-element-interactions -->
            <div
              class="rounded border-2 border-dashed transition-colors cursor-pointer flex items-center justify-center gap-1.5 py-3 text-xs"
              class:border-indigo-600={dragOver}
              class:bg-indigo-900={dragOver}
              class:text-indigo-300={dragOver}
              class:border-zinc-700={!dragOver}
              class:text-zinc-500={!dragOver}
              on:dragover|preventDefault={() => (dragOver = true)}
              on:dragleave={() => (dragOver = false)}
              on:drop={handleDrop}
              on:mousedown|stopPropagation
            >
              <UploadCloudIcon size="14" />
              <span>Drop image or</span>
              <label class="text-indigo-400 hover:text-indigo-300 cursor-pointer underline underline-offset-1">
                browse
                <input
                  type="file"
                  accept="image/*"
                  class="hidden"
                  on:change={handleFileInput}
                />
              </label>
            </div>
          {/if}

          <!-- Kind badge + date -->
          <div class="flex items-center gap-2">
            <span class="px-2 py-0.5 rounded text-xs font-medium {kindColors[file.kind] ?? kindColors.other}">
              {file.kind}
            </span>
            <span class="text-zinc-500 text-xs ml-auto">{file.lastModified.slice(0, 10)}</span>
          </div>

          <!-- Large colored size indicator -->
          <div class="flex items-baseline gap-2">
            <span class="text-3xl font-bold {sizeColor}">{file.lineCount}</span>
            <span class="text-sm text-zinc-500">lines</span>
            <div class="flex-1 h-2 rounded-full bg-zinc-800 overflow-hidden self-center">
              <div class="h-full rounded-full {barColor}" style:width="{Math.min(100, file.lineCount / 10)}%" />
            </div>
          </div>

          <!-- AI description (editable) -->
          {#if editingDescription}
            <!-- svelte-ignore a11y-no-static-element-interactions -->
            <div class="carta-card flex flex-col gap-1" on:mousedown|stopPropagation>
              <CartaEditor
                {carta}
                bind:value={editDescriptionText}
                disableToolbar={true}
                mode="split"
                placeholder="Describe this file…"
                theme="card"
              />
              <div class="flex gap-1">
                <button
                  class="flex items-center gap-1 text-[10px] text-green-400 hover:text-green-300 border border-green-800 hover:border-green-600 rounded px-1.5 py-0.5"
                  on:click={saveDescription}
                >
                  <CheckIcon size="10" /> Save
                </button>
                <button
                  class="text-[10px] text-zinc-500 hover:text-zinc-300 border border-zinc-700 rounded px-1.5 py-0.5"
                  on:click={cancelDescriptionEdit}
                >
                  Cancel
                </button>
              </div>
            </div>
          {:else if describing}
            <div class="flex items-center gap-2 text-indigo-400">
              <svg class="animate-spin w-3 h-3 flex-shrink-0" viewBox="0 0 24 24" fill="none">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"/>
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v4a4 4 0 00-4 4H4z"/>
              </svg>
              <span>Describing…</span>
            </div>
          {:else}
            <!-- Description view (rendered markdown) -->
            {#if file.description}
              <div class="group flex items-start gap-1">
                <div class="carta-view flex-1 text-sm">
                  {#key file.description}
                    <CartaViewer {carta} value={file.description} theme="card" />
                  {/key}
                </div>
                {#if canWrite}
                  <button
                    class="p-0.5 rounded hover:bg-zinc-700 text-zinc-600 hover:text-zinc-300 opacity-0 group-hover:opacity-100 transition-opacity flex-shrink-0 mt-0.5"
                    on:click={startDescriptionEdit}
                    on:mousedown|stopPropagation
                    title="Edit description"
                  >
                    <EditIcon size="11" />
                  </button>
                {/if}
              </div>
            {/if}

            <!-- Action buttons row -->
            {#if canWrite}
              <!-- svelte-ignore a11y-no-static-element-interactions -->
              <div class="flex flex-wrap gap-1.5" on:mousedown|stopPropagation>
                <!-- Edit button (only shown if no description) -->
                {#if !file.description}
                  <button
                    class="flex items-center gap-1 text-sm text-zinc-400 hover:text-zinc-200 border border-zinc-700 hover:border-zinc-500 rounded px-2.5 py-1"
                    on:click={startDescriptionEdit}
                    title="Write description manually"
                  >
                    <EditIcon size="12" /> Edit
                  </button>
                {/if}
                <button
                  class="flex items-center gap-1 text-sm text-indigo-400 hover:text-indigo-300 border border-indigo-800 hover:border-indigo-600 rounded px-2.5 py-1"
                  on:click={() => { if (file) { describing = true; dispatch("describe", file.path); } }}
                  title={file.description ? "Re-generate description with Claude" : "Generate description with Claude"}
                >
                  ✦ {file.description ? "Re-describe" : "Describe with Claude"}
                </button>
                <button
                  class="text-sm text-zinc-400 hover:text-zinc-200 border border-zinc-700 hover:border-zinc-500 rounded px-2.5 py-1"
                  on:click={() => (askingClaude = !askingClaude)}
                  title="Ask Claude a question about this file"
                >
                  Ask Claude…
                </button>
              </div>

              <!-- Ask Claude form -->
              {#if askingClaude}
                <!-- svelte-ignore a11y-no-static-element-interactions -->
                <div class="flex flex-col gap-1" on:mousedown|stopPropagation>
                  <input
                    type="text"
                    bind:value={claudeQuestion}
                    placeholder="Question or update request…"
                    class="bg-zinc-800 border border-zinc-700 focus:border-indigo-600 rounded px-2 py-1 text-xs text-zinc-200 placeholder-zinc-500 outline-none w-full"
                    on:keydown={(e) => {
                      if (e.key === "Enter" && claudeQuestion.trim() && file) {
                        dispatch("openInClaude", { path: file.path, question: claudeQuestion.trim() });
                        claudeQuestion = "";
                        askingClaude = false;
                      } else if (e.key === "Escape") {
                        askingClaude = false;
                      }
                    }}
                  />
                  <button
                    class="text-xs text-indigo-400 hover:text-indigo-300 border border-indigo-800 hover:border-indigo-600 rounded px-2 py-0.5 self-start disabled:opacity-40"
                    disabled={!claudeQuestion.trim()}
                    on:click={() => {
                      if (!claudeQuestion.trim() || !file) return;
                      dispatch("openInClaude", { path: file.path, question: claudeQuestion.trim() });
                      claudeQuestion = "";
                      askingClaude = false;
                    }}
                  >
                    ↗ Open in Claude Code
                  </button>
                </div>
              {/if}
            {/if}
          {/if}

          <!-- Libraries (external packages) -->
          {#if file.libraries.length > 0}
            <div class="flex flex-wrap items-baseline gap-x-1 gap-y-0.5">
              <span class="text-zinc-500 flex-shrink-0">imports:</span>
              {#each file.libraries.slice(0, 6) as lib (lib)}
                <button
                  class="text-yellow-600 hover:text-yellow-400 font-mono text-xs hover:underline"
                  on:click|stopPropagation={() => dispatch("openLibrary", lib)}
                  on:mousedown|stopPropagation
                  title="Open library card for {lib}"
                >{lib}</button>
              {/each}
              {#if file.libraries.length > 6}
                <span class="text-zinc-500 text-xs">+{file.libraries.length - 6}</span>
              {/if}
            </div>
          {/if}

          <!-- Local imports -->
          {#if file.localImports.length > 0}
            <div>
              <span class="text-zinc-500">uses:</span>
              <span class="ml-1">
                {#each file.localImports.slice(0, 3) as imp}
                  <button
                    class="text-indigo-400 hover:text-indigo-300 mr-1"
                    on:click={() => dispatch("openFile", imp)}
                  >{imp.split("/").pop()}</button>
                {/each}
                {#if file.localImports.length > 3}
                  <span class="text-zinc-500">+{file.localImports.length - 3}</span>
                {/if}
              </span>
            </div>
          {/if}

          <!-- Imported by -->
          {#if file.importedBy.length > 0}
            <div class="flex flex-col gap-0.5">
              <div class="flex items-center gap-1">
                <span class="text-zinc-500">used by:</span>
                <button
                  class="text-indigo-400 hover:text-indigo-300 font-mono text-[11px] truncate max-w-[160px]"
                  on:click={() => file && dispatch("openFile", file.importedBy[0])}
                  title={file.importedBy[0]}
                >{file.importedBy[0]}</button>
                {#if file.importedBy.length > 1}
                  <button
                    class="text-zinc-500 hover:text-zinc-300 text-[10px] leading-none px-0.5"
                    title="{importedByExpanded ? 'Collapse' : 'Show all ' + file.importedBy.length}"
                    on:click={() => (importedByExpanded = !importedByExpanded)}
                  >+{file.importedBy.length - 1}</button>
                {/if}
              </div>
              {#if importedByExpanded}
                <div class="ml-12 flex flex-col gap-0.5">
                  {#each file.importedBy.slice(1) as imp}
                    <button
                      class="text-indigo-400 hover:text-indigo-300 text-left truncate font-mono text-[11px]"
                      on:click={() => dispatch("openFile", imp)}
                      title={imp}
                    >{imp}</button>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}

          <!-- Exports -->
          {#if file.exports.length > 0}
            <div class="flex flex-col gap-0.5">
              <span class="text-zinc-500">exports:</span>
              {#each file.exports.slice(0, 8) as exportName (exportName)}
                <div>
                  <button
                    class="w-full text-left text-xs font-mono px-1 py-0.5 rounded hover:bg-zinc-800 transition-colors"
                    class:text-zinc-300={selectedExport !== exportName}
                    class:text-indigo-300={selectedExport === exportName}
                    class:bg-zinc-800={selectedExport === exportName}
                    on:click|stopPropagation={() => (selectedExport = selectedExport === exportName ? null : exportName)}
                    on:mousedown|stopPropagation
                    title="Show files that import {exportName}"
                  >{exportName} <span class="text-zinc-600 text-[10px]">{selectedExport === exportName ? '▴' : '▾'}</span></button>
                  {#if selectedExport === exportName}
                    <div class="ml-2 mt-0.5 flex flex-col gap-0.5 pb-0.5">
                      {#if file.importedBy.length > 0}
                        {#each file.importedBy as imp (imp)}
                          <button
                            class="text-[10px] text-indigo-400 hover:text-indigo-300 text-left px-1 py-0.5 rounded hover:bg-zinc-800 truncate font-mono"
                            on:click|stopPropagation={() => dispatch("openFile", imp)}
                            on:mousedown|stopPropagation
                            title={imp}
                          >↗ {imp}</button>
                        {/each}
                      {:else}
                        <span class="text-[10px] text-zinc-600 italic px-1">No importers found</span>
                      {/if}
                    </div>
                  {/if}
                </div>
              {/each}
              {#if file.exports.length > 8}
                <span class="text-zinc-600 text-[10px] px-1">+{file.exports.length - 8} more</span>
              {/if}
            </div>
          {/if}
        </div>
      {:else}
        <div class="flex-1 flex items-center justify-center text-zinc-500 text-sm p-4 text-center">
          No metadata yet.<br />
          <span class="text-xs text-zinc-600 mt-1">Run <code class="font-mono">sshx analyze</code> to enable.</span>
        </div>
      {/if}
    </div>

    <!-- RIGHT PANEL: Monaco code editor -->
    {#if showCode}
    <div class="flex flex-col flex-1 min-w-0 min-h-0">
      <!-- Editor area -->
      <div class="flex-1 overflow-hidden min-h-0">
        {#if file?.content}
          <div class="w-full h-full" use:mountEditor />
        {:else}
          <div class="flex-1 flex items-center justify-center text-zinc-500 text-sm p-4 text-center h-full">
            No preview available.<br />
            <span class="text-xs text-zinc-600">Run <code class="font-mono">sshx analyze</code> first.</span>
          </div>
        {/if}
      </div>

      <!-- Editor bottom bar -->
      <!-- svelte-ignore a11y-no-static-element-interactions -->
      <div class="flex-shrink-0 border-t border-zinc-800" on:mousedown|stopPropagation>
        <!-- Ask Claude expanded form -->
        {#if editorAskingClaude}
          <div class="px-2 pt-1.5 pb-1 flex flex-col gap-1 bg-zinc-900">
            <input
              type="text"
              bind:value={editorClaudeQuestion}
              placeholder="Question or request…"
              class="bg-zinc-800 border border-zinc-700 focus:border-indigo-600 rounded px-2 py-1 text-xs text-zinc-200 placeholder-zinc-500 outline-none w-full"
              on:keydown={(e) => {
                if (e.key === "Enter") submitEditorQuestion();
                else if (e.key === "Escape") editorAskingClaude = false;
              }}
            />
            <div class="flex items-center justify-between">
              <span class="text-[10px] text-zinc-500 font-mono truncate">
                {selectedLines
                  ? `lines ${selectedLines.start}–${selectedLines.end}`
                  : (file?.path?.split("/").pop() ?? "file")}
              </span>
              <button
                class="text-[10px] text-indigo-400 hover:text-indigo-300 border border-indigo-800 hover:border-indigo-600 rounded px-1.5 py-0.5 disabled:opacity-40 flex-shrink-0"
                disabled={!editorClaudeQuestion.trim()}
                on:click={submitEditorQuestion}
              >
                ↗ Send
              </button>
            </div>
          </div>
        {/if}

        <!-- Toolbar row -->
        <div class="flex items-center px-2 py-1 gap-1.5 bg-zinc-900 rounded-br-lg">
          <!-- Font size controls -->
          <div class="flex items-center gap-0.5">
            <button
              class="w-5 h-5 flex items-center justify-center text-[11px] text-zinc-500 hover:text-zinc-200 hover:bg-zinc-700 rounded leading-none"
              on:click={() => changeFontSize(-1)}
              title="Decrease font size"
            >A−</button>
            <button
              class="w-5 h-5 flex items-center justify-center text-[11px] text-zinc-500 hover:text-zinc-200 hover:bg-zinc-700 rounded leading-none"
              on:click={() => changeFontSize(1)}
              title="Increase font size"
            >A+</button>
          </div>

          <div class="flex-1" />

          <!-- Ask Claude button -->
          <button
            class="flex items-center gap-1 text-[10px] rounded px-1.5 py-0.5 border transition-colors"
            class:text-indigo-400={!editorAskingClaude}
            class:border-indigo-900={!editorAskingClaude}
            class:hover:border-indigo-700={!editorAskingClaude}
            class:text-indigo-300={editorAskingClaude}
            class:border-indigo-700={editorAskingClaude}
            class:bg-indigo-950={editorAskingClaude}
            on:click={() => (editorAskingClaude = !editorAskingClaude)}
          >
            <span>✦ Ask Claude</span>
            <span class="text-zinc-500 font-mono">
              {selectedLines
                ? `· ${selectedLines.start}–${selectedLines.end}`
                : `· ${file?.path?.split("/").pop() ?? "file"}`}
            </span>
          </button>

          <!-- Open in VSCode button -->
          <button
            class="flex items-center gap-1 text-[10px] rounded px-1.5 py-0.5 border transition-colors"
            class:text-blue-400={ideAvailable && !!file}
            class:border-blue-900={ideAvailable && !!file}
            class:hover:border-blue-700={ideAvailable && !!file}
            class:text-zinc-600={!ideAvailable || !file}
            class:border-zinc-800={!ideAvailable || !file}
            class:cursor-not-allowed={!ideAvailable || !file}
            disabled={!ideAvailable || !file}
            on:click={handleOpenInVSCode}
            title={ideAvailable ? "Open in VS Code" : "No IDE available — start sshx with --ide"}
          >
            Open in VSCode
          </button>
        </div>
      </div>
    </div>
    {/if}

  </div>
  {/if}
</div>

<style lang="postcss">
  .panel-window {
    @apply inline-flex rounded-lg border border-zinc-700 bg-zinc-800 opacity-90;
    transition: opacity 200ms, border-color 200ms;
  }

  @keyframes highlight-flash {
    0%   { border-color: theme(colors.indigo.400); box-shadow: 0 0 0 2px theme(colors.indigo.400 / 40%); }
    50%  { border-color: theme(colors.indigo.300); box-shadow: 0 0 0 4px theme(colors.indigo.300 / 60%); }
    100% { border-color: theme(colors.indigo.400); box-shadow: 0 0 0 2px theme(colors.indigo.400 / 40%); }
  }

  .panel-window.highlighted {
    animation: highlight-flash 0.6s ease-in-out 3;
    @apply opacity-100;
  }

  .description-clamp {
    display: -webkit-box;
    -webkit-line-clamp: 4;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .panel-window:hover {
    @apply opacity-100;
  }

  /* ── Carta dark theme for cards ─────────────────────────────────── */

  /* Stack input above preview instead of side-by-side */
  :global(.carta-theme__card .carta-container.mode-split) {
    flex-direction: column;
  }
  :global(.carta-theme__card .carta-container.mode-split > *) {
    width: 100%;
  }
  /* Remove the vertical splitter rule */
  :global(.carta-theme__card .mode-split.carta-container::after) {
    display: none;
  }

  /* Editor wrapper */
  :global(.carta-theme__card.carta-editor) {
    border: 1px solid #3f3f46; /* zinc-700 */
    border-radius: 6px;
    background: #18181b; /* zinc-900 */
    font-size: 0.75rem;
  }

  /* Textarea */
  :global(.carta-theme__card .carta-input) {
    background: #18181b;
    color: #d4d4d8; /* zinc-300 */
    caret-color: #a1a1aa;
    font-size: 0.75rem;
    line-height: 1.5;
    padding: 6px 8px;
    min-height: 80px;
    resize: none;
    outline: none;
  }

  /* Preview pane */
  :global(.carta-theme__card .carta-renderer) {
    background: #18181b;
    color: #a1a1aa; /* zinc-400 */
    font-size: 0.75rem;
    padding: 6px 8px;
    overflow-y: auto;
  }

  /* Markdown element resets inside the card theme */
  :global(.carta-theme__card .carta-renderer p) {
    margin: 0 0 4px;
  }
  :global(.carta-theme__card .carta-renderer code) {
    background: #27272a; /* zinc-800 */
    border-radius: 3px;
    padding: 0 3px;
    font-size: 0.7rem;
  }
  :global(.carta-theme__card .carta-renderer h1,
          .carta-theme__card .carta-renderer h2,
          .carta-theme__card .carta-renderer h3) {
    font-size: 0.8rem;
    font-weight: 600;
    margin: 4px 0 2px;
    color: #e4e4e7; /* zinc-200 */
  }
</style>
