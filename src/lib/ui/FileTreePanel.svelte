<script lang="ts">
  import { onMount, createEventDispatcher } from "svelte";
  import type { WsWidget, WsSourceFile } from "../protocol";
  import CircleButton from "./CircleButton.svelte";
  import CircleButtons from "./CircleButtons.svelte";

  export let widget: WsWidget;
  export let files: WsSourceFile[];
  export let canWrite: boolean;

  const dispatch = createEventDispatcher<{
    startMove: MouseEvent;
    delete: void;
    dragFile: { path: string; event: MouseEvent };
    collapse: boolean;
  }>();

  export let collapsed: boolean = false;

  /** Flat visible row in the tree. */
  type Row = {
    path: string;
    name: string;
    depth: number;
    isDir: boolean;
    file?: WsSourceFile;
    expanded?: boolean;
  };

  let expanded = new Set<string>();

  // Collect all unique directory paths from the file list.
  function allDirs(fileList: WsSourceFile[]): Set<string> {
    const dirs = new Set<string>();
    for (const f of fileList) {
      const parts = f.path.split("/");
      for (let i = 1; i < parts.length; i++) {
        dirs.add(parts.slice(0, i).join("/"));
      }
    }
    return dirs;
  }

  // Initialize: expand all top-level directories once files are available.
  onMount(() => {
    for (const f of files) {
      if (f.path.includes("/")) {
        expanded.add(f.path.split("/")[0]);
      }
    }
    expanded = expanded;
  });

  function expandAll() {
    expanded = allDirs(files);
  }

  function collapseAll() {
    expanded = new Set();
  }

  /** Pure function — reads `exp` but never mutates it. */
  function buildRows(fileList: WsSourceFile[], exp: Set<string>): Row[] {
    // Collect all unique directory paths.
    const dirs = new Set<string>();
    for (const f of fileList) {
      const parts = f.path.split("/");
      for (let i = 1; i < parts.length; i++) {
        dirs.add(parts.slice(0, i).join("/"));
      }
    }

    // Build sorted list of all paths (dirs + files).
    type Item = { path: string; isDir: boolean };
    const all: Item[] = [
      ...[...dirs].map((p) => ({ path: p, isDir: true })),
      ...fileList.map((f) => ({ path: f.path, isDir: false })),
    ].sort((a, b) => {
      const ap = a.path.split("/");
      const bp = b.path.split("/");
      for (let i = 0; i < Math.min(ap.length, bp.length); i++) {
        if (ap[i] !== bp[i]) {
          const aIsDir = i < ap.length - 1 || a.isDir;
          const bIsDir = i < bp.length - 1 || b.isDir;
          if (aIsDir !== bIsDir) return aIsDir ? -1 : 1;
          return ap[i].localeCompare(bp[i]);
        }
      }
      return ap.length - bp.length;
    });

    // Filter to only items whose ancestor directories are expanded.
    const rows: Row[] = [];
    for (const item of all) {
      const parts = item.path.split("/");
      const depth = parts.length - 1;

      let visible = true;
      for (let i = 1; i < parts.length; i++) {
        if (!exp.has(parts.slice(0, i).join("/"))) {
          visible = false;
          break;
        }
      }
      if (!visible) continue;

      const name = parts[parts.length - 1];
      rows.push({
        path: item.path,
        name,
        depth,
        isDir: item.isDir,
        file: item.isDir ? undefined : fileList.find((f) => f.path === item.path),
        expanded: item.isDir ? exp.has(item.path) : undefined,
      });
    }
    return rows;
  }

  $: rows = buildRows(files, expanded);
  $: rootName = widget.kind.type === "fileTree" ? widget.kind.root : "workspace";

  let searchQuery = "";

  $: searchResults = searchQuery.trim()
    ? files.filter((f) => {
        const q = searchQuery.toLowerCase();
        return (
          f.path.toLowerCase().includes(q) ||
          (f.description ?? "").toLowerCase().includes(q)
        );
      }).sort((a, b) => {
        // Exact filename match first
        const qa = a.path.split("/").pop()!.toLowerCase().includes(searchQuery.toLowerCase()) ? 0 : 1;
        const qb = b.path.split("/").pop()!.toLowerCase().includes(searchQuery.toLowerCase()) ? 0 : 1;
        return qa - qb || a.path.localeCompare(b.path);
      })
    : null;

  $: flatFiles = files.map(f => ({
    path: f.path,
    name: f.path.split('/').pop() ?? f.path,
  }));

  function toggle(path: string) {
    if (expanded.has(path)) {
      expanded.delete(path);
    } else {
      expanded.add(path);
    }
    expanded = expanded; // trigger reactivity
  }

  const kindColors: Record<string, string> = {
    component: "bg-blue-700 text-blue-100",
    hook: "bg-purple-700 text-purple-100",
    utility: "bg-zinc-600 text-zinc-100",
    config: "bg-yellow-700 text-yellow-100",
    type: "bg-green-700 text-green-100",
    other: "bg-zinc-700 text-zinc-200",
  };
</script>

<div
  class="panel-window flex flex-col"
  style:width={collapsed ? "280px" : `${widget.w}px`}
  style:height={collapsed ? "auto" : `${widget.h}px`}
