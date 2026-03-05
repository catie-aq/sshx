<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import CircleButton from "./CircleButton.svelte";
  import CircleButtons from "./CircleButtons.svelte";
  import type { WsWidget, WsComponentGraph } from "../protocol";
  import type { RuntimeNode, RuntimeEdge } from "../runtimeGraph";

  export let widget: WsWidget;
  export let componentGraph: WsComponentGraph | null;
  export let runtimeGraph: { nodes: RuntimeNode[]; edges: RuntimeEdge[] };

  const dispatch = createEventDispatcher<{
    startMove: MouseEvent;
    delete: void;
    navigateTo: string;
  }>();

  // SVG internal pan/zoom state
  let svgEl: SVGSVGElement;
  let vbX = 0;
  let vbY = 0;
  let vbW = 640;
  let vbH = 480;

  // Force-layout node type
  type ForceNode = {
    id: string;
    label: string;
    category: "file" | "claude" | "terminal";
    x: number;
    y: number;
    vx: number;
    vy: number;
    fixed: boolean;
  };

  type DrawEdge = {
    fromId: string;
    toId: string;
    color: string;
  };

  let forceNodes: ForceNode[] = [];
  let drawEdges: DrawEdge[] = [];
  let draggingNode: ForceNode | null = null;
  let prevNodeCount = 0;
  let prevEdgeCount = 0;

  // Rebuild force nodes when graph data changes
  $: {
    const allNodes = new Map<string, ForceNode>();
    const allEdges: DrawEdge[] = [];

    // Gather file nodes + code edges from component graph
    if (componentGraph) {
      for (const [path, node] of Object.entries(componentGraph.nodes)) {
        const id = `file:${path}`;
        const existing = forceNodes.find((n) => n.id === id);
        allNodes.set(id, existing ?? {
          id,
          label: node.label,
          category: "file",
          x: vbW * 0.5 + (Math.random() - 0.5) * 200,
          y: vbH * 0.5 + (Math.random() - 0.5) * 200,
          vx: 0,
          vy: 0,
          fixed: false,
        });
      }
      for (const edge of componentGraph.codeEdges) {
        allEdges.push({
          fromId: `file:${edge.from}`,
          toId: `file:${edge.to}`,
          color: "#52525b", // zinc-600
        });
      }
    }

    // Runtime nodes + edges
    for (const rn of runtimeGraph.nodes) {
      if (!allNodes.has(rn.id)) {
        const existing = forceNodes.find((n) => n.id === rn.id);
        allNodes.set(rn.id, existing ?? {
          id: rn.id,
          label: rn.label,
          category: rn.category,
          x: vbW * 0.5 + (Math.random() - 0.5) * 200,
          y: vbH * 0.5 + (Math.random() - 0.5) * 200,
          vx: 0,
          vy: 0,
          fixed: false,
        });
      }
    }
    for (const re of runtimeGraph.edges) {
      allEdges.push({
        fromId: re.from,
        toId: re.to,
        color: re.kind === "claude_file" ? "#6366f1" : "#06b6d4",
      });
    }

    // Deduplicate edges: keep only the first occurrence of each from→to pair
    // per color so the SVG isn't cluttered with overlapping lines.
    const seenEdges = new Set<string>();
    const uniqueEdges: typeof allEdges = [];
    for (const e of allEdges) {
      const key = `${e.color}:${e.fromId}→${e.toId}`;
      if (!seenEdges.has(key)) { seenEdges.add(key); uniqueEdges.push(e); }
    }

    const nodeList = [...allNodes.values()];
    const nodeCount = nodeList.length;
    const edgeCount = uniqueEdges.length;

    // Only re-run full simulation when topology changes
    if (nodeCount !== prevNodeCount || edgeCount !== prevEdgeCount) {
      prevNodeCount = nodeCount;
      prevEdgeCount = edgeCount;
      runForce(nodeList, uniqueEdges, nodeCount > 80 ? 50 : 120);
    }

    forceNodes = nodeList;
    drawEdges = uniqueEdges;
  }

  function runForce(nodes: ForceNode[], edges: DrawEdge[], iterations: number) {
    if (nodes.length === 0) return;
    const repK = 4000; // Coulomb repulsion coefficient
    const springK = 0.04; // Hooke spring constant
    const restLen = 100; // spring rest length
    const damping = 0.85;

    for (let iter = 0; iter < iterations; iter++) {
      // Coulomb repulsion between all pairs
      for (let i = 0; i < nodes.length; i++) {
        for (let j = i + 1; j < nodes.length; j++) {
          const a = nodes[i];
          const b = nodes[j];
          if (a.fixed && b.fixed) continue;
          const dx = b.x - a.x;
          const dy = b.y - a.y;
          const d2 = dx * dx + dy * dy + 1;
          const d = Math.sqrt(d2);
          const f = repK / d2;
          const fx = (dx / d) * f;
          const fy = (dy / d) * f;
          if (!a.fixed) { a.vx -= fx; a.vy -= fy; }
          if (!b.fixed) { b.vx += fx; b.vy += fy; }
        }
      }

      // Hooke spring attraction along edges
      const nodeMap = new Map(nodes.map((n) => [n.id, n]));
      for (const edge of edges) {
        const a = nodeMap.get(edge.fromId);
        const b = nodeMap.get(edge.toId);
        if (!a || !b) continue;
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const d = Math.sqrt(dx * dx + dy * dy) || 1;
        const f = springK * (d - restLen);
        const fx = (dx / d) * f;
        const fy = (dy / d) * f;
        if (!a.fixed) { a.vx += fx; a.vy += fy; }
        if (!b.fixed) { b.vx -= fx; b.vy -= fy; }
      }

      // Integrate positions with damping and boundary clamping
      for (const node of nodes) {
        if (node.fixed) continue;
        node.vx *= damping;
        node.vy *= damping;
        node.x = Math.max(50, Math.min(vbW - 50, node.x + node.vx));
        node.y = Math.max(20, Math.min(vbH - 20, node.y + node.vy));
      }
    }
  }

  // Convert a mouse event in screen coords to SVG viewbox coords
  function toSvgCoords(e: MouseEvent): { x: number; y: number } {
    if (!svgEl) return { x: 0, y: 0 };
    const pt = svgEl.createSVGPoint();
    pt.x = e.clientX;
    pt.y = e.clientY;
    const ctm = svgEl.getScreenCTM();
    if (!ctm) return { x: 0, y: 0 };
    const svgPt = pt.matrixTransform(ctm.inverse());
    return { x: svgPt.x, y: svgPt.y };
  }

  // Node drag
  function startNodeDrag(e: MouseEvent, node: ForceNode) {
    e.stopPropagation();
    if (e.button !== 0) return;
    node.fixed = true;
    draggingNode = node;
    function onMove(ev: MouseEvent) {
      if (!draggingNode) return;
      const { x, y } = toSvgCoords(ev);
      draggingNode.x = x;
      draggingNode.y = y;
      forceNodes = forceNodes; // trigger reactivity
    }
    function onUp() {
      if (draggingNode) {
        draggingNode.fixed = false;
        draggingNode = null;
      }
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    }
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  // SVG pan
  let panStart: { mx: number; my: number; vbX: number; vbY: number } | null = null;

  function startPan(e: MouseEvent) {
    if (e.button !== 0 || e.target !== svgEl) return;
    panStart = { mx: e.clientX, my: e.clientY, vbX, vbY };
    function onMove(ev: MouseEvent) {
      if (!panStart) return;
      const scaleX = vbW / svgEl.clientWidth;
      const scaleY = vbH / svgEl.clientHeight;
      vbX = panStart.vbX - (ev.clientX - panStart.mx) * scaleX;
      vbY = panStart.vbY - (ev.clientY - panStart.my) * scaleY;
    }
    function onUp() {
      panStart = null;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    }
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  // SVG zoom (wheel event — stop propagation so canvas doesn't zoom)
  function onWheel(e: WheelEvent) {
    e.stopPropagation();
    const factor = e.deltaY > 0 ? 1.1 : 0.9;
    const { x, y } = toSvgCoords(e as unknown as MouseEvent);
    vbW *= factor;
    vbH *= factor;
    vbX = x - (x - vbX) * factor;
    vbY = y - (y - vbY) * factor;
  }

  // Node colors by category
  const nodeFill: Record<string, string> = {
    file: "#27272a",    // zinc-800
    claude: "#1e1b4b",  // indigo-950
    terminal: "#083344", // cyan-950
  };
  const nodeStroke: Record<string, string> = {
    file: "#71717a",    // zinc-500
    claude: "#6366f1",  // indigo-500
    terminal: "#06b6d4", // cyan-500
  };
</script>

<!-- Widget outer frame -->
<div
  class="rounded-lg border border-zinc-700 bg-zinc-900 flex flex-col opacity-90 hover:opacity-100 transition-opacity duration-200"
  style:width="{widget.w}px"
  style:height="{widget.h}px"
  on:pointerdown|stopPropagation={() => {}}
  on:wheel|stopPropagation={() => {}}
>
  <!-- Title bar -->
  <div
    class="flex select-none items-center px-3 py-1.5 bg-zinc-800 rounded-t border-b border-zinc-700 cursor-grab active:cursor-grabbing flex-shrink-0"
    on:mousedown={(e) => dispatch("startMove", e)}
  >
    <!-- Left: CircleButtons -->
    <div class="flex-1 flex items-center">
      <CircleButtons>
        <CircleButton kind="red" on:click={() => dispatch("delete")} />
      </CircleButtons>
    </div>
    <!-- Center: title -->
    <div class="w-0 flex-grow-[4] text-sm text-zinc-300 text-center font-medium overflow-hidden whitespace-nowrap text-ellipsis">
      Component Graph
    </div>
    <!-- Right: node count -->
    <div class="flex-1 flex items-center justify-end">
      <span class="text-xs text-zinc-500">{forceNodes.length} nodes</span>
    </div>
  </div>

  <!-- SVG body -->
  <div class="flex-1 overflow-hidden rounded-b">
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <svg
      bind:this={svgEl}
      width="100%"
      height="100%"
      viewBox="{vbX} {vbY} {vbW} {vbH}"
      class="w-full h-full bg-zinc-900"
      on:mousedown={startPan}
      on:wheel={onWheel}
    >
      <!-- Edges -->
      {#each drawEdges as edge, ei (ei)}
        {@const a = forceNodes.find((n) => n.id === edge.fromId)}
        {@const b = forceNodes.find((n) => n.id === edge.toId)}
        {#if a && b}
          <line
            x1={a.x}
            y1={a.y}
            x2={b.x}
            y2={b.y}
            stroke={edge.color}
            stroke-width="1.5"
            stroke-opacity="0.6"
          />
        {/if}
      {/each}

      <!-- Nodes -->
      {#each forceNodes as node (node.id)}
        <!-- svelte-ignore a11y-no-static-element-interactions -->
        <g
          transform="translate({node.x},{node.y})"
          class="cursor-pointer"
          on:mousedown={(e) => startNodeDrag(e, node)}
          on:dblclick={() => {
            if (node.category === "file") {
              dispatch("navigateTo", node.id.replace(/^file:/, ""));
            }
          }}
        >
          <rect
            x="-40"
            y="-12"
            width="80"
            height="24"
            rx="4"
            fill={nodeFill[node.category] ?? "#27272a"}
            stroke={nodeStroke[node.category] ?? "#71717a"}
            stroke-width="1"
          />
          <text
            text-anchor="middle"
            dominant-baseline="middle"
            font-size="10"
            fill="#d4d4d8"
            pointer-events="none"
          >
            {node.label.length > 12 ? node.label.slice(0, 11) + "…" : node.label}
          </text>
        </g>
      {/each}

      {#if forceNodes.length === 0}
        <text
          x={vbX + vbW / 2}
          y={vbY + vbH / 2}
          text-anchor="middle"
          dominant-baseline="middle"
          font-size="12"
          fill="#52525b"
        >No graph data yet.</text>
      {/if}
    </svg>
  </div>
</div>
