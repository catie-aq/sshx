<script lang="ts">
  import {
    onDestroy,
    onMount,
    tick,
    beforeUpdate,
    afterUpdate,
    createEventDispatcher,
  } from "svelte";
  import { fade } from "svelte/transition";
  import { debounce, throttle } from "lodash-es";

  import { Encrypt } from "./encrypt";
  import { createLock } from "./lock";
  import { Srocket } from "./srocket";
  import type { WsClient, WsClaudeEvent, WsDrawing, WsFileMetadataUpdate, WsIceCandidate, WsNote, WsServer, WsSlide, WsSourceFile, WsTextBlock, WsUser, WsVideoStream, WsWidget, WsWinsize } from "./protocol";
  import { makeToast } from "./toast";
  import Chat, { type ChatMessage } from "./ui/Chat.svelte";
  import ChooseName from "./ui/ChooseName.svelte";
  import NameList from "./ui/NameList.svelte";
  import NetworkInfo from "./ui/NetworkInfo.svelte";
  import Settings from "./ui/Settings.svelte";
  import Toolbar from "./ui/Toolbar.svelte";
  import StickyNote from "./ui/StickyNote.svelte";
  import XTerm from "./ui/XTerm.svelte";
  import Avatars from "./ui/Avatars.svelte";
  import LiveCursor from "./ui/LiveCursor.svelte";
  import FileTreePanel from "./ui/FileTreePanel.svelte";
  import FileCard from "./ui/FileCard.svelte";
  import ClaudeActivityFeed from "./ui/ClaudeActivityFeed.svelte";
  import ClaudeExecutionGraph from "./ui/ClaudeExecutionGraph.svelte";
  import LibraryCard from "./ui/LibraryCard.svelte";
  import CommandPalette from "./ui/CommandPalette.svelte";
  import ScreenShareWidget from "./ui/ScreenShareWidget.svelte";
  import ImageWidget from "./ui/ImageWidget.svelte";
  import AppOverlayWidget from "./ui/AppOverlayWidget.svelte";
  import IdeEditorWidget from "./ui/IdeEditorWidget.svelte";
  import CircleButton from "./ui/CircleButton.svelte";
  import CircleButtons from "./ui/CircleButtons.svelte";
  import TextBlock from "./ui/TextBlock.svelte";
  import DrawingLayer from "./ui/DrawingLayer.svelte";
  import SlideRegion from "./ui/SlideRegion.svelte";
  import Timeline from "./ui/Timeline.svelte";
  import ContextMenu from "./ui/ContextMenu.svelte";
  import OpenFileDialog from "./ui/OpenFileDialog.svelte";
  import type { SearchItem } from "./protocol";
  import { slide } from "./action/slide";
  import { TouchZoom, INITIAL_ZOOM } from "./action/touchZoom";
  import { arrangeNewTerminal } from "./arrange";
  import { settings } from "./settings";
  import { EyeIcon } from "svelte-feather-icons";
  import "$lib/fonts"; // Load all font CSS
  import { type SelectionItem, type SelectionRect, normalizeRect, rectContainsPoint } from "$lib/selection";
  import { UndoHistory, type HistoryAction } from "$lib/history";

  export let id: string;

  const dispatch = createEventDispatcher<{ receiveName: string }>();

  // The magic numbers "left" and "top" are used to approximately center the
  // terminal at the time that it is first created.
  const CONSTANT_OFFSET_LEFT = 378;
  const CONSTANT_OFFSET_TOP = 240;

  const OFFSET_LEFT_CSS = `calc(50vw - ${CONSTANT_OFFSET_LEFT}px)`;
  const OFFSET_TOP_CSS = `calc(50vh - ${CONSTANT_OFFSET_TOP}px)`;
  const OFFSET_TRANSFORM_ORIGIN_CSS = `calc(-1 * ${OFFSET_LEFT_CSS}) calc(-1 * ${OFFSET_TOP_CSS})`;

  // Terminal width and height limits.
  const TERM_MIN_ROWS = 8;
  const TERM_MIN_COLS = 32;

  function getConstantOffset() {
    return [
      0.5 * window.innerWidth - CONSTANT_OFFSET_LEFT,
      0.5 * window.innerHeight - CONSTANT_OFFSET_TOP,
    ];
  }

  let fabricEl: HTMLElement;
  let touchZoom: TouchZoom;
  let center = [0, 0];
  let zoom = INITIAL_ZOOM;

  let showChat = false; // @hmr:keep
  let settingsOpen = false; // @hmr:keep
  let showNetworkInfo = false; // @hmr:keep
  let commandPaletteOpen = false; // @hmr:keep
  let contextMenuVisible = false; // @hmr:keep
  let contextMenuX = 0;
  let contextMenuY = 0;
  let contextMenuCanvasPos: [number, number] = [0, 0];
  let filePickerOpen = false;
  let filePickerScreenPos: [number, number] = [0, 0];
  let filePickerCanvasPos: [number, number] = [0, 0];
  let shellNames = new Map<number, string>(); // local-only terminal labels

  onMount(() => {
    function handleGlobalKey(e: KeyboardEvent) {
      // Escape: exit fullscreen IDE
      if (e.key === "Escape" && fullscreenIdeWid !== null) {
        fullscreenIdeWid = null;
        e.preventDefault();
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        commandPaletteOpen = true;
      }
      // When a terminal is focused, let it capture all keys except Ctrl+K (command palette).
      if (focused.length > 0) return;
      // Edition hotkeys — auto-activate edition major mode
      if (!e.metaKey && !e.ctrlKey && !e.altKey) {
        const tag = (e.target as HTMLElement)?.tagName;
        const editable = (e.target as HTMLElement)?.isContentEditable;
        if (tag !== "INPUT" && tag !== "TEXTAREA" && !editable) {
          if (e.key === "t") {
            e.preventDefault();
            majorMode = "edition";
            textToolActive = !textToolActive;
            if (!textToolActive) textToolGhost = null;
            else { drawingTool = null; }
          }
          if (e.key === "n") {
            e.preventDefault();
            majorMode = "edition";
            handleCreateNote();
          }
          if (e.key === "l") {
            e.preventDefault();
            majorMode = "edition";
            drawingTool = drawingTool === "pencil" ? null : "pencil";
            if (drawingTool) { textToolActive = false; textToolGhost = null; }
          }
          if (e.key === "h" && !e.shiftKey) {
            e.preventDefault();
            majorMode = "edition";
            drawingTool = drawingTool === "highlighter" ? null : "highlighter";
            if (drawingTool) { textToolActive = false; textToolGhost = null; }
          }
        }
      }
      if (e.key === "Escape" && textToolActive) {
        textToolActive = false;
        textToolGhost = null;
      }
      if (e.key === "Escape" && drawingTool) {
        drawingTool = null;
      }
      if (e.key === "Escape" && slideshowPlaying) {
        stopSlideshowPlay();
      }
      // Arrow key navigation in slideshow play mode
      if (slideshowPlaying) {
        if (e.key === "ArrowRight" || e.key === "ArrowDown") {
          e.preventDefault();
          navigateToSlide(Math.min(currentSlideIndex + 1, sortedSlides.length - 1));
        }
        if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
          e.preventDefault();
          navigateToSlide(Math.max(currentSlideIndex - 1, 0));
        }
      }
      if (e.key === "Escape" && selectedItems.length > 0) {
        selectedItems = [];
      }
      // Ctrl+C: copy selected items
      if ((e.metaKey || e.ctrlKey) && e.key === "c" && selectedItems.length > 0) {
        const tag = (e.target as HTMLElement)?.tagName;
        const editable = (e.target as HTMLElement)?.isContentEditable;
        if (tag === "INPUT" || tag === "TEXTAREA" || editable) return;
        e.preventDefault();
        clipboard = selectedItems.map((item) => {
          if (item.type === "note") return { type: "note", data: { ...notes.get(item.id) } };
          if (item.type === "textBlock") return { type: "textBlock", data: { ...textBlocks.get(item.id) } };
          if (item.type === "drawing") return { type: "drawing", data: { ...drawings.get(item.id) } };
          return { type: item.type, data: null };
        }).filter((c) => c.data !== null);
      }
      // Ctrl+V: paste clipboard
      if ((e.metaKey || e.ctrlKey) && e.key === "v" && clipboard.length > 0) {
        const tag = (e.target as HTMLElement)?.tagName;
        const editable = (e.target as HTMLElement)?.isContentEditable;
        if (tag === "INPUT" || tag === "TEXTAREA" || editable) return;
        e.preventDefault();
        const offset = 40;
        for (const item of clipboard) {
          if (item.type === "note" && item.data) {
            srocket?.send({ createNote: [item.data.x + offset, item.data.y + offset] });
          } else if (item.type === "textBlock" && item.data) {
            srocket?.send({ createTextBlock: [item.data.x + offset, item.data.y + offset] });
          } else if (item.type === "drawing" && item.data) {
            const d = item.data;
            const shifted = [...d.points];
            for (let i = 0; i < shifted.length; i += 2) {
              shifted[i] += offset;
              shifted[i + 1] += offset;
            }
            srocket?.send({ createDrawing: { ...d, points: shifted } });
          }
        }
      }
      // Ctrl+Z: undo
      if ((e.metaKey || e.ctrlKey) && e.key === "z" && !e.shiftKey) {
        const tag = (e.target as HTMLElement)?.tagName;
        const editable = (e.target as HTMLElement)?.isContentEditable;
        if (tag === "INPUT" || tag === "TEXTAREA" || editable) return;
        e.preventDefault();
        const action = undoHistory.undo();
        if (action) applyUndoAction(action);
      }
      // Ctrl+Shift+Z: redo
      if ((e.metaKey || e.ctrlKey) && e.key === "z" && e.shiftKey) {
        const tag = (e.target as HTMLElement)?.tagName;
        const editable = (e.target as HTMLElement)?.isContentEditable;
        if (tag === "INPUT" || tag === "TEXTAREA" || editable) return;
        e.preventDefault();
        const action = undoHistory.redo();
        if (action) applyRedoAction(action);
      }
      // Delete/Backspace: delete selected items
      if ((e.key === "Delete" || e.key === "Backspace") && selectedItems.length > 0) {
        const tag = (e.target as HTMLElement)?.tagName;
        const editable = (e.target as HTMLElement)?.isContentEditable;
        if (tag === "INPUT" || tag === "TEXTAREA" || editable) return;
        e.preventDefault();
        for (const item of selectedItems) {
          if (item.type === "note") srocket?.send({ deleteNote: item.id });
          else if (item.type === "textBlock") srocket?.send({ deleteTextBlock: item.id });
          else if (item.type === "widget") srocket?.send({ closeWidget: item.id });
          else if (item.type === "drawing") srocket?.send({ deleteDrawing: item.id });
        }
        selectedItems = [];
      }
    }
    window.addEventListener("keydown", handleGlobalKey);
    return () => window.removeEventListener("keydown", handleGlobalKey);
  });

  onMount(() => {
    touchZoom = new TouchZoom(fabricEl);
    touchZoom.onMove(() => {
      center = touchZoom.center;
      zoom = touchZoom.zoom;

      // Blur if the user is currently focused on a terminal.
      //
      // This makes it so that panning does not stop when the cursor happens to
      // intersect with the textarea, which absorbs wheel and touch events.
      if (document.activeElement) {
        const classList = [...document.activeElement.classList];
        if (classList.includes("xterm-helper-textarea")) {
          (document.activeElement as HTMLElement).blur();
        }
      }

      showNetworkInfo = false;
    });
  });

  /** Returns the mouse position in infinite grid coordinates, offset transformations and zoom. */
  function normalizePosition(event: MouseEvent): [number, number] {
    const [ox, oy] = getConstantOffset();
    return [
      Math.round(center[0] + event.pageX / zoom - ox),
      Math.round(center[1] + event.pageY / zoom - oy),
    ];
  }

  let encrypt: Encrypt;
  let srocket: Srocket<WsServer, WsClient> | null = null;

  let connected = false;
  let exitReason: string | null = null;

  /** Bound "write" method for each terminal. */
  const writers: Record<number, (data: string) => void> = {};
  const termWrappers: Record<number, HTMLDivElement> = {};
  const termElements: Record<number, HTMLDivElement> = {};
  const chunknums: Record<number, number> = {};
  const locks: Record<number, any> = {};
  let userId = 0;
  let users: [number, WsUser][] = [];
  let shells: [number, WsWinsize][] = [];
  let subscriptions = new Set<number>();

  // May be undefined before `users` is first populated.
  $: hasWriteAccess = users.find(([uid]) => uid === userId)?.[1]?.canWrite;

  let moving = -1; // Terminal ID that is being dragged.
  let movingOrigin = [0, 0]; // Coordinates of mouse at origin when drag started.
  let movingSize: WsWinsize; // New [x, y] position of the dragged terminal.
  let movingIsDone = false; // Moving finished but hasn't been acknowledged.

  let resizing = -1; // Terminal ID that is being resized.
  let resizingOrigin = [0, 0]; // Coordinates of top-left origin when resize started.
  let resizingCell = [0, 0]; // Pixel dimensions of a single terminal cell.
  let resizingSize: WsWinsize; // Last resize message sent.

  let majorMode: "none" | "edition" | "slides" = "none";
  let workspaceOpen = false;
  let notes = new Map<number, WsNote>(); // Nid → WsNote
  let movingNote: number | null = null; // Nid being dragged
  let movingNoteOrigin = [0, 0]; // [dx, dy] offset from note origin
  let movingNotePos: { x: number; y: number } | null = null;

  // Note resize state
  let resizingNote: { nid: number; startW: number; startH: number; startX: number; startY: number } | null = null;

  function startNoteResize(e: MouseEvent, nid: number, note: WsNote) {
    const w = note.w || 260;
    const h = note.h || 160;
    resizingNote = { nid, startW: w, startH: h, startX: e.clientX, startY: e.clientY };
    window.addEventListener("pointermove", onNoteResizeMove);
    window.addEventListener("pointerup", onNoteResizeEnd, { once: true });
  }

  function onNoteResizeMove(e: PointerEvent) {
    if (!resizingNote) return;
    const dw = (e.clientX - resizingNote.startX) / zoom;
    const dh = (e.clientY - resizingNote.startY) / zoom;
    const w = Math.max(140, resizingNote.startW + dw);
    const h = Math.max(80, resizingNote.startH + dh);
    const note = notes.get(resizingNote.nid);
    if (note) { note.w = Math.round(w); note.h = Math.round(h); notes = notes; }
  }

  function onNoteResizeEnd(e: PointerEvent) {
    if (!resizingNote) return;
    const dw = (e.clientX - resizingNote.startX) / zoom;
    const dh = (e.clientY - resizingNote.startY) / zoom;
    const w = Math.round(Math.max(140, resizingNote.startW + dw));
    const h = Math.round(Math.max(80, resizingNote.startH + dh));
    const note = notes.get(resizingNote.nid);
    if (note) {
      srocket?.send({ updateNote: [resizingNote.nid, { ...note, w, h }] });
    }
    resizingNote = null;
    window.removeEventListener("pointermove", onNoteResizeMove);
  }

  let textBlocks = new Map<number, WsTextBlock>(); // Tid → WsTextBlock
  let movingTextBlock: number | null = null;
  let movingTextBlockOrigin = [0, 0];
  let movingTextBlockPos: { x: number; y: number } | null = null;
  let textToolActive = false;
  let textToolGhost: { x: number; y: number } | null = null;
  let pendingAutoFocusPos: [number, number] | null = null;
  let pendingAutoFocusTid: number | null = null;

  // Drawing state
  let drawings = new Map<number, WsDrawing>();
  let drawingTool: "pencil" | "highlighter" | null = null;
  let drawingColor = "#ffffff";

  // Slide state
  let slides = new Map<number, WsSlide>();
  let slideshowMode = false;
  let slideshowPlaying = false;
  let currentSlideIndex = 0;
  let movingSlide: number | null = null;
  let movingSlideOrigin = [0, 0];
  let movingSlidePos: { x: number; y: number } | null = null;
  let resizingSlide: { slid: number; startW: number; startH: number; startX: number; startY: number } | null = null;

  $: sortedSlides = [...slides.entries()].sort(([, a], [, b]) => a.order - b.order);

  function handleCreateSlide() {
    const [ox, oy] = getConstantOffset();
    const cx = Math.round(center[0] + window.innerWidth / 2 / zoom - ox);
    const cy = Math.round(center[1] + window.innerHeight / 2 / zoom - oy);
    const order = slides.size + 1;
    srocket?.send({ createSlide: { x: cx - 480, y: cy - 270, w: 960, h: 540, order, label: "" } });
  }

  function navigateToSlide(index: number) {
    const sorted = sortedSlides;
    if (index < 0 || index >= sorted.length) return;
    currentSlideIndex = index;
    const [, slide] = sorted[index];
    // Center viewport on slide, compensating for the constant CSS offset
    const zoomTarget = Math.min(
      window.innerWidth / slide.w,
      window.innerHeight / slide.h,
    ) * 0.85;
    touchZoom.moveTo([
      slide.x + slide.w / 2 - CONSTANT_OFFSET_LEFT / zoomTarget,
      slide.y + slide.h / 2 - CONSTANT_OFFSET_TOP / zoomTarget,
    ], zoomTarget);
  }

  function startSlideshowPlay() {
    slideshowPlaying = true;
    if (sortedSlides.length > 0) navigateToSlide(0);
  }

  function stopSlideshowPlay() {
    slideshowPlaying = false;
  }

  let pdfExporting = false;

  async function handleExportPdf() {
    if (pdfExporting || sortedSlides.length === 0) return;
    pdfExporting = true;

    try {
      const [{ default: html2canvas }, { jsPDF }] = await Promise.all([
        import("html2canvas"),
        import("jspdf"),
      ]);

      // Save current viewport state
      const savedCenter = [...touchZoom.center];
      const savedZoom = touchZoom.zoom;

      // Hide UI overlays during capture
      const overlays = fabricEl.parentElement?.querySelectorAll<HTMLElement>(
        ".absolute.top-20, .absolute.bottom-4, .absolute.top-4"
      );
      overlays?.forEach((el) => (el.style.visibility = "hidden"));

      const captures: { canvas: HTMLCanvasElement; w: number; h: number }[] = [];

      for (let i = 0; i < sortedSlides.length; i++) {
        const [, sl] = sortedSlides[i];

        // Navigate to slide (this animates over 350ms)
        navigateToSlide(i);
        // Wait for animation + render settle
        await new Promise((r) => setTimeout(r, 500));
        await tick();

        // Capture the full viewport
        const captured = await html2canvas(fabricEl, {
          backgroundColor: null,
          scale: 2,
          useCORS: true,
          logging: false,
        });

        captures.push({ canvas: captured, w: sl.w, h: sl.h });
      }

      // Restore UI overlays
      overlays?.forEach((el) => (el.style.visibility = ""));

      // Restore viewport
      touchZoom.center = savedCenter;
      touchZoom.zoom = savedZoom;

      // Build PDF — use first slide's aspect ratio as page size
      if (captures.length === 0) return;

      // Use landscape A4-ish sizing based on viewport aspect ratio
      const vw = window.innerWidth;
      const vh = window.innerHeight;
      const orientation = vw >= vh ? "landscape" : "portrait";
      const pdf = new jsPDF({
        orientation,
        unit: "px",
        format: [vw, vh],
        hotfixes: ["px_scaling"],
      });

      for (let i = 0; i < captures.length; i++) {
        if (i > 0) pdf.addPage([vw, vh], orientation);
        const imgData = captures[i].canvas.toDataURL("image/png");
        pdf.addImage(imgData, "PNG", 0, 0, vw, vh);
      }

      pdf.save("slides-export.pdf");
      makeToast({ kind: "success", message: `Exported ${captures.length} slide(s) to PDF` });
    } catch (err) {
      console.error("PDF export failed:", err);
      makeToast({ kind: "error", message: "PDF export failed — see console for details" });
    } finally {
      pdfExporting = false;
    }
  }

  // Selection state
  let selectedItems: SelectionItem[] = [];
  let selectionRect: SelectionRect | null = null;
  let isSelecting = false;

  // Undo/redo
  const undoHistory = new UndoHistory();

  // Clipboard (local, for copy/paste)
  let clipboard: { type: string; data: any }[] = [];

  // Alt+drag duplication tracking
  let altDragActive = false;

  function applyUndoAction(action: HistoryAction) {
    switch (action.type) {
      case "createNote":
        srocket?.send({ deleteNote: action.id });
        break;
      case "deleteNote":
        srocket?.send({ createNote: [action.data.x, action.data.y] });
        break;
      case "updateNote":
        srocket?.send({ updateNote: [action.id, action.before] });
        break;
      case "createTextBlock":
        srocket?.send({ deleteTextBlock: action.id });
        break;
      case "deleteTextBlock":
        srocket?.send({ createTextBlock: [action.data.x, action.data.y] });
        break;
      case "updateTextBlock":
        srocket?.send({ updateTextBlock: [action.id, action.before] });
        break;
      case "createDrawing":
        srocket?.send({ deleteDrawing: action.id });
        break;
      case "deleteDrawing":
        srocket?.send({ createDrawing: action.data });
        break;
      case "batch":
        for (const a of action.actions) applyUndoAction(a);
        break;
    }
  }

  function applyRedoAction(action: HistoryAction) {
    switch (action.type) {
      case "createNote":
        srocket?.send({ createNote: [action.data.x, action.data.y] });
        break;
      case "deleteNote":
        srocket?.send({ deleteNote: action.id });
        break;
      case "updateNote":
        srocket?.send({ updateNote: [action.id, action.after] });
        break;
      case "createTextBlock":
        srocket?.send({ createTextBlock: [action.data.x, action.data.y] });
        break;
      case "deleteTextBlock":
        srocket?.send({ deleteTextBlock: action.id });
        break;
      case "updateTextBlock":
        srocket?.send({ updateTextBlock: [action.id, action.after] });
        break;
      case "createDrawing":
        srocket?.send({ createDrawing: action.data });
        break;
      case "deleteDrawing":
        srocket?.send({ deleteDrawing: action.id });
        break;
      case "batch":
        for (const a of action.actions) applyRedoAction(a);
        break;
    }
  }

  /** Compute items inside a selection rectangle. */
  function computeSelection(rect: SelectionRect): SelectionItem[] {
    const nr = normalizeRect(rect);
    const items: SelectionItem[] = [];
    for (const [nid, note] of notes) {
      if (rectContainsPoint(nr, note.x, note.y, note.w || 260, note.h || 160)) {
        items.push({ type: "note", id: nid });
      }
    }
    for (const [tid, block] of textBlocks) {
      if (rectContainsPoint(nr, block.x, block.y, 200, 40)) {
        items.push({ type: "textBlock", id: tid });
      }
    }
    for (const [wid, widget] of widgets) {
      if (rectContainsPoint(nr, widget.x, widget.y, widget.w, widget.h)) {
        items.push({ type: "widget", id: wid });
      }
    }
    return items;
  }

  // Workspace intelligence state
  let workspaceRootName = "workspace"; // project folder name, e.g. "my-project"
  let sourceFiles = new Map<string, WsSourceFile>(); // path → metadata
  // Per-Claude-session activity state
  type ClaudeInstance = {
    sessionId: string;
    events: WsClaudeEvent[];
    transcriptPath: string | null;
    /** Name derived from the first user_message in this session. */
    sessionName: string | null;
    /** User-set custom name (from widget.name, propagated from widgetDiff). */
    widgetName: string | null;
    /** UNIX timestamp (seconds) of the transcript file's last modification. */
    fileMtime: number | null;
    /** True once a session_end event has been received (/exit command). */
    closed: boolean;
  };
  let claudeInstances = new Map<string, ClaudeInstance>();
  // Track sessions for which we already auto-opened a panel (prevents re-opening on reconnect).
  const autoOpenedClaudeSessions = new Set<string>();

  // PID of the running claude process, null if not found.
  let claudePid: string | null = null;
  let claudePidDead = false;

  // Latest context snapshot from the CLI (markdown), or null if none received.
  let latestContextSnapshot: string | null = null;
  // Increments on each new snapshot arrival so the child component always reacts,
  // even when the snapshot content is identical to the previous one.
  let contextSnapshotVersion = 0;

  // Per-shell input buffers for detecting the "claude" command.
  const inputBuffers: Record<number, string> = {};
  // True once "claude" has been typed as a command in any shell, or events arrive.
  let claudeDetectedLocally = false;
  $: claudeActive = claudeDetectedLocally || [...claudeInstances.values()].some((i) => i.events.length > 0);

  $: searchItems = [
    ...shells.map(([sid, ws]): SearchItem => {
      const name = shellNames.get(sid);
      return {
        type: "terminal",
        id: sid,
        label: name || `Terminal ${sid}`,
        sublabel: name ? `Terminal ${sid}` : "",
        x: ws.x,
        y: ws.y,
      };
    }),
    ...[...widgets.entries()].map(([wid, w]): SearchItem => {
      const fileMeta = w.kind.type === "fileCard" ? sourceFiles.get(w.kind.path) : null;
      return {
        type: w.kind.type === "fileTree" ? "fileTree" : "fileCard",
        id: wid,
        label: w.kind.type === "fileCard" ? w.kind.path : w.kind.type === "fileTree" ? `File Tree: ${w.kind.root}` : "Widget",
        sublabel: w.kind.type === "fileCard" ? (fileMeta?.kind ?? "File") : "Tree",
        description: fileMeta?.description || undefined,
        x: w.x,
        y: w.y,
      };
    }),
  ] satisfies SearchItem[];

  // Auto-open: when true, a FileCard is created for each file Claude reads.
  let autoOpenCards = false;
  let autoOpenOffset = 0; // cascade so cards don't stack exactly on top of each other

  /** Extract a file path from a tool_use event's content JSON. */
  function extractFilePathFromEvent(event: WsClaudeEvent): string | null {
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
  let widgets = new Map<number, WsWidget>(); // Wid → widget
  let movingWidget: number | null = null; // Wid being dragged
  let movingWidgetOrigin = [0, 0];
  let movingWidgetPos: { x: number; y: number } | null = null;

  // Client-side-only library panels (not persisted to server).
  let libraryPanels = new Map<string, { x: number; y: number; w: number; h: number }>();
  let movingLibrary: string | null = null;
  let movingLibraryOrigin = [0, 0];
  let movingLibraryPos: { x: number; y: number } | null = null;
  let resizingLibrary: { name: string; startW: number; startH: number; startX: number; startY: number } | null = null;

  let resizingWidget: { wid: number; startW: number; startH: number; startX: number; startY: number } | null = null;

  /** Last known mouse position in canvas (infinite grid) coordinates. Updated on every mousemove. */
  let lastCanvasMousePos: [number, number] | null = null;

  // Pending input for the next newly-created terminal (used by "Open in Claude Code").
  let pendingShellInput: string | null = null;
  let shellIdsBeforeCreate = new Set<number>();

  // FileCard deduplication: highlighted widget ID and its auto-clear timer.
  let highlightedWidgetId: number | null = null;
  let highlightClearTimer: ReturnType<typeof setTimeout> | null = null;

  let chatMessages: ChatMessage[] = [];
  let newMessages = false;

  let serverLatencies: number[] = [];
  let shellLatencies: number[] = [];

  // --- Video stream / screen share state ---
  /** All active video streams announced by the server (includes x/y/w/h). */
  let videoStreams = new Map<number, WsVideoStream>();
  /** Streams dismissed by the user (red-X closed locally, still alive on server). */
  // hiddenStreams is no longer used — screen shares are closed globally, not hidden.
  /** WebRTC peer connections: viewer-side (vid → RTCPeerConnection). */
  let peerConnections = new Map<number, RTCPeerConnection>();
  /** Local screen share stream (non-null when we are sharing). */
  let localStream: MediaStream | null = null;
  /** The vid assigned by the server for our screen share (null = not sharing). */
  let screenShareVid: number | null = null;
  /** Browser stream controller status: vid → uid of controller (null = no one). */
  let browserControllers = new Map<number, number | null>();
  /** ICE candidate queues while remote description is not yet set. */
  const iceCandidateQueues = new Map<number, RTCIceCandidateInit[]>();
  /** References to ScreenShareWidget instances, for routing VP8 frames. */
  let screenShareWidgetRefs: Record<number, { feedFrame: (d: Uint8Array, t: number, k: boolean) => void } | undefined> = {};
  /** Streams we have already sent watchStream for (avoid duplicate requests). */
  const watchedStreams = new Set<number>();

  function autoWatch(vid: number, stream: import("./protocol").WsVideoStream) {
    if (watchedStreams.has(vid)) return;
    const isOurShare = stream.ownerUid === userId && !stream.isBrowser;
    if (!isOurShare) {
      watchedStreams.add(vid);
      srocket?.send({ watchStream: vid });
    }
  }

  $: isSharing = screenShareVid !== null;

  onMount(async () => {
    // The page hash sets the end-to-end encryption key.
    const key = window.location.hash?.slice(1).split(",")[0] ?? "";
    const writePassword = window.location.hash?.slice(1).split(",")[1] ?? null;

    encrypt = await Encrypt.new(key);
    const encryptedZeros = await encrypt.zeros();

    const writeEncryptedZeros = writePassword
      ? await (await Encrypt.new(writePassword)).zeros()
      : null;

    srocket = new Srocket<WsServer, WsClient>(`/api/s/${id}`, {
      onMessage(message) {
        if (message.hello) {
          userId = message.hello[0];
          dispatch("receiveName", message.hello[1]);
          makeToast({
            kind: "success",
            message: `Connected to the server.`,
          });
          exitReason = null;
        } else if (message.invalidAuth) {
          exitReason =
            "The URL is not correct, invalid end-to-end encryption key.";
          srocket?.dispose();
        } else if (message.chunks) {
          let [id, seqnum, chunks] = message.chunks;
          locks[id](async () => {
            await tick();
            chunknums[id] += chunks.length;
            for (const data of chunks) {
              const buf = await encrypt.segment(
                0x100000000n | BigInt(id),
                BigInt(seqnum),
                data,
              );
              seqnum += data.length;
              writers[id](new TextDecoder().decode(buf));
            }
          });
        } else if (message.users) {
          users = message.users;
        } else if (message.userDiff) {
          const [id, update] = message.userDiff;
          users = users.filter(([uid]) => uid !== id);
          if (update !== null) {
            users = [...users, [id, update]];
          }
        } else if (message.shells) {
          shells = message.shells;
          if (movingIsDone) {
            moving = -1;
          }
          for (const [id] of message.shells) {
            if (!subscriptions.has(id)) {
              chunknums[id] ??= 0;
              locks[id] ??= createLock();
              subscriptions.add(id);
              srocket?.send({ subscribe: [id, chunknums[id]] });
            }
          }
          // Pre-fill a newly created terminal with a pending command.
          if (pendingShellInput) {
            for (const [id] of message.shells) {
              if (!shellIdsBeforeCreate.has(id)) {
                const text = pendingShellInput;
                pendingShellInput = null;
                // Small delay so the shell PTY is ready to receive input.
                setTimeout(() => {
                  handleInput(id, new TextEncoder().encode(text));
                }, 400);
                break;
              }
            }
          }
        } else if (message.hear) {
          const [uid, name, msg] = message.hear;
          chatMessages.push({ uid, name, msg, sentAt: new Date() });
          chatMessages = chatMessages;
          if (!showChat) newMessages = true;
        } else if (message.shellLatency !== undefined) {
          const shellLatency = Number(message.shellLatency);
          shellLatencies = [...shellLatencies, shellLatency].slice(-10);
        } else if (message.pong !== undefined) {
          const serverLatency = Date.now() - Number(message.pong);
          serverLatencies = [...serverLatencies, serverLatency].slice(-10);
        } else if (message.notes) {
          notes = new Map(message.notes);
        } else if (message.noteDiff) {
          const [nid, note] = message.noteDiff;
          if (note === null) {
            notes.delete(nid);
          } else {
            notes.set(nid, note);
          }
          notes = notes; // trigger reactivity
        } else if (message.textBlocks) {
          textBlocks = new Map(message.textBlocks);
        } else if (message.textBlockDiff) {
          const [tid, block] = message.textBlockDiff;
          if (block === null) {
            textBlocks.delete(tid);
          } else {
            textBlocks.set(tid, block);
            // Auto-focus newly created text block
            if (pendingAutoFocusPos && block.x === pendingAutoFocusPos[0] && block.y === pendingAutoFocusPos[1]) {
              pendingAutoFocusTid = tid;
              pendingAutoFocusPos = null;
              requestAnimationFrame(() => { pendingAutoFocusTid = null; });
            }
          }
          textBlocks = textBlocks;
        } else if (message.drawings) {
          drawings = new Map(message.drawings);
        } else if (message.drawingDiff) {
          const [did, drawing] = message.drawingDiff;
          if (drawing === null) {
            drawings.delete(did);
          } else {
            drawings.set(did, drawing);
          }
          drawings = drawings;
        } else if (message.slides) {
          slides = new Map(message.slides);
        } else if (message.slideDiff) {
          const [slid, slide] = message.slideDiff;
          if (slide === null) {
            slides.delete(slid);
          } else {
            slides.set(slid, slide);
          }
          slides = slides;
        } else if (message.sourceFiles) {
          const [root, _rootPath, files] = message.sourceFiles;
          workspaceRootName = root || "workspace";
          sourceFiles = new Map(files.map((f) => [f.path, f]));
        } else if (message.claudeEvent) {
          const ev = message.claudeEvent;
          if (ev.kind === "transcript") {
            // Synthetic marker event — store transcript path on the instance keyed
            // by the session UUID embedded in the filename (= ev.sessionId).
            const sid = ev.sessionId || ev.content; // fallback: use path as key
            let inst = claudeInstances.get(sid) ?? { sessionId: sid, events: [], transcriptPath: null, sessionName: null, widgetName: null, fileMtime: null, closed: false };
            inst.transcriptPath = ev.content;
            if (ev.fileMtime != null) inst.fileMtime = ev.fileMtime;
            claudeInstances.set(sid, inst);
            claudeInstances = claudeInstances;
            // Auto-open the Claude activity panel for this session if we haven't
            // done so already in this browser session (prevents re-opening on reconnect).
            if (hasWriteAccess && !autoOpenedClaudeSessions.has(sid)) {
              autoOpenedClaudeSessions.add(sid);
              const alreadyOpen = [...widgets.values()].some(
                (w) => w.kind.type === "claudeFeed" && (w.kind as any).instanceId === sid,
              );
              if (!alreadyOpen) {
                const [ox, oy] = getConstantOffset();
                const x = Math.round(center[0] + window.innerWidth / 2 / zoom - ox);
                const y = Math.round(center[1] + window.innerHeight / 2 / zoom - oy);
                srocket?.send({ openClaudeFeed: [x, y, sid] });
              }
            }
          } else if (ev.kind === "session_end") {
            // Claude /exit command — mark session closed and auto-close its widget.
            const sid = ev.sessionId;
            const inst = claudeInstances.get(sid);
            if (inst) {
              inst.closed = true;
              claudeInstances.set(sid, inst);
              claudeInstances = claudeInstances;
            }
            if (hasWriteAccess) {
              // Close the ClaudeFeed widget for this session, if one is open.
              for (const [wid, w] of widgets) {
                if (w.kind.type === "claudeFeed" && (w.kind as any).instanceId === sid) {
                  srocket?.send({ closeWidget: wid });
                  break;
                }
              }
            }
          } else if (ev.kind === "claude_pid") {
            // PID status update from the background tracker.
            if (ev.tool === "dead") {
              claudePidDead = true;
            } else if (ev.tool === "running") {
              claudePid = ev.content;
              claudePidDead = false;
            }
          } else {
            // Route event to its per-session instance (or "" fallback).
            const sid = ev.sessionId;
            let inst = claudeInstances.get(sid) ?? { sessionId: sid, events: [], transcriptPath: null, sessionName: null, widgetName: null, fileMtime: null, closed: false };
            // Derive session name from first user_message.
            if (inst.sessionName === null && ev.kind === "user_message" && ev.content.trim()) {
              inst.sessionName = ev.content.trim().replace(/\s+/g, " ").slice(0, 60);
            }
            inst.events = [...inst.events.slice(-199), ev];
            claudeInstances.set(sid, inst);
            claudeInstances = claudeInstances;
            // Auto-open: if enabled and this is a tool_use with a readable file path,
            // create a FileCard widget if one doesn't already exist.
            if (autoOpenCards && hasWriteAccess && ev.kind === "tool_use") {
              const filePath = extractFilePathFromEvent(ev);
              if (filePath) {
                const exists = [...widgets.values()].some(
                  (w) => w.kind.type === "fileCard" && w.kind.path === filePath,
                );
                if (!exists) {
                  const [ox, oy] = getConstantOffset();
                  const n = autoOpenOffset;
                  const x = Math.round(center[0] + window.innerWidth / 2 / zoom - ox + n * 30);
                  const y = Math.round(center[1] + window.innerHeight / 2 / zoom - oy + n * 30);
                  autoOpenOffset = (autoOpenOffset + 1) % 8;
                  srocket?.send({ openFileCard: [x, y, filePath] });
                }
              }
            }
          }
        } else if (message.widgets) {
          widgets = new Map(message.widgets);
        } else if (message.widgetDiff) {
          const [wid, widget] = message.widgetDiff;
          if (widget === null) {
            widgets.delete(wid);
          } else {
            if (widget.kind?.type === "ideEditor") {
              console.log("[sshx] IDE widget received:", JSON.stringify(widget.kind));
            }
            widgets.set(wid, widget);
            // Propagate custom widget name into claudeInstances so the dropdown shows it.
            if (widget.kind.type === "claudeFeed" && widget.kind.instanceId) {
              const iid = widget.kind.instanceId;
              const inst = claudeInstances.get(iid);
              if (inst) {
                inst.widgetName = widget.name ?? null;
                claudeInstances.set(iid, inst);
                claudeInstances = claudeInstances;
              }
            }
          }
          widgets = widgets; // trigger reactivity
        } else if (message.shellNames) {
          shellNames = new Map(message.shellNames);
        } else if (message.shellNameDiff) {
          const [sid, name] = message.shellNameDiff;
          shellNames.set(sid, name);
          shellNames = shellNames;
        } else if (message.videoStreams) {
          videoStreams = new Map(message.videoStreams);
          // Auto-watch all streams that are not ours.
          for (const [vid, stream] of message.videoStreams) autoWatch(vid, stream);
        } else if (message.videoStreamDiff) {
          const [vid, stream] = message.videoStreamDiff;
          if (stream === null) {
            videoStreams.delete(vid);
            // Stream removed — nothing to do locally.
            watchedStreams.delete(vid);
            // Clean up any peer connection for this vid.
            const pc = peerConnections.get(vid);
            if (pc) { pc.close(); peerConnections.delete(vid); }
            if (screenShareVid === vid) {
              screenShareVid = null;
              localStream?.getTracks().forEach((t) => t.stop());
              localStream = null;
            }
          } else {
            videoStreams.set(vid, stream);
            // If we just started sharing, record our vid.
            if (stream.ownerUid === userId && !stream.isBrowser && screenShareVid === null && localStream !== null) {
              screenShareVid = vid;
            }
            autoWatch(vid, stream);
          }
          videoStreams = videoStreams;
        } else if (message.rtcOffer) {
          const [vid, targetUid, sdp] = message.rtcOffer;
          if (targetUid !== userId) return; // not for us
          if (sdp.startsWith("watch:")) {
            // We are the sharer; a viewer wants to watch. Create an offer for them.
            const viewerUid = parseInt(sdp.slice(6));
            handleNewViewer(vid, viewerUid);
          } else {
            // We are a viewer receiving an SDP offer from the sharer.
            handleRtcOffer(vid, sdp);
          }
        } else if (message.rtcAnswer) {
          const [vid, targetUid, senderUid, sdp] = message.rtcAnswer;
          if (targetUid !== userId) return;
          // We are the sharer; the viewer (senderUid) sent back an answer.
          // Use compound key to find the correct PC for this specific viewer.
          const compoundKey = vid * 1000000 + senderUid;
          const pc = peerConnections.get(compoundKey) ?? peerConnections.get(vid);
          if (pc) {
            (async () => {
              await pc.setRemoteDescription({ type: "answer", sdp });
              // Flush queued ICE candidates for this specific viewer.
              const queued = iceCandidateQueues.get(compoundKey) ?? iceCandidateQueues.get(vid) ?? [];
              for (const c of queued) await pc.addIceCandidate(c);
              iceCandidateQueues.delete(compoundKey);
              iceCandidateQueues.delete(vid);
            })();
          }
        } else if (message.rtcIce) {
          const [vid, targetUid, senderUid, candidate] = message.rtcIce;
          if (targetUid !== userId) return;
          // Use compound key for sharer-side lookups (one PC per viewer).
          const compoundKey = vid * 1000000 + senderUid;
          const pc = peerConnections.get(compoundKey) ?? peerConnections.get(vid);
          if (pc) {
            const iceCandidate = { candidate: candidate.candidate, sdpMid: candidate.sdpMid, sdpMLineIndex: candidate.sdpMlineIndex };
            if (pc.remoteDescription) {
              pc.addIceCandidate(iceCandidate);
            } else {
              const queueKey = peerConnections.has(compoundKey) ? compoundKey : vid;
              const q = iceCandidateQueues.get(queueKey) ?? [];
              q.push(iceCandidate);
              iceCandidateQueues.set(queueKey, q);
            }
          }
        } else if (message.browserControlStatus) {
          const [vid, controller] = message.browserControlStatus;
          browserControllers.set(vid, controller ?? null);
          browserControllers = browserControllers;
        } else if (message.browserFrame) {
          const [vid, timestamp, data, keyframe] = message.browserFrame;
          screenShareWidgetRefs[vid]?.feedFrame(data, Number(timestamp), keyframe);
        } else if (message.iceServers) {
          iceServers = message.iceServers.map((s) => ({
            urls: s.urls,
            ...(s.username !== undefined && { username: s.username }),
            ...(s.credential !== undefined && { credential: s.credential }),
          }));
        } else if (message.ideAvailable !== undefined) {
          ideAvailable = message.ideAvailable;
        } else if (message.ideStates) {
          ideStates = new Map(message.ideStates);
          ideStates = ideStates; // trigger reactivity
        } else if (message.ideStateDiff) {
          const [wid, state] = message.ideStateDiff;
          if (state) {
            ideStates.set(wid, state);
          } else {
            ideStates.delete(wid);
          }
          ideStates = ideStates; // trigger reactivity
        } else if (message.editLock) {
          editLock = message.editLock;
        } else if (message.contextSnapshot !== undefined) {
          latestContextSnapshot = message.contextSnapshot;
          contextSnapshotVersion++;
        } else if (message.highlightComponent) {
          // Forward to any connected overlay tabs (e.g. Next.js dev server).
          try {
            const bc = new BroadcastChannel("sshx-overlay");
            bc.postMessage({ type: "HighlightComponent", name: message.highlightComponent });
            bc.close();
          } catch (_) {}
        } else if (message.error) {
          console.warn("Server error: " + message.error);
        }
      },

      onConnect() {
        srocket?.send({ authenticate: [encryptedZeros, writeEncryptedZeros] });
        if ($settings.name) {
          srocket?.send({ setName: $settings.name });
        }
        connected = true;
      },

      onDisconnect() {
        connected = false;
        subscriptions.clear();
        users = [];
        serverLatencies = [];
        shellLatencies = [];
        // Close all peer connections on disconnect.
        for (const pc of peerConnections.values()) pc.close();
        peerConnections.clear();
        peerConnections = peerConnections;
        videoStreams.clear();
        videoStreams = videoStreams;
        if (localStream) {
          localStream.getTracks().forEach((t) => t.stop());
          localStream = null;
        }
        screenShareVid = null;
      },

      onClose(event) {
        if (event.code === 4404) {
          exitReason = "Failed to connect: " + event.reason;
        } else if (event.code === 4500) {
          exitReason = "Internal server error: " + event.reason;
        }
      },
    });
  });

  onDestroy(() => srocket?.dispose());

  // Send periodic ping messages for latency estimation.
  onMount(() => {
    const pingIntervalId = window.setInterval(() => {
      if (srocket?.connected) {
        srocket.send({ ping: BigInt(Date.now()) });
      }
    }, 2000);
    return () => window.clearInterval(pingIntervalId);
  });

  function integerMedian(values: number[]) {
    if (values.length === 0) {
      return null;
    }
    const sorted = values.toSorted();
    const mid = Math.floor(sorted.length / 2);
    return sorted.length % 2 !== 0
      ? sorted[mid]
      : Math.round((sorted[mid - 1] + sorted[mid]) / 2);
  }

  $: if ($settings.name) {
    srocket?.send({ setName: $settings.name });
  }

  let counter = 0n;

  function handleOpenFileTree() {
    if (hasWriteAccess === false) {
      makeToast({ kind: "info", message: "You are in read-only mode." });
      return;
    }
    const [ox, oy] = getConstantOffset();
    const x = Math.round(center[0] + window.innerWidth / 2 / zoom - ox);
    const y = Math.round(center[1] + window.innerHeight / 2 / zoom - oy);
    srocket?.send({ openFileTree: [x, y, workspaceRootName] });
  }

  function startWidgetResize(e: PointerEvent, wid: number, widget: WsWidget) {
    resizingWidget = { wid, startW: widget.w, startH: widget.h, startX: e.clientX, startY: e.clientY };
    window.addEventListener("pointermove", onWidgetResizeMove);
    window.addEventListener("pointerup", onWidgetResizeEnd, { once: true });
  }

  function onWidgetResizeMove(e: PointerEvent) {
    if (!resizingWidget) return;
    const dw = (e.clientX - resizingWidget.startX) / zoom;
    const dh = (e.clientY - resizingWidget.startY) / zoom;
    const w = Math.max(160, resizingWidget.startW + dw);
    const h = Math.max(120, resizingWidget.startH + dh);
    const widget = widgets.get(resizingWidget.wid);
    if (widget) { widget.w = w; widget.h = h; widgets = widgets; }
  }

  function onWidgetResizeEnd(e: PointerEvent) {
    if (!resizingWidget) return;
    const dw = (e.clientX - resizingWidget.startX) / zoom;
    const dh = (e.clientY - resizingWidget.startY) / zoom;
    const w = Math.max(160, resizingWidget.startW + dw);
    const h = Math.max(120, resizingWidget.startH + dh);
    const rw = Math.round(w);
    const rh = Math.round(h);
    srocket?.send({ resizeWidget: [resizingWidget.wid, rw, rh] });
    // Persist dimensions to JSON for fileCard widgets so they reopen at the same size.
    if (hasWriteAccess) {
      const widget = widgets.get(resizingWidget.wid);
      if (widget?.kind.type === "fileCard") {
        srocket?.send({ updateFileMetadata: [widget.kind.path, { widgetW: rw, widgetH: rh }] });
      }
    }
    resizingWidget = null;
    window.removeEventListener("pointermove", onWidgetResizeMove);
  }

  function handleToggleWorkspace() {
    workspaceOpen = !workspaceOpen;
    // Open a FileTree widget the first time workspace is toggled on.
    if (workspaceOpen && ![...widgets.values()].some((w) => w.kind.type === "fileTree")) {
      handleOpenFileTree();
    }
  }

  // Derive list of active IDE editor widgets for the toolbar dropdown.
  $: ideEditors = [...widgets.entries()]
    .filter(([, w]) => w.kind.type === "ideEditor")
    .map(([wid, w]): [number, string] => [
      wid,
      w.kind.type === "ideEditor" ? w.kind.workspaceLabel : "IDE",
    ]);

  // Derive whether an AppOverlay widget exists and is visible.
  $: appOverlayOpen = [...widgets.values()].some(
    (w) => w.kind.type === "appOverlay" && !w.collapsed,
  );

  function handleToggleAppOverlay() {
    // Find existing AppOverlay widget.
    for (const [wid, w] of widgets) {
      if (w.kind.type === "appOverlay") {
        if (w.collapsed) {
          srocket?.send({ setWidgetCollapsed: [wid, false] });
        } else {
          // Already visible — scroll to it.
          const targetZoom = Math.min(zoom, INITIAL_ZOOM);
          touchZoom.moveTo([w.x, w.y], targetZoom);
        }
        return;
      }
    }
    // No widget exists — create one at viewport center.
    if (!hasWriteAccess) return;
    const [ox, oy] = getConstantOffset();
    const x = Math.round(center[0] + window.innerWidth / 2 / zoom - ox);
    const y = Math.round(center[1] + window.innerHeight / 2 / zoom - oy);
    srocket?.send({ openAppOverlay: [x, y] });
  }

  /**
   * If a FileCard for `path` already exists on the canvas, navigate to it,
   * highlight it for 2 s, and return true.  Returns false if it doesn't exist.
   */
  function focusExistingFileCard(path: string): boolean {
    for (const [wid, w] of widgets) {
      if (w.kind.type === "fileCard" && w.kind.path === path) {
        const targetZoom = Math.min(zoom, INITIAL_ZOOM);
        touchZoom.moveTo([w.x, w.y], targetZoom);
        highlightedWidgetId = wid;
        if (highlightClearTimer !== null) clearTimeout(highlightClearTimer);
        highlightClearTimer = setTimeout(() => { highlightedWidgetId = null; }, 2000);
        return true;
      }
    }
    return false;
  }

  async function handleCreate() {
    if (hasWriteAccess === false) {
      makeToast({
        kind: "info",
        message: "You are in read-only mode and cannot create new terminals.",
      });
      return;
    }
    if (shells.length >= 14) {
      makeToast({
        kind: "error",
        message: "You can only create up to 14 terminals.",
      });
      return;
    }
    const existing = shells.map(([id, winsize]) => ({
      x: winsize.x,
      y: winsize.y,
      width: termWrappers[id].clientWidth,
      height: termWrappers[id].clientHeight,
    }));
    const { x, y } = arrangeNewTerminal(existing);
    srocket?.send({ create: [x, y] });
    touchZoom.moveTo([x, y], INITIAL_ZOOM);
  }

  function handleCreateWithInput(command: string) {
    if (!hasWriteAccess) return;
    shellIdsBeforeCreate = new Set(shells.map(([id]) => id));
    pendingShellInput = command;
    const existing = shells.map(([id, winsize]) => ({
      x: winsize.x,
      y: winsize.y,
      width: termWrappers[id]?.clientWidth ?? 200,
      height: termWrappers[id]?.clientHeight ?? 150,
    }));
    const { x, y } = arrangeNewTerminal(existing);
    srocket?.send({ create: [x, y] });
    touchZoom.moveTo([x, y], INITIAL_ZOOM);
  }

  function handleCreateNote() {
    if (hasWriteAccess === false) {
      makeToast({
        kind: "info",
        message: "You are in read-only mode and cannot create sticky notes.",
      });
      return;
    }
    const existing = [
      ...shells.map(([, ws]) => ({ x: ws.x, y: ws.y, width: 200, height: 150 })),
      ...[...notes.values()].map((n) => ({ x: n.x, y: n.y, width: 250, height: 200 })),
    ];
    const { x, y } = arrangeNewTerminal(existing);
    srocket?.send({ createNote: [x, y] });
    touchZoom.moveTo([x, y], INITIAL_ZOOM);
  }

  async function handleInput(id: number, data: Uint8Array) {
    if (counter === 0n) {
      // On the first call, initialize the counter to a random 64-bit integer.
      const array = new Uint8Array(8);
      crypto.getRandomValues(array);
      counter = new DataView(array.buffer).getBigUint64(0);
    }
    const offset = counter;
    counter += BigInt(data.length); // Must increment before the `await`.
    const encrypted = await encrypt.segment(0x200000000n, offset, data);
    srocket?.send({ data: [id, encrypted, offset] });

    // Detect the "claude" command being run in any terminal.
    if (!claudeDetectedLocally) {
      const text = new TextDecoder().decode(data);
      inputBuffers[id] ??= "";
      for (const ch of text) {
        if (ch === "\r" || ch === "\n") {
          const line = inputBuffers[id].trim();
          if (line === "claude" || line.startsWith("claude ")) {
            claudeDetectedLocally = true;
          }
          inputBuffers[id] = "";
        } else if (ch === "\x7f" || ch === "\b") {
          inputBuffers[id] = inputBuffers[id].slice(0, -1);
        } else {
          inputBuffers[id] += ch;
        }
      }
    }
  }

  // --- Screen share / WebRTC helpers ---

  let iceServers: RTCIceServer[] = [{ urls: "stun:stun.l.google.com:19302" }];
  let ideAvailable = false;
  /** Wid of the IDE widget currently shown fullscreen (local-only, not synced). */
  let fullscreenIdeWid: number | null = null;
  /** The ideId of the fullscreen IDE widget, used to build the correct iframe URL. */
  let fullscreenIdeId: number = 0;
  /** Map from wid to IdeEditorWidget component instance, for programmatic file opens. */
  let ideEditorRefs: Record<number, { openFile: (path: string) => void }> = {};

  // --- IDE state sync ---
  import type { WsIdeState, WsEditLock } from "./protocol";
  let ideStates: Map<number, WsIdeState> = new Map();
  let editLock: WsEditLock | null = null;

  function createPeerConnection(vid: number): RTCPeerConnection {
    const pc = new RTCPeerConnection({ iceServers });
    pc.onicecandidate = ({ candidate }) => {
      if (candidate) {
        // We broadcast ICE to the owner; they relay back to all interested viewers.
        // For simplicity, target uid 0 is a placeholder — the server relays to all.
        const stream = videoStreams.get(vid);
        const ownerUid = stream?.ownerUid ?? 0;
        const iceMsg: WsIceCandidate = {
          candidate: candidate.candidate,
          sdpMid: candidate.sdpMid ?? null,
          sdpMlineIndex: candidate.sdpMLineIndex ?? null,
        };
        srocket?.send({ sendRtcIce: [vid, ownerUid, iceMsg] });
      }
    };
    pc.onconnectionstatechange = () => {
      if (pc.connectionState === "failed" || pc.connectionState === "closed") {
        peerConnections.delete(vid);
        peerConnections = peerConnections;
      }
    };
    return pc;
  }

  /** Called when we are the sharer and a new viewer wants to watch. */
  async function handleNewViewer(vid: number, viewerUid: number) {
    if (!localStream) return;
    const pc = createPeerConnection(vid);
    // Store under compound key for multi-viewer tracking, AND under plain vid
    // so that the rtcAnswer handler (which only knows targetUid = our own uid) can find it.
    peerConnections.set(vid * 1000000 + viewerUid, pc);
    peerConnections.set(vid, pc);
    // Sharer's ICE should target the viewer.
    pc.onicecandidate = ({ candidate }) => {
      if (candidate) {
        srocket?.send({ sendRtcIce: [vid, viewerUid, {
          candidate: candidate.candidate,
          sdpMid: candidate.sdpMid ?? null,
          sdpMlineIndex: candidate.sdpMLineIndex ?? null,
        }]});
      }
    };
    for (const track of localStream.getTracks()) {
      pc.addTrack(track, localStream);
    }
    const offer = await pc.createOffer();
    await pc.setLocalDescription(offer);
    srocket?.send({ sendRtcOffer: [vid, viewerUid, offer.sdp!] });
    peerConnections = peerConnections;
  }

  /** Called when we are a viewer and receive an SDP offer from the sharer. */
  async function handleRtcOffer(vid: number, sdp: string) {
    const pc = createPeerConnection(vid);
    peerConnections.set(vid, pc);
    pc.onicecandidate = ({ candidate }) => {
      if (candidate) {
        const stream = videoStreams.get(vid);
        const ownerUid = stream?.ownerUid ?? 0;
        srocket?.send({ sendRtcIce: [vid, ownerUid, {
          candidate: candidate.candidate,
          sdpMid: candidate.sdpMid ?? null,
          sdpMlineIndex: candidate.sdpMLineIndex ?? null,
        }]});
      }
    };
    await pc.setRemoteDescription({ type: "offer", sdp });
    // Flush queued ICE candidates.
    const queued = iceCandidateQueues.get(vid) ?? [];
    for (const c of queued) await pc.addIceCandidate(c);
    iceCandidateQueues.delete(vid);
    const answer = await pc.createAnswer();
    await pc.setLocalDescription(answer);
    const stream = videoStreams.get(vid);
    const ownerUid = stream?.ownerUid ?? 0;
    srocket?.send({ sendRtcAnswer: [vid, ownerUid, answer.sdp!] });
    peerConnections = peerConnections;
  }

  /** Start sharing the current user's screen. */
  async function handleStartScreenShare() {
    if (!hasWriteAccess) {
      makeToast({ kind: "info", message: "You are in read-only mode." });
      return;
    }
    try {
      localStream = await (navigator.mediaDevices as any).getDisplayMedia({
        video: { frameRate: 30 },
        audio: true,
      });
      // Notify server — it will reply with videoStreamDiff containing our vid.
      srocket?.send({ startScreenShare: true });
      // Stop sharing if user dismisses the browser prompt.
      localStream.getVideoTracks()[0]?.addEventListener("ended", () => {
        handleStopScreenShare();
      });
    } catch {
      makeToast({ kind: "error", message: "Screen share cancelled or not supported." });
    }
  }

  /** Stop sharing our screen. */
  function handleStopScreenShare() {
    localStream?.getTracks().forEach((t) => t.stop());
    localStream = null;
    if (screenShareVid !== null) {
      srocket?.send({ stopScreenShare: true });
      screenShareVid = null;
    }
  }

  // --- Video widget drag handling ---
  let movingVideo: number | null = null;
  let movingVideoOrigin = [0, 0];
  let movingVideoPos: { x: number; y: number } | null = null;

  let resizingVideo: { vid: number; startW: number; startH: number; startX: number; startY: number } | null = null;

  function startVideoResize(e: PointerEvent, vid: number, stream: WsVideoStream) {
    resizingVideo = { vid, startW: stream.w, startH: stream.h, startX: e.clientX, startY: e.clientY };
    window.addEventListener("pointermove", onVideoResizeMove);
    window.addEventListener("pointerup", onVideoResizeEnd, { once: true });
  }

  function onVideoResizeMove(e: PointerEvent) {
    if (!resizingVideo) return;
    const dw = (e.clientX - resizingVideo.startX) / zoom;
    const dh = (e.clientY - resizingVideo.startY) / zoom;
    const w = Math.max(200, resizingVideo.startW + dw);
    const h = Math.max(150, resizingVideo.startH + dh);
    const s = videoStreams.get(resizingVideo.vid);
    if (s) { s.w = Math.round(w); s.h = Math.round(h); videoStreams = videoStreams; }
  }

  function onVideoResizeEnd(e: PointerEvent) {
    if (!resizingVideo) return;
    const dw = (e.clientX - resizingVideo.startX) / zoom;
    const dh = (e.clientY - resizingVideo.startY) / zoom;
    const w = Math.max(200, Math.round(resizingVideo.startW + dw));
    const h = Math.max(150, Math.round(resizingVideo.startH + dh));
    srocket?.send({ resizeVideoStream: [resizingVideo.vid, w, h] });
    resizingVideo = null;
    window.removeEventListener("pointermove", onVideoResizeMove);
  }

  // Stupid hack to preserve input focus when terminals are reordered.
  // See: https://github.com/sveltejs/svelte/issues/3973
  let activeElement: Element | null = null;

  beforeUpdate(() => {
    activeElement = document.activeElement;
  });

  afterUpdate(() => {
    if (activeElement instanceof HTMLElement && !commandPaletteOpen) {
      activeElement.focus();
    }
  });

  // Global mouse handler logic follows, attached to the window element for smoothness.
  onMount(() => {
    // 50 milliseconds between successive terminal move updates.
    const sendMove = throttle((message: WsClient) => {
      srocket?.send(message);
    }, 50);

    // 80 milliseconds between successive cursor updates.
    const sendCursor = throttle((message: WsClient) => {
      srocket?.send(message);
    }, 80);

    function handleMouse(event: MouseEvent) {
      // Use else-if so only ONE drag operation runs per frame.
      // This prevents multiple widgets from moving simultaneously
      // if state gets out of sync (e.g. missed mouseup).
      if (movingVideo !== null && movingVideoPos) {
        const [x, y] = normalizePosition(event);
        movingVideoPos = {
          x: Math.round(x - movingVideoOrigin[0]),
          y: Math.round(y - movingVideoOrigin[1]),
        };
        sendMove({ moveVideoStream: [movingVideo, movingVideoPos.x, movingVideoPos.y] });
      } else if (movingLibrary !== null && movingLibraryPos) {
        const [x, y] = normalizePosition(event);
        movingLibraryPos = {
          x: Math.round(x - movingLibraryOrigin[0]),
          y: Math.round(y - movingLibraryOrigin[1]),
        };
      } else if (resizingLibrary !== null) {
        const dw = (event.clientX - resizingLibrary.startX) / zoom;
        const dh = (event.clientY - resizingLibrary.startY) / zoom;
        const panel = libraryPanels.get(resizingLibrary.name);
        if (panel) {
          panel.w = Math.max(180, resizingLibrary.startW + dw);
          panel.h = Math.max(120, resizingLibrary.startH + dh);
          libraryPanels = libraryPanels;
        }
      } else if (movingWidget !== null && movingWidgetPos) {
        const [x, y] = normalizePosition(event);
        movingWidgetPos = {
          x: Math.round(x - movingWidgetOrigin[0]),
          y: Math.round(y - movingWidgetOrigin[1]),
        };
        sendMove({ moveWidget: [movingWidget, movingWidgetPos.x, movingWidgetPos.y] });
      } else if (movingNote !== null && movingNotePos) {
        const [x, y] = normalizePosition(event);
        movingNotePos = {
          x: Math.round(x - movingNoteOrigin[0]),
          y: Math.round(y - movingNoteOrigin[1]),
        };
        const base = notes.get(movingNote);
        if (base) sendMove({ updateNote: [movingNote, { ...base, ...movingNotePos }] });
      } else if (movingTextBlock !== null && movingTextBlockPos) {
        const [x, y] = normalizePosition(event);
        movingTextBlockPos = {
          x: Math.round(x - movingTextBlockOrigin[0]),
          y: Math.round(y - movingTextBlockOrigin[1]),
        };
        const base = textBlocks.get(movingTextBlock);
        if (base) sendMove({ updateTextBlock: [movingTextBlock, { ...base, ...movingTextBlockPos }] });
      } else if (moving !== -1 && !movingIsDone) {
        const [x, y] = normalizePosition(event);
        movingSize = {
          ...movingSize,
          x: Math.round(x - movingOrigin[0]),
          y: Math.round(y - movingOrigin[1]),
        };
        sendMove({ move: [moving, movingSize] });
      } else if (resizing !== -1) {
        const cols = Math.max(
          Math.floor((event.pageX - resizingOrigin[0]) / resizingCell[0]),
          TERM_MIN_COLS, // Minimum number of columns.
        );
        const rows = Math.max(
          Math.floor((event.pageY - resizingOrigin[1]) / resizingCell[1]),
          TERM_MIN_ROWS, // Minimum number of rows.
        );
        if (rows !== resizingSize.rows || cols !== resizingSize.cols) {
          resizingSize = { ...resizingSize, rows, cols };
          srocket?.send({ move: [resizing, resizingSize] });
        }
      }

      // Slide move
      if (movingSlide !== null && movingSlidePos) {
        const [x, y] = normalizePosition(event);
        movingSlidePos = {
          x: Math.round(x - movingSlideOrigin[0]),
          y: Math.round(y - movingSlideOrigin[1]),
        };
      }

      // Slide resize
      if (resizingSlide !== null) {
        const dw = (event.clientX - resizingSlide.startX) / zoom;
        const dh = (event.clientY - resizingSlide.startY) / zoom;
        const sl = slides.get(resizingSlide.slid);
        if (sl) {
          sl.w = Math.max(200, Math.round(resizingSlide.startW + dw));
          sl.h = Math.max(150, Math.round(resizingSlide.startH + dh));
          slides = slides;
        }
      }

      // Rubber band selection
      if (isSelecting && selectionRect) {
        const [x, y] = normalizePosition(event);
        selectionRect = { ...selectionRect, x2: x, y2: y };
        selectedItems = computeSelection(selectionRect);
      }

      lastCanvasMousePos = normalizePosition(event);
      sendCursor({ setCursor: lastCanvasMousePos });
    }

    function handleMouseEnd(event: MouseEvent) {
      // End rubber band selection
      if (isSelecting) {
        isSelecting = false;
        if (selectionRect) {
          selectedItems = computeSelection(selectionRect);
        }
        selectionRect = null;
      }

      // End slide move
      if (movingSlide !== null && movingSlidePos) {
        const sl = slides.get(movingSlide);
        if (sl) {
          srocket?.send({ updateSlide: [movingSlide, { ...sl, x: movingSlidePos.x, y: movingSlidePos.y }] });
          sl.x = movingSlidePos.x;
          sl.y = movingSlidePos.y;
          slides = slides;
        }
        movingSlide = null;
        movingSlidePos = null;
      }

      // End slide resize
      if (resizingSlide !== null) {
        const sl = slides.get(resizingSlide.slid);
        if (sl) {
          srocket?.send({ updateSlide: [resizingSlide.slid, sl] });
        }
        resizingSlide = null;
      }

      if (movingVideo !== null && movingVideoPos) {
        sendMove.cancel();
        srocket?.send({ moveVideoStream: [movingVideo, movingVideoPos.x, movingVideoPos.y] });
        // Update local stream position immediately to prevent jump
        // when pos falls back to stream (before server broadcasts update).
        const s = videoStreams.get(movingVideo);
        if (s) {
          s.x = movingVideoPos.x;
          s.y = movingVideoPos.y;
          videoStreams = videoStreams;
        }
        movingVideo = null;
        movingVideoPos = null;
      }

      if (movingLibrary !== null && movingLibraryPos) {
        const existing = libraryPanels.get(movingLibrary);
        libraryPanels.set(movingLibrary, { ...movingLibraryPos, w: existing?.w ?? 240, h: existing?.h ?? 220 });
        libraryPanels = libraryPanels;
        movingLibrary = null;
        movingLibraryPos = null;
      }

      if (resizingLibrary !== null) {
        resizingLibrary = null;
      }

      if (movingWidget !== null && movingWidgetPos) {
        sendMove.cancel();
        srocket?.send({ moveWidget: [movingWidget, movingWidgetPos.x, movingWidgetPos.y] });
        // Update local widget position immediately to prevent jump
        // when pos falls back to widget (before server broadcasts update).
        const w = widgets.get(movingWidget);
        if (w) {
          w.x = movingWidgetPos.x;
          w.y = movingWidgetPos.y;
          widgets = widgets;
        }
        movingWidget = null;
        movingWidgetPos = null;
      }

      if (movingNote !== null && movingNotePos) {
        sendMove.cancel();
        const base = notes.get(movingNote);
        if (base) {
          srocket?.send({ updateNote: [movingNote, { ...base, ...movingNotePos }] });
          // Update local note position immediately to prevent jump.
          base.x = movingNotePos.x;
          base.y = movingNotePos.y;
          notes = notes;
        }
        movingNote = null;
        movingNotePos = null;
      }

      if (movingTextBlock !== null && movingTextBlockPos) {
        sendMove.cancel();
        const base = textBlocks.get(movingTextBlock);
        if (base) {
          srocket?.send({ updateTextBlock: [movingTextBlock, { ...base, ...movingTextBlockPos }] });
          base.x = movingTextBlockPos.x;
          base.y = movingTextBlockPos.y;
          textBlocks = textBlocks;
        }
        movingTextBlock = null;
        movingTextBlockPos = null;
      }

      if (moving !== -1) {
        movingIsDone = true;
        sendMove.cancel();
        srocket?.send({ move: [moving, movingSize] });
      }

      if (resizing !== -1) {
        resizing = -1;
      }

      if (event.type === "mouseleave") {
        sendCursor.cancel();
        srocket?.send({ setCursor: null });
      }
    }

    window.addEventListener("mousemove", handleMouse);
    window.addEventListener("mouseup", handleMouseEnd);
    document.body.addEventListener("mouseleave", handleMouseEnd);
    return () => {
      window.removeEventListener("mousemove", handleMouse);
      window.removeEventListener("mouseup", handleMouseEnd);
      document.body.removeEventListener("mouseleave", handleMouseEnd);
    };
  });

  // Paste handler: paste an image from the clipboard onto the canvas.
  // If the mouse is over a FileCard, fill that card's image. Otherwise create a new ImageWidget.
  onMount(() => {
    async function handlePaste(e: ClipboardEvent) {
      if (!hasWriteAccess) return;
      const items = e.clipboardData?.items;
      if (!items) return;
      for (const item of Array.from(items)) {
        if (item.type.startsWith("image/")) {
          e.preventDefault();
          const blob = item.getAsFile();
          if (!blob) return;
          const formData = new FormData();
          formData.append("file", blob);
          try {
            const res = await fetch(`/api/s/${id}/upload`, { method: "POST", body: formData });
            if (!res.ok) return;
            const { url } = await res.json() as { url: string };
            // Check if mouse is over a FileCard (hit-test in canvas coords)
            const [mx, my] = lastCanvasMousePos ?? [0, 0];
            let hoveredFilePath: string | null = null;
            for (const [, w] of widgets) {
              if (w.kind.type !== "fileCard") continue;
              if (mx >= w.x && mx <= w.x + w.w && my >= w.y && my <= w.y + w.h) {
                hoveredFilePath = w.kind.path;
                break;
              }
            }
            if (hoveredFilePath) {
              srocket?.send({ updateFileMetadata: [hoveredFilePath, { imagePath: url }] });
            } else {
              const [cx, cy] = lastCanvasMousePos ?? [0, 0];
              const defaultName = blob.name || `paste-${Date.now()}.png`;
              srocket?.send({ createImageWidget: [cx, cy, url, defaultName, defaultName] });
            }
          } catch (err) {
            console.error("Image paste upload failed:", err);
          }
          break;
        }
      }
    }
    document.addEventListener("paste", handlePaste);
    return () => document.removeEventListener("paste", handlePaste);
  });

  let focused: number[] = [];
  $: setFocus(focused);

  // Wait a small amount of time, since blur events happen before focus events.
  const setFocus = debounce((focused: number[]) => {
    srocket?.send({ setFocus: focused[0] ?? null });
  }, 20);
</script>

<!-- Wheel handler stops native macOS Chrome zooming on pinch. -->
<main
  class="p-8"
  class:cursor-nwse-resize={resizing !== -1 || resizingWidget !== null || resizingLibrary !== null || resizingNote !== null}
  class:cursor-grabbing={resizing === -1 && resizingWidget === null && (moving !== -1 || movingWidget !== null || movingNote !== null || movingTextBlock !== null || movingLibrary !== null)}
  class:select-none={resizing !== -1 || resizingWidget !== null || resizingLibrary !== null || resizingNote !== null || resizingSlide !== null || moving !== -1 || movingWidget !== null || movingNote !== null || movingTextBlock !== null || movingLibrary !== null || movingSlide !== null}
  on:wheel={(event) => event.preventDefault()}
>
  <div
    class="absolute top-8 inset-x-0 flex justify-center pointer-events-none z-10"
  >
    <Toolbar
      {connected}
      {newMessages}
      {hasWriteAccess}
      {workspaceOpen}
      {isSharing}
      {appOverlayOpen}
      hiddenStreamCount={0}
      claudeInstances={claudeInstances}
      {claudeActive}
      on:create={handleCreate}
      {textToolActive}
      {drawingTool}
      {drawingColor}
      {majorMode}
      on:createNote={handleCreateNote}
      on:toggleTextTool={() => {
        textToolActive = !textToolActive;
        if (!textToolActive) textToolGhost = null;
      }}
      on:toggleDrawingTool={({ detail }) => {
        drawingTool = detail;
        if (drawingTool) { textToolActive = false; textToolGhost = null; }
      }}
      on:colorChange={({ detail }) => { drawingColor = detail; }}
      on:toggleWorkspace={handleToggleWorkspace}
      on:openClaudeInstance={({ detail: sid }) => {
        if (!hasWriteAccess) return;
        const [ox, oy] = getConstantOffset();
        const x = Math.round(center[0] + window.innerWidth / 2 / zoom - ox);
        const y = Math.round(center[1] + window.innerHeight / 2 / zoom - oy);
        srocket?.send({ openClaudeFeed: [x, y, sid] });
      }}
      on:resumeClaudeInTerminal={({ detail: sid }) => {
        handleCreateWithInput(`claude --resume ${sid}`);
      }}
      {ideAvailable}
      {ideEditors}
      on:toggleAppOverlay={handleToggleAppOverlay}
      on:focusIde={({ detail: wid }) => {
        const w = widgets.get(wid);
        if (w) {
          // Pan the canvas to center on this widget
          const [ox, oy] = getConstantOffset();
          center[0] = w.x + w.w / 2 - (window.innerWidth / 2 - ox) / zoom;
          center[1] = w.y + w.h / 2 - (window.innerHeight / 2 - oy) / zoom;
        }
      }}
      on:openIde={() => {
        if (!hasWriteAccess) return;
        const [ox, oy] = getConstantOffset();
        const x = Math.round(center[0] + window.innerWidth / 2 / zoom - ox);
        const y = Math.round(center[1] + window.innerHeight / 2 / zoom - oy);
        srocket?.send({ openIdeEditor: [x, y, ""] });
      }}
      on:majorModeChange={({ detail }) => {
        majorMode = detail;
        // When switching to slides, enable slideshow mode; when leaving, disable it
        slideshowMode = detail === "slides";
        // When leaving edition mode, deactivate edition tools
        if (detail !== "edition") {
          textToolActive = false;
          textToolGhost = null;
          drawingTool = null;
        }
      }}
      on:chat={() => {
        showChat = !showChat;
        newMessages = false;
      }}
      on:settings={() => {
        settingsOpen = true;
      }}
      on:networkInfo={() => {
        showNetworkInfo = !showNetworkInfo;
      }}
      on:search={() => (commandPaletteOpen = true)}
      on:startScreenShare={handleStartScreenShare}
      on:stopScreenShare={handleStopScreenShare}
      on:showStreams={() => {}}
    />

    {#if showNetworkInfo}
      <div class="absolute top-20 translate-x-[116.5px]">
        <NetworkInfo
          status={connected
            ? "connected"
            : exitReason
            ? "no-shell"
            : "no-server"}
          serverLatency={integerMedian(serverLatencies)}
          shellLatency={integerMedian(shellLatencies)}
        />
      </div>
    {/if}

  </div>

  <!-- Timeline panel (right side, visible in slideshow mode) -->
  {#if slideshowMode}
    <div class="absolute top-20 right-4 z-10 pointer-events-auto">
      <Timeline
        {slides}
        currentIndex={currentSlideIndex}
        playing={slideshowPlaying}
        exporting={pdfExporting}
        canWrite={hasWriteAccess ?? false}
        on:navigate={({ detail }) => navigateToSlide(detail)}
        on:create={handleCreateSlide}
        on:reorder={({ detail }) => srocket?.send({ reorderSlides: detail })}
        on:play={startSlideshowPlay}
        on:exportPdf={handleExportPdf}
        on:stop={stopSlideshowPlay}
        on:delete={({ detail: slid }) => srocket?.send({ deleteSlide: slid })}
      />
    </div>
  {/if}

  <!-- Bottom-right panel column: Chat -->
  {#if showChat}
    <div
      class="absolute bottom-4 right-4 z-10 w-80 flex flex-col gap-2 pointer-events-none"
      style="max-height: calc(100vh - 80px);"
    >
      <div class="flex-1 min-h-0 flex flex-col pointer-events-auto">
        <Chat
          {userId}
          messages={chatMessages}
          on:chat={(event) => srocket?.send({ chat: event.detail })}
          on:close={() => (showChat = false)}
        />
      </div>
    </div>
  {/if}

  <Settings open={settingsOpen} on:close={() => (settingsOpen = false)} />

  <ChooseName />

  <!--
    Dotted circle background appears underneath the rest of the elements, but
    moves and zooms with the fabric of the canvas.
  -->
  <div
    class="absolute inset-0 -z-10"
    style:background-image="radial-gradient(#333 {zoom}px, transparent 0)"
    style:background-size="{24 * zoom}px {24 * zoom}px"
    style:background-position="{-zoom * center[0]}px {-zoom * center[1]}px"
  />

  <div class="py-2">
    {#if exitReason !== null}
      <div class="text-red-400">{exitReason}</div>
    {:else if connected}
      <div class="flex items-center">
        <div class="text-green-400">You are connected!</div>
        {#if userId && hasWriteAccess === false}
          <div
            class="bg-yellow-900 text-yellow-200 px-1 py-0.5 rounded ml-3 inline-flex items-center gap-1"
          >
            <EyeIcon size="14" />
            <span class="text-xs">Read-only</span>
          </div>
        {/if}
      </div>
    {:else}
      <div class="text-yellow-400">Connecting…</div>
    {/if}

    <div class="mt-4">
      <NameList {users} />
    </div>
  </div>

  <div
    class="absolute inset-0 overflow-hidden touch-none"
    class:cursor-crosshair={textToolActive && !drawingTool}
    bind:this={fabricEl}
    on:contextmenu|preventDefault={(e) => {
      contextMenuX = e.clientX;
      contextMenuY = e.clientY;
      contextMenuCanvasPos = normalizePosition(e);
      contextMenuVisible = true;
    }}
    on:mousedown={(e) => {
      // Start rubber band selection on empty canvas (left click, no tool active)
      if (e.button === 0 && e.target === fabricEl && !textToolActive && !drawingTool && majorMode === "edition") {
        const [x, y] = normalizePosition(e);
        selectionRect = { x1: x, y1: y, x2: x, y2: y };
        isSelecting = true;
        if (!e.shiftKey) selectedItems = [];
      }
      // Alt+click on canvas: deselect
      if (e.button === 0 && e.target === fabricEl && !e.altKey) {
        selectedItems = [];
      }
    }}
    on:click={(e) => {
      if (textToolActive && e.target === fabricEl) {
        const [x, y] = normalizePosition(e);
        pendingAutoFocusPos = [x, y];
        srocket?.send({ createTextBlock: [x, y] });
        textToolActive = false;
        textToolGhost = null;
      }
    }}
    on:mousemove={(e) => {
      if (textToolActive) {
        textToolGhost = { x: e.clientX, y: e.clientY };
      }
    }}
  >
    <!-- Drawing layer (SVG overlay for pencil/highlighter strokes) -->
    <DrawingLayer
      {drawings}
      {center}
      {zoom}
      activeTool={drawingTool}
      color={drawingColor}
      canWrite={hasWriteAccess ?? false}
      on:create={({ detail }) => srocket?.send({ createDrawing: detail })}
    />

    {#each shells as [id, winsize] (id)}
      {@const ws = id === resizing ? resizingSize : id === moving ? movingSize : winsize}
      <div
        class="absolute"
        style:left={OFFSET_LEFT_CSS}
        style:top={OFFSET_TOP_CSS}
        style:transform-origin={OFFSET_TRANSFORM_ORIGIN_CSS}
        transition:fade|local
        use:slide={{ x: ws.x, y: ws.y, center, zoom, immediate: id === moving }}
        bind:this={termWrappers[id]}
        on:pointerdown={(e) => { if (e.button === 1) e.stopPropagation(); }}
      >
        <XTerm
          rows={ws.rows}
          cols={ws.cols}
          shellName={shellNames.get(id) ?? ""}
          bind:write={writers[id]}
          bind:termEl={termElements[id]}
          on:nameChange={({ detail: name }) => {
            srocket?.send({ setShellName: [id, name] });
          }}
          on:data={({ detail: data }) =>
            hasWriteAccess && handleInput(id, data)}
          on:close={() => srocket?.send({ close: id })}
          on:shrink={() => {
            if (!hasWriteAccess) return;
            const rows = Math.max(ws.rows - 4, TERM_MIN_ROWS);
            const cols = Math.max(ws.cols - 10, TERM_MIN_COLS);
            if (rows !== ws.rows || cols !== ws.cols) {
              srocket?.send({ move: [id, { ...ws, rows, cols }] });
            }
          }}
          on:expand={() => {
            if (!hasWriteAccess) return;
            const rows = ws.rows + 4;
            const cols = ws.cols + 10;
            srocket?.send({ move: [id, { ...ws, rows, cols }] });
          }}
          on:bringToFront={() => {
            if (!hasWriteAccess) return;
            showNetworkInfo = false;
            srocket?.send({ move: [id, null] });
          }}
          on:startMove={({ detail: event }) => {
            if (!hasWriteAccess) return;
            const [x, y] = normalizePosition(event);
            moving = id;
            movingOrigin = [x - ws.x, y - ws.y];
            movingSize = ws;
            movingIsDone = false;
          }}
          on:focus={() => {
            if (!hasWriteAccess) return;
            focused = [...focused, id];
          }}
          on:blur={() => {
            focused = focused.filter((i) => i !== id);
          }}
        />

        <!-- User avatars -->
        <div class="absolute bottom-2.5 right-2.5 pointer-events-none">
          <Avatars
            users={users.filter(
              ([uid, user]) => uid !== userId && user.focus === id,
            )}
          />
        </div>

        <!-- Interactable element for resizing -->
        <div
          class="absolute w-5 h-5 -bottom-1 -right-1 cursor-nwse-resize select-none"
          on:mousedown={(event) => {
            const canvasEl = termElements[id].querySelector(".xterm-screen");
            if (canvasEl) {
              resizing = id;
              const r = canvasEl.getBoundingClientRect();
              resizingOrigin = [event.pageX - r.width, event.pageY - r.height];
              resizingCell = [r.width / ws.cols, r.height / ws.rows];
              resizingSize = ws;
            }
          }}
          on:pointerdown={(event) => event.stopPropagation()}
        />
      </div>
    {/each}

    {#each [...notes] as [nid, note] (nid)}
      {@const pos = nid === movingNote ? movingNotePos ?? note : note}
      {@const isSelected = selectedItems.some(s => s.type === "note" && s.id === nid)}
      <div
        class="absolute"
        class:canvas-focus-ring={isSelected}
        style:left={OFFSET_LEFT_CSS}
        style:top={OFFSET_TOP_CSS}
        style:transform-origin={OFFSET_TRANSFORM_ORIGIN_CSS}
        transition:fade|local
        use:slide={{ x: pos.x, y: pos.y, center, zoom, immediate: nid === movingNote }}
      >
        <StickyNote
          {note}
          canWrite={hasWriteAccess ?? false}
          on:startMove={({ detail: event }) => {
            if (!hasWriteAccess || note.pinned) return;
            const [x, y] = normalizePosition(event);
            movingNote = nid;
            movingNoteOrigin = [x - note.x, y - note.y];
            movingNotePos = { x: note.x, y: note.y };
          }}
          on:startResize={({ detail: event }) => {
            startNoteResize(event, nid, note);
          }}
          on:update={({ detail: updatedNote }) => {
            srocket?.send({ updateNote: [nid, updatedNote] });
          }}
          on:delete={() => {
            srocket?.send({ deleteNote: nid });
          }}
        />
      </div>
    {/each}

    {#each [...textBlocks] as [tid, block] (tid)}
      {@const pos = tid === movingTextBlock ? movingTextBlockPos ?? block : block}
      {@const isSelected = selectedItems.some(s => s.type === "textBlock" && s.id === tid)}
      <div
        class="absolute"
        class:canvas-focus-ring={isSelected}
        style:left={OFFSET_LEFT_CSS}
        style:top={OFFSET_TOP_CSS}
        style:transform-origin={OFFSET_TRANSFORM_ORIGIN_CSS}
        transition:fade|local
        use:slide={{ x: pos.x, y: pos.y, center, zoom, immediate: tid === movingTextBlock }}
      >
        <TextBlock
          {block}
          canWrite={hasWriteAccess ?? false}
          autoFocus={tid === pendingAutoFocusTid}
          on:startMove={({ detail: event }) => {
            if (!hasWriteAccess) return;
            const [x, y] = normalizePosition(event);
            movingTextBlock = tid;
            movingTextBlockOrigin = [x - block.x, y - block.y];
            movingTextBlockPos = { x: block.x, y: block.y };
          }}
          on:update={({ detail: updatedBlock }) => {
            srocket?.send({ updateTextBlock: [tid, updatedBlock] });
          }}
          on:delete={() => {
            srocket?.send({ deleteTextBlock: tid });
          }}
        />
      </div>
    {/each}

    {#each [...widgets] as [wid, widget] (wid)}
      {@const pos = wid === movingWidget ? movingWidgetPos ?? widget : widget}
      {@const isSelected = selectedItems.some(s => s.type === "widget" && s.id === wid)}
      <div
        class="absolute"
        class:canvas-focus-ring={isSelected}
        style:left={OFFSET_LEFT_CSS}
        style:top={OFFSET_TOP_CSS}
        style:transform-origin={OFFSET_TRANSFORM_ORIGIN_CSS}
        transition:fade|local
        use:slide={{ x: pos.x, y: pos.y, center, zoom, immediate: wid === movingWidget }}
        on:pointerdown|stopPropagation={() => {}}
      >
        {#if widget.kind.type === "fileTree"}
          <FileTreePanel
            {widget}
            collapsed={widget.collapsed ?? false}
            files={[...sourceFiles.values()]}
            canWrite={hasWriteAccess ?? false}
            on:startMove={({ detail: event }) => {
              if (!hasWriteAccess) return;
              const [x, y] = normalizePosition(event);
              movingWidget = wid;
              movingWidgetOrigin = [x - widget.x, y - widget.y];
              movingWidgetPos = { x: widget.x, y: widget.y };
            }}
            on:delete={() => srocket?.send({ closeWidget: wid })}
            on:dragFile={({ detail: { path, event } }) => {
              if (!hasWriteAccess) return;
              if (!focusExistingFileCard(path)) {
                const [x, y] = normalizePosition(event);
                srocket?.send({ openFileCard: [Math.round(x + 30), Math.round(y), path] });
              }
            }}
            on:collapse={({ detail: newCollapsed }) => {
              console.log("[collapse] fileTree wid=", wid, "collapsed=", newCollapsed);
              widget.collapsed = newCollapsed;
              widgets = widgets;
              srocket?.send({ setWidgetCollapsed: [wid, newCollapsed] });
            }}
          />
        {:else if widget.kind.type === "fileCard"}
          <FileCard
            {widget}
            collapsed={widget.collapsed ?? false}
            highlighted={highlightedWidgetId === wid}
            sessionId={id}
            file={sourceFiles.get(widget.kind.path) ?? null}
            canWrite={hasWriteAccess ?? false}
            on:startMove={({ detail: event }) => {
              if (!hasWriteAccess) return;
              const [x, y] = normalizePosition(event);
              movingWidget = wid;
              movingWidgetOrigin = [x - widget.x, y - widget.y];
              movingWidgetPos = { x: widget.x, y: widget.y };
            }}
            on:delete={() => srocket?.send({ closeWidget: wid })}
            on:openFile={({ detail: path }) => {
              if (!hasWriteAccess) return;
              if (!focusExistingFileCard(path)) {
                srocket?.send({ openFileCard: [widget.x + 30, widget.y + 30, path] });
              }
            }}
            on:describe={({ detail: path }) => srocket?.send({ describeFiles: [path] })}
            on:updateMetadata={({ detail: { path, update } }) => {
              if (!hasWriteAccess) return;
              srocket?.send({ updateFileMetadata: [path, update] });
            }}
            on:openInClaude={({ detail: { path, question } }) => {
              handleCreateWithInput(`claude "in file @${path}, ${question}"`);
            }}
            on:collapse={({ detail: newCollapsed }) => {
              console.log("[collapse] fileCard wid=", wid, "collapsed=", newCollapsed);
              widget.collapsed = newCollapsed;
              widgets = widgets;
              srocket?.send({ setWidgetCollapsed: [wid, newCollapsed] });
            }}
            on:loadImports={() => {
              if (!hasWriteAccess || widget.kind.type !== "fileCard") return;
              const sf = sourceFiles.get(widget.kind.path);
              if (!sf) return;
              let offset = 0;
              for (const imp of sf.localImports) {
                if (![...widgets.values()].some((w) => w.kind.type === "fileCard" && w.kind.path === imp)) {
                  srocket?.send({ openFileCard: [widget.x - 320 + offset * 10, widget.y + offset * 15, imp] });
                  offset++;
                }
              }
            }}
            on:loadDependents={() => {
              if (!hasWriteAccess || widget.kind.type !== "fileCard") return;
              const sf = sourceFiles.get(widget.kind.path);
              if (!sf) return;
              let offset = 0;
              for (const dep of sf.importedBy) {
                if (![...widgets.values()].some((w) => w.kind.type === "fileCard" && w.kind.path === dep)) {
                  srocket?.send({ openFileCard: [widget.x + 320 + offset * 10, widget.y + offset * 15, dep] });
                  offset++;
                }
              }
            }}
            on:openLibrary={({ detail: libName }) => {
              if (!libraryPanels.has(libName)) {
                libraryPanels.set(libName, { x: widget.x - 270, y: widget.y, w: 240, h: 220 });
              }
              libraryPanels = libraryPanels;
            }}
            {ideAvailable}
            {ideEditors}
            on:openInVSCode={({ detail: { path, wid } }) => {
              const targetWid = wid ?? ideEditors[0]?.[0];
              if (targetWid == null) return;
              const ideWidget = widgets.get(targetWid);
              if (!ideWidget) return;
              // Pan the canvas to center on the IDE widget
              const [ox, oy] = getConstantOffset();
              center[0] = ideWidget.x + ideWidget.w / 2 - (window.innerWidth / 2 - ox) / zoom;
              center[1] = ideWidget.y + ideWidget.h / 2 - (window.innerHeight / 2 - oy) / zoom;
              // Tell the IDE widget to open the file
              ideEditorRefs[targetWid]?.openFile(path);
            }}
          />
        {:else if widget.kind.type === "claudeFeed"}
          {@const inst = claudeInstances.get(widget.kind.instanceId)}
          <ClaudeExecutionGraph
            {widget}
            collapsed={widget.collapsed ?? false}
            events={inst?.events ?? []}
            {claudeActive}
            transcriptPath={inst?.transcriptPath ?? null}
            {autoOpenCards}
            sessionId={widget.kind.instanceId}
            sessionName={inst?.sessionName ?? null}
            name={widget.name ?? null}
            {claudePid}
            {claudePidDead}
            contextSnapshot={latestContextSnapshot}
            {contextSnapshotVersion}
            on:startMove={({ detail: event }) => {
              if (!hasWriteAccess) return;
              const [x, y] = normalizePosition(event);
              movingWidget = wid;
              movingWidgetOrigin = [x - widget.x, y - widget.y];
              movingWidgetPos = { x: widget.x, y: widget.y };
            }}
            on:delete={() => srocket?.send({ closeWidget: wid })}
            on:collapse={({ detail: newCollapsed }) => {
              widget.collapsed = newCollapsed;
              widgets = widgets;
              srocket?.send({ setWidgetCollapsed: [wid, newCollapsed] });
            }}
            on:rename={({ detail: newName }) => {
              if (!hasWriteAccess) return;
              srocket?.send({ setWidgetName: [wid, newName] });
            }}
            on:highlightFile={({ detail: path }) => {
              if (!focusExistingFileCard(path) && hasWriteAccess) {
                const [ox, oy] = getConstantOffset();
                const x = Math.round(center[0] + window.innerWidth / 2 / zoom - ox);
                const y = Math.round(center[1] + window.innerHeight / 2 / zoom - oy);
                srocket?.send({ openFileCard: [x, y, path] });
              }
            }}
            on:toggleAutoOpen={() => { autoOpenCards = !autoOpenCards; }}
            on:ctrlClick={({ detail: e }) => { touchZoom.zoomAtPoint(e.clientX, e.clientY, e.shiftKey ? 1 / 1.4 : 1.4); }}
            on:requestContextSnapshot={() => { srocket?.send({ requestContextSnapshot: true }); }}
          />
        {:else if widget.kind.type === "image"}
          <ImageWidget
            {widget}
            canWrite={hasWriteAccess ?? false}
            on:startMove={({ detail: event }) => {
              if (!hasWriteAccess) return;
              const [x, y] = normalizePosition(event);
              movingWidget = wid;
              movingWidgetOrigin = [x - widget.x, y - widget.y];
              movingWidgetPos = { x: widget.x, y: widget.y };
            }}
            on:delete={() => srocket?.send({ closeWidget: wid })}
            on:rename={({ detail: newName }) => {
              if (!hasWriteAccess) return;
              srocket?.send({ setWidgetName: [wid, newName] });
              srocket?.send({ pushImageWidget: [wid, newName] });
            }}
          />
        {:else if widget.kind.type === "appOverlay"}
          <AppOverlayWidget
            {widget}
            collapsed={widget.collapsed ?? false}
            canWrite={hasWriteAccess ?? false}
            on:startMove={({ detail: event }) => {
              if (!hasWriteAccess) return;
              const [x, y] = normalizePosition(event);
              movingWidget = wid;
              movingWidgetOrigin = [x - widget.x, y - widget.y];
              movingWidgetPos = { x: widget.x, y: widget.y };
            }}
            on:delete={() => srocket?.send({ setWidgetCollapsed: [wid, true] })}
            on:collapse={({ detail: newCollapsed }) => {
              widget.collapsed = newCollapsed;
              widgets = widgets;
              srocket?.send({ setWidgetCollapsed: [wid, newCollapsed] });
            }}
            on:updateSettings={({ detail: { url, allowOpenFile, allowOpenClaude } }) => {
              srocket?.send({ updateAppOverlay: [wid, url, allowOpenFile, allowOpenClaude] });
            }}
          />
        {:else if widget.kind.type === "ideEditor"}
          <IdeEditorWidget
            bind:this={ideEditorRefs[wid]}
            {widget}
            canWrite={hasWriteAccess ?? false}
            sessionName={id}
            {ideStates}
            {editLock}
            users={new Map(users)}
            myUid={userId}
            on:startMove={({ detail: event }) => {
              if (!hasWriteAccess) return;
              const [x, y] = normalizePosition(event);
              movingWidget = wid;
              movingWidgetOrigin = [x - widget.x, y - widget.y];
              movingWidgetPos = { x: widget.x, y: widget.y };
            }}
            on:resize={({ detail }) => {
              srocket?.send({ resizeWidget: [wid, detail.w, detail.h] });
            }}
            on:maximize={({ detail: ideId }) => { fullscreenIdeWid = wid; fullscreenIdeId = ideId; }}
            on:delete={() => srocket?.send({ closeWidget: wid })}
          />
        {/if}
        {#if hasWriteAccess}
          <div
            class="absolute bottom-0 right-0 w-3 h-3 cursor-nwse-resize"
            on:pointerdown|stopPropagation={(e) => startWidgetResize(e, wid, widget)}
          />
        {/if}
      </div>
    {/each}

    {#each users.filter(([id, user]) => id !== userId && user.cursor !== null) as [id, user] (id)}
      <div
        class="absolute"
        style:left={OFFSET_LEFT_CSS}
        style:top={OFFSET_TOP_CSS}
        style:transform-origin={OFFSET_TRANSFORM_ORIGIN_CSS}
        transition:fade|local={{ duration: 200 }}
        use:slide={{
          x: user.cursor?.[0] ?? 0,
          y: user.cursor?.[1] ?? 0,
          center,
          zoom,
        }}
      >
        <LiveCursor {user} />
      </div>
    {/each}

    {#each [...videoStreams] as [vid, stream] (vid)}
      {@const pos = vid === movingVideo && movingVideoPos ? movingVideoPos : stream}
      <div
        class="absolute"
        style:left={OFFSET_LEFT_CSS}
        style:top={OFFSET_TOP_CSS}
        style:transform-origin={OFFSET_TRANSFORM_ORIGIN_CSS}
        transition:fade|local
        use:slide={{ x: pos.x, y: pos.y, center, zoom, immediate: vid === movingVideo }}
        on:pointerdown|stopPropagation={() => {}}
      >
        <ScreenShareWidget
          bind:this={screenShareWidgetRefs[vid]}
          {vid}
          {stream}
          w={stream.w}
          h={stream.h}
          {userId}
          peerConnection={peerConnections.get(vid) ?? null}
          localStream={screenShareVid === vid ? localStream : null}
          hasControl={browserControllers.get(vid) === userId}
          canWrite={hasWriteAccess ?? false}
          on:startMove={({ detail: event }) => {
            const [x, y] = normalizePosition(event);
            movingVideo = vid;
            movingVideoOrigin = [x - stream.x, y - stream.y];
            movingVideoPos = { x: stream.x, y: stream.y };
          }}
          on:close={() => {
            if (stream.ownerUid === userId && !stream.isBrowser) {
              handleStopScreenShare();
            } else {
              // Close the stream for everyone.
              const pc = peerConnections.get(vid);
              if (pc) { pc.close(); peerConnections.delete(vid); peerConnections = peerConnections; }
              srocket?.send({ closeStream: vid });
            }
          }}
            on:stopShare={handleStopScreenShare}
            on:startResize={({ detail: e }) => startVideoResize(e, vid, stream)}
            on:requestControl={() => srocket?.send({ requestBrowserControl: vid })}
            on:releaseControl={() => srocket?.send({ releaseBrowserControl: vid })}
            on:browserInput={({ detail }) => srocket?.send({ browserInput: [vid, detail] })}
          />
        </div>
    {/each}

    {#each [...libraryPanels] as [libName, panel] (libName)}
      {@const pos = libName === movingLibrary ? movingLibraryPos ?? panel : panel}
      <div
        class="absolute"
        style:left={OFFSET_LEFT_CSS}
        style:top={OFFSET_TOP_CSS}
        style:transform-origin={OFFSET_TRANSFORM_ORIGIN_CSS}
        transition:fade|local
        use:slide={{ x: pos.x, y: pos.y, center, zoom, immediate: libName === movingLibrary }}
        on:pointerdown|stopPropagation={() => {}}
      >
        <LibraryCard
          name={libName}
          w={panel.w ?? 240}
          h={panel.h ?? 220}
          importedBy={[...sourceFiles.values()].filter((f) => f.libraries.includes(libName)).map((f) => f.path)}
          on:startMove={({ detail: event }) => {
            const [x, y] = normalizePosition(event);
            movingLibrary = libName;
            movingLibraryOrigin = [x - panel.x, y - panel.y];
            movingLibraryPos = { x: panel.x, y: panel.y };
          }}
          on:startResize={({ detail: e }) => {
            resizingLibrary = { name: libName, startW: panel.w ?? 240, startH: panel.h ?? 220, startX: e.clientX, startY: e.clientY };
          }}
          on:delete={() => {
            libraryPanels.delete(libName);
            libraryPanels = libraryPanels;
          }}
          on:openFile={({ detail: path }) => {
            if (!hasWriteAccess) return;
            if (!focusExistingFileCard(path)) {
              srocket?.send({ openFileCard: [pos.x + 260, pos.y, path] });
            }
          }}
        />
      </div>
    {/each}

    <!-- Slide regions -->
    {#if slideshowMode}
      {#each [...slides] as [slid, sl] (slid)}
        {@const pos = slid === movingSlide ? movingSlidePos ?? sl : sl}
        <div
          class="absolute"
          style:left={OFFSET_LEFT_CSS}
          style:top={OFFSET_TOP_CSS}
          style:transform-origin={OFFSET_TRANSFORM_ORIGIN_CSS}
          use:slide={{ x: pos.x, y: pos.y, center, zoom, immediate: slid === movingSlide }}
        >
          <SlideRegion
            slide={sl}
            canWrite={hasWriteAccess ?? false}
            playing={slideshowPlaying}
            on:startMove={({ detail: event }) => {
              if (!hasWriteAccess) return;
              const [x, y] = normalizePosition(event);
              movingSlide = slid;
              movingSlideOrigin = [x - sl.x, y - sl.y];
              movingSlidePos = { x: sl.x, y: sl.y };
            }}
            on:startResize={({ detail: e }) => {
              resizingSlide = { slid, startW: sl.w, startH: sl.h, startX: e.clientX, startY: e.clientY };
            }}
            on:delete={() => srocket?.send({ deleteSlide: slid })}
          />
        </div>
      {/each}
    {/if}

    {#if textToolActive && textToolGhost}
      <div
        class="fixed pointer-events-none z-50 text-zinc-400 text-sm bg-zinc-800/80 px-2 py-1 rounded border border-zinc-600"
        style:left="{textToolGhost.x + 16}px"
        style:top="{textToolGhost.y + 16}px"
      >
        Add text
      </div>
    {/if}

    <!-- Rubber band selection rectangle -->
    {#if isSelecting && selectionRect}
      {@const nr = normalizeRect(selectionRect)}
      {@const ox = 0.5 * (typeof window !== 'undefined' ? window.innerWidth : 800) - 378}
      {@const oy = 0.5 * (typeof window !== 'undefined' ? window.innerHeight : 600) - 240}
      <div
        class="absolute pointer-events-none border-2 border-indigo-400 bg-indigo-400/10 rounded"
        style:left="{(nr.x1 - center[0] + ox) * zoom}px"
        style:top="{(nr.y1 - center[1] + oy) * zoom}px"
        style:width="{(nr.x2 - nr.x1) * zoom}px"
        style:height="{(nr.y2 - nr.y1) * zoom}px"
        style:z-index="50"
      />
    {/if}
  </div>

  <CommandPalette
    open={commandPaletteOpen}
    items={searchItems}
    on:close={() => (commandPaletteOpen = false)}
    on:navigate={({ detail }) => touchZoom.moveTo([detail.x, detail.y], zoom)}
  />

  <ContextMenu
    x={contextMenuX}
    y={contextMenuY}
    visible={contextMenuVisible}
    on:close={() => (contextMenuVisible = false)}
    on:select={({ detail }) => {
      const [cx, cy] = contextMenuCanvasPos;
      if (detail === "new-terminal") {
        if (!hasWriteAccess) { makeToast("error", "Write access required"); return; }
        if (shells.length >= 14) { makeToast("error", "Maximum 14 terminals"); return; }
        srocket?.send({ create: [cx, cy] });
        touchZoom.moveTo([cx, cy], INITIAL_ZOOM);
      } else if (detail === "new-text") {
        if (!hasWriteAccess) { makeToast("error", "Write access required"); return; }
        pendingAutoFocusPos = [cx, cy];
        srocket?.send({ createTextBlock: [cx, cy] });
      } else if (detail === "new-note") {
        if (!hasWriteAccess) { makeToast("error", "Write access required"); return; }
        srocket?.send({ createNote: [cx, cy] });
      } else if (detail === "open-file") {
        filePickerScreenPos = [contextMenuX, contextMenuY];
        filePickerCanvasPos = [cx, cy];
        filePickerOpen = true;
      } else if (detail === "toggle-file-tree") {
        if (!hasWriteAccess) { makeToast("error", "Write access required"); return; }
        const exists = [...widgets.values()].some(w => w.kind.type === "fileTree");
        if (!exists) {
          srocket?.send({ openFileTree: [cx, cy, workspaceRootName] });
        }
        workspaceOpen = !workspaceOpen;
      }
    }}
  />

  {#if filePickerOpen}
    <OpenFileDialog
      x={filePickerScreenPos[0]}
      y={filePickerScreenPos[1]}
      files={[...sourceFiles.keys()]}
      on:open={({ detail: path }) => {
        filePickerOpen = false;
        const [cx, cy] = filePickerCanvasPos;
        if (!focusExistingFileCard(path)) {
          srocket?.send({ openFileCard: [cx, cy, path] });
        }
      }}
      on:close={() => (filePickerOpen = false)}
    />
  {/if}

  <!-- Fullscreen IDE overlay (local-only, outside canvas transform) -->
  {#if fullscreenIdeWid !== null}
    <div class="fixed inset-0 z-[100] flex flex-col bg-zinc-900">
      <div class="flex-shrink-0 flex items-center gap-2 border-b-2 border-indigo-700 bg-black px-3 py-2 select-none">
        <CircleButtons>
          <CircleButton kind="red" on:click={() => {
            if (fullscreenIdeWid !== null) srocket?.send({ closeWidget: fullscreenIdeWid });
            fullscreenIdeWid = null;
          }} />
          <CircleButton kind="yellow" on:click={() => { fullscreenIdeWid = null; }} />
          <CircleButton kind="green" on:click={() => { fullscreenIdeWid = null; }} />
        </CircleButtons>
        <span class="flex-1 text-center text-sm text-zinc-300">VS Code — Fullscreen</span>
        <button
          class="text-xs text-zinc-400 hover:text-zinc-100 px-2 py-0.5 border border-zinc-600 rounded hover:border-zinc-400 transition-colors"
          on:click={() => { fullscreenIdeWid = null; }}
        >
          Exit
        </button>
      </div>
      <iframe
        src="/ide/s/{id}/{fullscreenIdeId}/"
        title="VS Code IDE (fullscreen)"
        class="flex-1 w-full border-none"
        sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-downloads"
      />
    </div>
  {/if}
</main>
