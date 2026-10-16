<script lang="ts">
  import { createEventDispatcher, tick } from "svelte";
  import type { WsWidget } from "../protocol";
  import {
    ANNOTATION_COLORS,
    annotatedFileName,
    fontSizeFor,
    parseAnnotations,
    strokePath,
    strokeWidthFor,
    workspaceImagePath,
    type ImageAnnotation,
  } from "../imageAnnotations";
  import CircleButton from "./CircleButton.svelte";
  import CircleButtons from "./CircleButtons.svelte";

  export let widget: WsWidget;
  export let canWrite: boolean;
  /** Absolute path of the CLI's working directory, when known. */
  export let workspaceRoot: string | null = null;

  const dispatch = createEventDispatcher<{
    startMove: MouseEvent;
    delete: void;
    rename: string;
    saveAnnotations: ImageAnnotation[];
  }>();

  let el: HTMLDivElement;
  let editing = false;
  let editValue = "";

  $: imageUrl = widget.kind.type === "image" ? widget.kind.url : "";
  $: imageAlt = widget.kind.type === "image" ? widget.kind.alt : "";
  $: annotationsJson = widget.kind.type === "image" ? widget.kind.annotations : undefined;
  $: hasAnnotatedCopy = widget.kind.type === "image" && !!widget.kind.annotatedUrl;
  $: displayName = widget.name ?? "Image";

  // Once annotated, the copied path points at the flattened copy so Claude sees the notes.
  $: relativePath = widget.name
    ? workspaceImagePath(
        hasAnnotatedCopy ? annotatedFileName(widget.name) : widget.name,
        workspaceRoot,
      )
    : null;

  let copied = false;

  function copyPath() {
    if (!relativePath) return;
    navigator.clipboard.writeText(relativePath).then(() => {
      copied = true;
      setTimeout(() => (copied = false), 1500);
    });
  }

  // --- Annotations ---------------------------------------------------------

  let naturalW = 0;
  let naturalH = 0;
  let svg: SVGSVGElement;

  let annotating = false;
  let tool: "pen" | "text" = "pen";
  let color = ANNOTATION_COLORS[0];
  let draft: ImageAnnotation[] = [];
  let currentStroke: ImageAnnotation | null = null;
  let textInput: { x: number; y: number; value: string } | null = null;
  let textInputEl: HTMLInputElement;

  /** Shown until the server echoes our save back, to avoid a flash of the old state. */
  let justSaved: ImageAnnotation[] | null = null;
  $: annotationsJson, (justSaved = null);

  $: saved = parseAnnotations(annotationsJson);
  $: shown = annotating
    ? currentStroke
      ? [...draft, currentStroke]
      : draft
    : justSaved ?? saved;

  function handleImageLoad(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    naturalW = img.naturalWidth;
    naturalH = img.naturalHeight;
  }

  function startAnnotating() {
    draft = [...saved];
    annotating = true;
    el?.focus();
  }

  function stopAnnotating() {
    commitText();
    annotating = false;
    currentStroke = null;
    textInput = null;
  }

  function saveAnnotations() {
    commitText();
    justSaved = draft;
    dispatch("saveAnnotations", draft);
    stopAnnotating();
  }

  function undo() {
    if (textInput) textInput = null;
    else draft = draft.slice(0, -1);
  }

  function toImagePoint(e: PointerEvent): [number, number] {
    const ctm = svg.getScreenCTM();
    if (!ctm) return [0, 0];
    const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(ctm.inverse());
    return [Math.round(p.x), Math.round(p.y)];
  }

  async function handlePointerDown(e: PointerEvent) {
    if (!annotating || e.button !== 0) return;
    e.preventDefault();
    const point = toImagePoint(e);
    if (tool === "pen") {
      svg.setPointerCapture(e.pointerId);
      currentStroke = { kind: "stroke", color, width: strokeWidthFor(naturalW), points: [point] };
    } else {
      commitText();
      textInput = { x: point[0], y: point[1], value: "" };
      await tick();
      textInputEl?.focus();
    }
  }

  function handlePointerMove(e: PointerEvent) {
    if (!currentStroke || currentStroke.kind !== "stroke") return;
    const point = toImagePoint(e);
    const last = currentStroke.points[currentStroke.points.length - 1];
    if (last[0] === point[0] && last[1] === point[1]) return;
    currentStroke = { ...currentStroke, points: [...currentStroke.points, point] };
  }

  function handlePointerUp() {
    if (!currentStroke) return;
    draft = [...draft, currentStroke];
    currentStroke = null;
  }

  function commitText() {
    if (!textInput) return;
    const text = textInput.value.trim();
    if (text) {
      draft = [
        ...draft,
        { kind: "text", color, size: fontSizeFor(naturalW), x: textInput.x, y: textInput.y, text },
      ];
    }
    textInput = null;
  }

  function handleTextKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Enter") {
      e.preventDefault();
      commitText();
    } else if (e.key === "Escape") {
      textInput = null;
    }
  }

  // --- Widget --------------------------------------------------------------

  function handleKeydown(e: KeyboardEvent) {
    if (!canWrite) return;
    if (editing) return;
    if (annotating) {
      if ((e.metaKey || e.ctrlKey) && e.key === "z") {
        e.preventDefault();
        undo();
      } else if (e.key === "Escape") {
        stopAnnotating();
      }
      // Never let Delete/Backspace remove the widget while drawing.
      e.stopPropagation();
      return;
    }
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      dispatch("delete");
    }
  }

  function handleMousedown(e: MouseEvent) {
    el?.focus();
    if (canWrite) dispatch("startMove", e);
  }

  function startEditing() {
    if (!canWrite) return;
    editing = true;
    editValue = widget.name ?? "";
  }

  function commitEdit() {
    editing = false;
    const name = editValue.trim();
    if (name && name !== (widget.name ?? "")) {
      dispatch("rename", name);
    }
  }

  function cancelEdit() {
    editing = false;
  }

  function handleEditKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      commitEdit();
    } else if (e.key === "Escape") {
      cancelEdit();
    }
  }
