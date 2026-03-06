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
  }>();

  export let collapsed: boolean = false;
  export let graphMode: boolean = false;
  export let highlighted: boolean = false;

  /** Which export name is currently expanded to show its importers. */
  let selectedExport: string | null = null;

  let flipped = false;
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
      dispatch("updateMetadata", { path: file.path, update: { imagePath: url } });
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
</script>

<div
  class="panel-window flex flex-col select-none"
  class:highlighted
  style:width={collapsed ? "280px" : `${effectiveW}px`}
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
      {file?.path ?? (widget.kind.type === "fileCard" ? widget.kind.path : "file")}
    </div>
    <div class="flex-1 flex items-center justify-end pr-2"><span class="text-zinc-500 text-[10px]">{collapsed ? '▴' : '▾'}</span></div>
  </div>

  {#if collapsed}
    <div class="px-3 py-2 bg-zinc-900 rounded-b-lg">
      {#if graphMode && file}
        <!-- Graph mode: import/export port list -->
        <div class="flex flex-col text-xs">
          {#each file.localImports as imp (imp)}
            <div class="flex items-center gap-1.5 py-0.5 hover:bg-zinc-800 rounded px-0.5 group">
              <!-- svelte-ignore a11y-no-static-element-interactions -->
              <span
                class="w-2.5 h-2.5 rounded-full bg-zinc-500 group-hover:bg-indigo-400 flex-shrink-0 cursor-pointer transition-colors"
                title="Open {imp}"
                on:click|stopPropagation={() => dispatch("openFile", imp)}
                on:mousedown|stopPropagation
              />
              <span class="text-zinc-400 truncate">{imp.split("/").pop()}</span>
            </div>
          {/each}
          {#each file.libraries.slice(0, 3) as lib (lib)}
            <!-- svelte-ignore a11y-no-static-element-interactions -->
            <div
              class="flex items-center gap-1.5 py-0.5 px-0.5 hover:bg-zinc-800 rounded cursor-pointer group"
              on:click|stopPropagation={() => dispatch("openLibrary", lib)}
              on:mousedown|stopPropagation
              title="Open library card for {lib}"
            >
              <span class="w-2.5 h-2.5 rounded-full bg-zinc-700 group-hover:bg-amber-700 flex-shrink-0 transition-colors" />
              <span class="text-zinc-600 group-hover:text-amber-500 truncate italic text-[10px] transition-colors">{lib}</span>
            </div>
          {/each}
          {#each file.importedBy as dep (dep)}
            <div class="flex items-center justify-end gap-1.5 py-0.5 hover:bg-zinc-800 rounded px-0.5 group">
              <span class="text-zinc-400 truncate text-right">{dep.split("/").pop()}</span>
              <!-- svelte-ignore a11y-no-static-element-interactions -->
              <span
                class="w-2.5 h-2.5 rounded-full bg-zinc-500 group-hover:bg-cyan-400 flex-shrink-0 cursor-pointer transition-colors"
                title="Open {dep}"
                on:click|stopPropagation={() => dispatch("openFile", dep)}
                on:mousedown|stopPropagation
              />
            </div>
          {/each}
        </div>
        {#if file.localImports.length > 0 || file.importedBy.length > 0}
          <!-- svelte-ignore a11y-no-static-element-interactions -->
          <div class="flex gap-1 pt-1.5 border-t border-zinc-800 mt-1" on:mousedown|stopPropagation>
            {#if file.localImports.length > 0}
              <button
                class="text-[10px] px-1.5 py-0.5 rounded bg-zinc-800 hover:bg-zinc-700 text-zinc-400"
                on:click|stopPropagation={() => dispatch("loadImports")}
                title="Open all imported files"
              >↗ imports</button>
            {/if}
            {#if file.importedBy.length > 0}
              <button
                class="text-[10px] px-1.5 py-0.5 rounded bg-zinc-800 hover:bg-zinc-700 text-zinc-400"
                on:click|stopPropagation={() => dispatch("loadDependents")}
                title="Open dependent files"
              >↗ dependents</button>
            {/if}
          </div>
        {/if}
      {:else if file?.description}
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
  <!-- Card body -->
  <div class="flex-1 overflow-hidden bg-zinc-900 rounded-b-lg flex flex-col">
    {#if !flipped}
      <!-- Front face: metadata -->
      {#if file}
        <div class="p-3 flex flex-col gap-2 overflow-y-auto text-xs flex-1">
          <!-- Illustration image -->
          {#if file.imagePath}
            <div class="relative group rounded overflow-hidden bg-zinc-800">
              <img
                src={file.imagePath}
                alt="Illustration"
                class="w-full object-contain max-h-40"
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
          {:else if canWrite}
            <!-- Image drop zone -->
            <!-- svelte-ignore a11y-no-static-element-interactions -->
            <div
              class="rounded border-2 border-dashed transition-colors cursor-pointer flex items-center justify-center gap-1.5 py-2 text-[11px]"
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
              <UploadCloudIcon size="12" />
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
            <span class="px-1.5 py-0.5 rounded text-xs {kindColors[file.kind] ?? kindColors.other}">
              {file.kind}
            </span>
            <span class="text-zinc-600 text-[10px] ml-auto">{file.lastModified.slice(0, 10)}</span>
          </div>

          <!-- Size blocks -->
          <span class="flex items-center gap-0.5">
            {#each Array(Math.min(12, Math.ceil(file.lineCount / 50))).fill(null) as _}
              <span class="inline-block w-1.5 h-3 rounded-sm bg-zinc-600"></span>
            {/each}
            {#if file.lineCount > 600}
              <span class="text-zinc-500 text-[9px]">…</span>
            {/if}
            <span class="ml-1 text-zinc-500 text-[10px]">{file.lineCount} lines</span>
          </span>

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
                <div class="carta-view flex-1">
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

            <!-- Claude action buttons (always visible when canWrite) -->
            {#if canWrite}
              <!-- svelte-ignore a11y-no-static-element-interactions -->
              <div class="flex flex-wrap gap-1" on:mousedown|stopPropagation>
                <button
                  class="text-xs text-indigo-400 hover:text-indigo-300 border border-indigo-800 hover:border-indigo-600 rounded px-2 py-0.5"
                  on:click={() => { if (file) { describing = true; dispatch("describe", file.path); } }}
                  title={file.description ? "Re-generate description with Claude" : "Generate description with Claude"}
                >
                  ✦ {file.description ? "Re-describe" : "Describe with Claude"}
                </button>
                {#if !file.description}
                  <button
                    class="text-xs text-zinc-500 hover:text-zinc-300 border border-zinc-700 hover:border-zinc-500 rounded px-1.5 py-0.5"
                    on:click={startDescriptionEdit}
                    title="Write description manually"
                  >
                    <EditIcon size="10" />
                  </button>
                {/if}
                <button
                  class="text-xs text-zinc-400 hover:text-zinc-200 border border-zinc-700 hover:border-zinc-500 rounded px-2 py-0.5"
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

          <!-- Libraries (external packages) — clickable to open LibraryCard -->
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

          <!-- Exports — click an export to reveal which files import it -->
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

      <div class="flex justify-end px-3 pb-2 flex-shrink-0">
        <button
          class="text-xs text-zinc-500 hover:text-zinc-300"
          on:click={() => (flipped = true)}
        >
          code →
        </button>
      </div>
    {:else}
      <!-- Back face: Monaco code editor -->
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
        <div class="flex items-center px-2 py-1 gap-1.5 bg-zinc-900 rounded-b-lg">
          <button
            class="text-[11px] text-zinc-500 hover:text-zinc-300"
            on:click={() => { flipped = false; editorAskingClaude = false; }}
          >
            ← info
          </button>

          <div class="flex-1" />

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

          <!-- Ask Claude button with context indicator -->
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
    padding: 6px 8px;
    min-height: 72px;
    max-height: 120px;
    resize: none;
    font-size: 0.75rem;
    line-height: 1.5;
    border-bottom: 1px solid #3f3f46;
  }
  :global(.carta-theme__card .carta-input ::selection) {
    background: #3730a380;
  }

  /* Syntax highlight layer (sits over textarea) */
  :global(.carta-theme__card .carta-highlight) {
    padding: 6px 8px;
    font-size: 0.75rem;
    line-height: 1.5;
    color: transparent; /* highlight layer, not text */
  }

  /* Preview renderer */
  :global(.carta-theme__card .carta-renderer) {
    background: #09090b; /* zinc-950 */
    color: #a1a1aa; /* zinc-400 */
    padding: 6px 8px;
    font-size: 0.72rem;
    line-height: 1.6;
    max-height: 120px;
    overflow-y: auto;
    border-radius: 0 0 6px 6px;
  }

  /* Markdown elements inside renderer */
  :global(.carta-theme__card .carta-renderer p) { margin-bottom: 0.3em; }
  :global(.carta-theme__card .carta-renderer h1),
  :global(.carta-theme__card .carta-renderer h2),
  :global(.carta-theme__card .carta-renderer h3) {
    color: #e4e4e7;
    font-weight: 600;
    margin-bottom: 0.2em;
  }
  :global(.carta-theme__card .carta-renderer strong) { color: #e4e4e7; font-weight: 700; }
  :global(.carta-theme__card .carta-renderer em) { font-style: italic; }
  :global(.carta-theme__card .carta-renderer code) {
    font-family: "Fira Code VF", monospace;
    font-size: 0.85em;
    background: #27272a;
    border-radius: 3px;
    padding: 0.1em 0.3em;
    color: #c4b5fd;
  }
  :global(.carta-theme__card .carta-renderer ul) { padding-left: 1.2em; list-style: disc; }
  :global(.carta-theme__card .carta-renderer ol) { padding-left: 1.2em; list-style: decimal; }
  :global(.carta-theme__card .carta-renderer li) { margin-bottom: 0.1em; }

  /* ── Monaco thin scrollbars ─────────────────────────────────────── */
  :global(.monaco-scrollable-element > .scrollbar) {
    border-radius: 3px;
  }
  :global(.monaco-scrollable-element > .scrollbar > .slider) {
    border-radius: 3px !important;
    background: rgba(100, 100, 120, 0.45) !important;
  }
  :global(.monaco-scrollable-element > .scrollbar > .slider:hover) {
    background: rgba(140, 140, 165, 0.6) !important;
  }
  :global(.monaco-scrollable-element > .scrollbar.vertical) {
    width: 6px !important;
  }
  :global(.monaco-scrollable-element > .scrollbar.horizontal) {
    height: 6px !important;
  }

  /* Read-only viewer */
  .carta-view :global(.carta-viewer) {
    color: #a1a1aa;
    font-size: 0.72rem;
    line-height: 1.6;
    font-style: italic;
  }
  .carta-view :global(.carta-viewer p) { margin-bottom: 0.3em; }
  .carta-view :global(.carta-viewer p:last-child) { margin-bottom: 0; }
  .carta-view :global(.carta-viewer strong) { color: #e4e4e7; font-weight: 700; }
  .carta-view :global(.carta-viewer em) { font-style: italic; }
  .carta-view :global(.carta-viewer code) {
    font-family: "Fira Code VF", monospace;
    font-size: 0.85em;
    background: #27272a;
    border-radius: 3px;
    padding: 0.1em 0.3em;
    color: #c4b5fd;
  }
  .carta-view :global(.carta-viewer ul) { padding-left: 1.2em; list-style: disc; }
  .carta-view :global(.carta-viewer ol) { padding-left: 1.2em; list-style: decimal; }
</style>
