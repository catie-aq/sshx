<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import type { WsDrawing } from "$lib/protocol";

  /** All drawings keyed by ID. */
  export let drawings: Map<number, WsDrawing>;
  /** Current canvas center [x, y]. */
  export let center: number[];
  /** Current zoom level. */
  export let zoom: number;
  /** Active drawing tool: null = none, "pencil", "highlighter". */
  export let activeTool: "pencil" | "highlighter" | null = null;
  /** Current drawing color. */
  export let color: string = "#ffffff";
  /** Can the user draw? */
  export let canWrite: boolean = false;

  const dispatch = createEventDispatcher<{
    create: WsDrawing;
    delete: number;
  }>();

  // In-progress stroke
  let currentPoints: number[] = [];
  let isDrawing = false;

  const PENCIL_WIDTH = 2;
  const HIGHLIGHTER_WIDTH = 24;
  const HIGHLIGHTER_OPACITY = 0.35;

  function screenToCanvas(clientX: number, clientY: number): [number, number] {
    const ox = 0.5 * window.innerWidth - 378;
    const oy = 0.5 * window.innerHeight - 240;
    return [
      center[0] + clientX / zoom - ox,
      center[1] + clientY / zoom - oy,
    ];
  }

  function handlePointerDown(e: PointerEvent) {
    if (!activeTool || !canWrite) return;
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    isDrawing = true;
    const [x, y] = screenToCanvas(e.clientX, e.clientY);
    currentPoints = [x, y];
    (e.target as Element)?.setPointerCapture?.(e.pointerId);
  }

  function handlePointerMove(e: PointerEvent) {
    if (!isDrawing) return;
    e.preventDefault();
    const [x, y] = screenToCanvas(e.clientX, e.clientY);
    currentPoints = [...currentPoints, x, y];
  }

  function handlePointerUp(e: PointerEvent) {
    if (!isDrawing) return;
    isDrawing = false;
    // Guard against NaN/Infinity from degenerate screenToCanvas (e.g. zoom=0)
    if (currentPoints.some(v => !Number.isFinite(v))) {
      currentPoints = [];
      return;
    }
    if (currentPoints.length >= 4) {
      const drawing: WsDrawing = {
        tool: activeTool!,
        points: currentPoints,
        color,
        width: activeTool === "highlighter" ? HIGHLIGHTER_WIDTH : PENCIL_WIDTH,
        opacity: activeTool === "highlighter" ? HIGHLIGHTER_OPACITY : 1.0,
      };
      dispatch("create", drawing);
    }
    currentPoints = [];
  }

  function pointsToPath(points: number[]): string {
    if (points.length < 4) return "";
    let d = `M ${points[0]} ${points[1]}`;
    for (let i = 2; i < points.length - 2; i += 2) {
      const cx = (points[i] + points[i + 2]) / 2;
      const cy = (points[i + 1] + points[i + 3]) / 2;
      d += ` Q ${points[i]} ${points[i + 1]} ${cx} ${cy}`;
    }
    d += ` L ${points[points.length - 2]} ${points[points.length - 1]}`;
    return d;
  }

  // View transform
  $: viewX = (window?.innerWidth ?? 800) / 2 - 378 - center[0] * zoom;
  $: viewY = (window?.innerHeight ?? 600) / 2 - 240 - center[1] * zoom;
</script>

<!-- svelte-ignore a11y-no-static-element-interactions -->
<svg
  class="drawing-layer"
  class:active={activeTool !== null}
  on:pointerdown={handlePointerDown}
  on:pointermove={handlePointerMove}
  on:pointerup={handlePointerUp}
>
  <g transform="translate({viewX}, {viewY}) scale({zoom})">
    <!-- Existing drawings -->
    {#each [...drawings] as [did, drawing] (did)}
      <path
        d={pointsToPath(drawing.points)}
        fill="none"
        stroke={drawing.color}
        stroke-width={drawing.width}
        stroke-linecap="round"
        stroke-linejoin="round"
        opacity={drawing.opacity}
      />
    {/each}

    <!-- Current stroke in progress -->
    {#if isDrawing && currentPoints.length >= 4}
      <path
        d={pointsToPath(currentPoints)}
        fill="none"
        stroke={color}
        stroke-width={activeTool === "highlighter" ? HIGHLIGHTER_WIDTH : PENCIL_WIDTH}
        stroke-linecap="round"
        stroke-linejoin="round"
        opacity={activeTool === "highlighter" ? HIGHLIGHTER_OPACITY : 1.0}
      />
    {/if}
  </g>
</svg>

<style>
  .drawing-layer {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 1;
    overflow: hidden;
  }

  .drawing-layer.active {
    pointer-events: auto;
    cursor: crosshair;
  }
</style>