</script>

<!-- svelte-ignore a11y-no-noninteractive-tabindex -->
<div
  bind:this={el}
  class="image-widget"
  style:width="{widget.w}px"
  tabindex="0"
  on:keydown={handleKeydown}
>
  <!-- Title bar -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div
    class="flex select-none flex-shrink-0 cursor-grab active:cursor-grabbing items-center border-b border-zinc-700 bg-zinc-800/80"
    on:mousedown={handleMousedown}
  >
    <div class="flex-1 flex items-center px-2 py-1">
      <CircleButtons>
        <CircleButton
          kind="red"
          on:mousedown={(event) => event.button === 0 && dispatch("delete")}
        />
      </CircleButtons>
    </div>
    <div
      class="py-1 text-sm text-zinc-300 text-center overflow-hidden whitespace-nowrap text-ellipsis w-0 flex-grow-[4]"
    >
      {#if editing}
        <!-- svelte-ignore a11y-autofocus -->
        <input
          type="text"
          class="bg-zinc-700 text-zinc-200 text-sm text-center rounded px-1 w-full outline-none"
          bind:value={editValue}
          on:blur={commitEdit}
          on:keydown={handleEditKeydown}
          autofocus
        />
      {:else}
        <!-- svelte-ignore a11y-no-static-element-interactions -->
        <span
          class="cursor-text"
          on:dblclick|stopPropagation={startEditing}
          title="Double-click to rename"
        >{displayName}</span>
      {/if}
    </div>
    <div class="flex-1 flex justify-end gap-0.5 px-2">
      {#if canWrite && !annotating}
        <button
          class="icon-btn"
          title="Annotate (draw and add text)"
          aria-label="Annotate image"
          on:mousedown|stopPropagation
          on:click|stopPropagation={startAnnotating}
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
            <path d="M11 2.5l2.5 2.5L6 12.5H3.5V10z" />
          </svg>
        </button>
      {/if}
      {#if relativePath}
        <button
          class="icon-btn"
          class:text-emerald-400={copied}
          title={copied ? "Copied!" : `Copy path: ${relativePath}`}
          aria-label="Copy image path"
          on:mousedown|stopPropagation
          on:click|stopPropagation={copyPath}
        >
          {#if copied}
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
              <path d="M3 8.5l3 3 7-7" />
            </svg>
          {:else}
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
              <rect x="5" y="5" width="8.5" height="8.5" rx="1.5" />
              <path d="M10.5 5V3.5A1.5 1.5 0 009 2H3.5A1.5 1.5 0 002 3.5V9a1.5 1.5 0 001.5 1.5H5" />
            </svg>
          {/if}
        </button>
      {/if}
    </div>
  </div>

  <!-- Annotation toolbar -->
  {#if annotating}
    <div class="flex items-center gap-1 px-2 py-1 text-xs border-b border-zinc-700 bg-zinc-800">
      <button class="tool-btn" class:active={tool === "pen"} on:click={() => (tool = "pen")}>Draw</button>
      <button class="tool-btn" class:active={tool === "text"} on:click={() => (tool = "text")}>Text</button>
      <div class="flex items-center gap-1 mx-1">
        {#each ANNOTATION_COLORS as c}
          <button
            class="w-3.5 h-3.5 rounded-full border"
            class:border-zinc-100={color === c}
            class:border-transparent={color !== c}
            style:background-color={c}
            aria-label="Color {c}"
            on:click={() => (color = c)}
          />
        {/each}
      </div>
      <button class="tool-btn" disabled={draft.length === 0 && !textInput} on:click={undo}>Undo</button>
      <button class="tool-btn" disabled={draft.length === 0} on:click={() => (draft = [])}>Clear</button>
      <div class="flex-1" />
      <button class="tool-btn" on:click={stopAnnotating}>Cancel</button>
      <button class="tool-btn text-indigo-300" on:click={saveAnnotations}>Save</button>
    </div>
  {/if}

  <!-- Image -->
  <div class="relative flex-1 overflow-hidden min-h-0">
    {#if imageUrl}
      <img
        src={imageUrl}
        alt={imageAlt}
        class="block w-full h-full object-contain pointer-events-none"
        draggable="false"
        on:load={handleImageLoad}
      />
    {/if}
    {#if naturalW > 0 && (annotating || shown.length > 0)}
      <!-- svelte-ignore a11y-no-static-element-interactions -->
      <svg
        bind:this={svg}
        class="absolute inset-0 w-full h-full"
        class:pointer-events-none={!annotating}
        class:cursor-crosshair={annotating && tool === "pen"}
        class:cursor-text={annotating && tool === "text"}
        viewBox="0 0 {naturalW} {naturalH}"
        preserveAspectRatio="xMidYMid meet"
        on:pointerdown={handlePointerDown}
        on:pointermove={handlePointerMove}
        on:pointerup={handlePointerUp}
        on:pointercancel={handlePointerUp}
      >
        {#each shown as a}
          {#if a.kind === "stroke"}
            <path
              d={strokePath(a.points)}
              fill="none"
              stroke={a.color}
              stroke-width={a.width}
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          {:else}
            <text
              x={a.x}
              y={a.y}
              fill={a.color}
              font-size={a.size}
              font-weight="bold"
              font-family="sans-serif"
              dominant-baseline="hanging"
              stroke="#18181b"
              stroke-width={a.size / 5}
              paint-order="stroke"
            >{a.text}</text>
          {/if}
        {/each}
      </svg>
    {/if}
    {#if textInput && naturalW > 0}
      <input
        bind:this={textInputEl}
        bind:value={textInput.value}
        class="absolute min-w-[8rem] bg-zinc-900/80 text-sm rounded px-1 outline-none border border-zinc-600"
        style:left="{(textInput.x / naturalW) * 100}%"
        style:top="{(textInput.y / naturalH) * 100}%"
        style:color={color}
        placeholder="Text, then Enter"
        on:keydown={handleTextKeydown}
        on:blur={commitText}
        on:pointerdown|stopPropagation
      />
    {/if}
  </div>
</div>

<style lang="postcss">
  .image-widget {
    @apply inline-flex flex-col rounded border border-zinc-600 bg-zinc-900 overflow-hidden cursor-default;
    outline: none;
  }
  .image-widget:focus {
    @apply ring-2 ring-indigo-400;
  }
  .icon-btn {
    @apply p-0.5 rounded text-zinc-400 hover:text-zinc-100 hover:bg-zinc-700;
  }
  .tool-btn {
    @apply px-1.5 py-0.5 rounded text-zinc-300 hover:bg-zinc-700 disabled:opacity-40 disabled:hover:bg-transparent;
  }
  .tool-btn.active {
    @apply bg-zinc-700 text-zinc-100;
  }
</style>