>
  <!-- Header bar -->
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
        {#if !collapsed}
          <CircleButton
            kind="yellow"
            on:mousedown={(event) => event.button === 0 && collapseAll()}
          />
          <CircleButton
            kind="green"
            on:mousedown={(event) => event.button === 0 && expandAll()}
          />
        {/if}
      </CircleButtons>
    </div>
    <div
      class="py-2 text-sm text-zinc-300 text-center font-medium overflow-hidden whitespace-nowrap text-ellipsis w-0 flex-grow-[4]"
      title={rootName}
    >
      {rootName}
    </div>
    <div class="flex-1 flex items-center justify-end px-3 py-1.5">
      <span class="text-xs text-zinc-500">{collapsed ? '▴' : `${files.length} files ▾`}</span>
    </div>
  </div>

  {#if collapsed}
    <div class="bg-zinc-900 rounded-b-lg py-1 max-h-48 overflow-y-auto" on:wheel={(e) => { if (!e.ctrlKey && !e.metaKey && !e.altKey) e.stopPropagation(); }}>
      {#each flatFiles as f (f.path)}
        <div class="px-3 py-0.5 text-sm text-zinc-300 truncate hover:bg-zinc-800">{f.name}</div>
      {/each}
      {#if files.length === 0}
        <div class="px-3 py-2 text-zinc-500 text-sm text-center">No files yet…</div>
      {/if}
    </div>
  {:else}
  <!-- Search bar -->
  <div class="flex items-center gap-1.5 px-2 py-1 border-t border-zinc-700/60 flex-shrink-0">
    <span class="text-zinc-500 text-[10px]">⌕</span>
    <input
      class="flex-1 bg-transparent text-xs text-zinc-300 placeholder-zinc-600 outline-none"
      placeholder="Filtrer les fichiers…"
      bind:value={searchQuery}
      on:mousedown|stopPropagation
      on:pointerdown|stopPropagation
    />
    {#if searchQuery}
      <button
        class="text-zinc-500 hover:text-zinc-300 text-[10px] leading-none"
        on:mousedown|stopPropagation|preventDefault={() => (searchQuery = "")}
      >✕</button>
    {/if}
  </div>

  <!-- Tree / search results body -->
  <div class="overflow-y-auto flex-1 bg-zinc-900 rounded-b-lg text-xs" on:wheel={(e) => { if (!e.ctrlKey && !e.metaKey && !e.altKey) e.stopPropagation(); }}>
    {#if searchResults !== null}
      <!-- Flat filtered list -->
      {#each searchResults as f (f.path)}
        {@const name = f.path.split("/").pop() ?? f.path}
        {@const dir = f.path.includes("/") ? f.path.slice(0, f.path.lastIndexOf("/")) : ""}
        <div
          role="button"
          tabindex="0"
          class="flex items-center gap-1 px-2 py-0.5 hover:bg-zinc-800 cursor-pointer select-none"
          on:click={(e) => dispatch("dragFile", { path: f.path, event: e })}
          on:keydown={(e) => e.key === "Enter" && dispatch("dragFile", { path: f.path, event: e })}
        >
          <span class="flex-shrink-0" style:width="10px" />
          <div class="flex-1 min-w-0">
            <span class="text-zinc-200 font-medium">{name}</span>
            {#if dir}
              <span class="text-zinc-500 ml-1">{dir}/</span>
            {/if}
          </div>
          <span class="ml-auto flex-shrink-0 px-1 rounded text-[10px] {kindColors[f.kind] ?? kindColors.other}">
            {f.kind[0]}
          </span>
        </div>
      {/each}
      {#if searchResults.length === 0}
        <div class="px-3 py-4 text-zinc-500 text-center">Aucun résultat</div>
      {/if}
    {:else}
      <!-- Normal tree -->
      {#each rows as row (row.path)}
        <div
          role="button"
          tabindex="0"
          class="flex items-center gap-1 px-1 py-0.5 hover:bg-zinc-800 cursor-pointer select-none"
          style:padding-left="{4 + row.depth * 12}px"
          on:click={(e) =>
            row.isDir
              ? toggle(row.path)
              : dispatch("dragFile", { path: row.path, event: e })}
          on:keydown={(e) =>
            e.key === "Enter" &&
            (row.isDir
              ? toggle(row.path)
              : dispatch("dragFile", { path: row.path, event: e }))}
        >
          {#if row.isDir}
            <span class="text-zinc-400 flex-shrink-0" style:width="10px">
              {row.expanded ? "▾" : "▸"}
            </span>
            <span class="text-yellow-300 truncate">{row.name}/</span>
          {:else}
            <span class="flex-shrink-0" style:width="10px" />
            <span class="truncate text-zinc-300">{row.name}</span>
            {#if row.file}
              {@const dots = Math.min(6, Math.ceil(row.file.lineCount / 50))}
              <span class="ml-1 flex items-center gap-0.5 flex-shrink-0">
                {#each {length: dots} as _}
                  <span class="inline-block w-1 h-2 rounded-sm bg-zinc-600"></span>
                {/each}
                {#if row.file.lineCount > 300}
                  <span class="text-zinc-500 text-[9px]">…</span>
                {/if}
              </span>
              <span class="ml-auto flex-shrink-0 px-1 rounded text-[10px] {kindColors[row.file.kind] ?? kindColors.other}">
                {row.file.kind[0]}
              </span>
            {/if}
          {/if}
        </div>
      {/each}

      {#if files.length === 0}
        <div class="px-3 py-4 text-zinc-500 text-center">
          No workspace files yet…
        </div>
      {/if}
    {/if}
  </div>
  {/if}

</div>

<style lang="postcss">
  .panel-window {
    @apply inline-flex rounded-lg border border-zinc-700 bg-zinc-900 opacity-90;
    transition: opacity 200ms;
  }

  .panel-window:hover {
    @apply opacity-100;
  }
</style>
