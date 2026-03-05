<script lang="ts">
  import type { WsWidget, WsSourceFile, WsComponentGraph } from "../protocol";

  export let center: number[];
  export let zoom: number;
  export let widgets: Map<number, WsWidget>;
  export let sourceFiles: Map<string, WsSourceFile>;
  export let componentGraph: WsComponentGraph | null;

  const CONSTANT_OFFSET_LEFT = 378;
  const CONSTANT_OFFSET_TOP = 240;
  const COLLAPSED_W = 280;
  const COLLAPSED_H = 60; // approximate collapsed card height (header + one line)

  /** Convert canvas coordinates to SVG pixel coordinates within fabricEl. */
  function toScreen(cx: number, cy: number): [number, number] {
    if (typeof window === "undefined") return [0, 0];
    return [
      zoom * (window.innerWidth / 2 - CONSTANT_OFFSET_LEFT + cx - center[0]),
      zoom * (window.innerHeight / 2 - CONSTANT_OFFSET_TOP + cy - center[1]),
    ];
  }

  function bezierPath(x1: number, y1: number, x2: number, y2: number): string {
    const cp = Math.max(40, Math.abs(x2 - x1) * 0.5);
    return `M${x1},${y1} C${x1 + cp},${y1} ${x2 - cp},${y2} ${x2},${y2}`;
  }

  interface Edge {
    id: string;
    d: string;
  }

  // Build path → widget lookup for open FileCards.
  $: fileCardMap = (() => {
    const m = new Map<string, WsWidget>();
    for (const w of widgets.values()) {
      if (w.kind.type === "fileCard") m.set(w.kind.path, w);
    }
    return m;
  })();

  // Reactive edge list — depends on center/zoom/widgets/componentGraph.
  $: fileEdges = (() => {
    // Reference center and zoom so Svelte reacts to pan/zoom changes.
    void center;
    void zoom;
    if (!componentGraph) return [] as Edge[];

    const seen = new Set<string>();
    const edges: Edge[] = [];

    for (const edge of componentGraph.codeEdges) {
      const fromW = fileCardMap.get(edge.from);
      const toW = fileCardMap.get(edge.to);
      if (!fromW || !toW) continue;

      const id = `${edge.from}→${edge.to}`;
      if (seen.has(id)) continue;
      seen.add(id);

      const fromCardW = fromW.collapsed ? COLLAPSED_W : fromW.w;
      const fromCardH = fromW.collapsed ? COLLAPSED_H : fromW.h;
      const toCardH = toW.collapsed ? COLLAPSED_H : toW.h;

      const [x1, y1] = toScreen(fromW.x + fromCardW, fromW.y + fromCardH / 2);
      const [x2, y2] = toScreen(toW.x, toW.y + toCardH / 2);

      edges.push({ id, d: bezierPath(x1, y1, x2, y2) });
    }
    return edges;
  })();
</script>

<svg
  class="absolute inset-0 w-full h-full"
  style="pointer-events: none; overflow: visible;"
>
  {#each fileEdges as e (e.id)}
    <path
      d={e.d}
      fill="none"
      stroke="#71717a"
      stroke-width="1.5"
      stroke-opacity="0.55"
      stroke-linecap="round"
    />
  {/each}
</svg>
