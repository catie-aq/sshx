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
  import type { WsClient, WsClaudeEvent, WsComponentGraph, WsFileMetadataUpdate, WsNote, WsServer, WsSourceFile, WsUser, WsWidget, WsWinsize } from "./protocol";
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
  import GraphView from "./ui/GraphView.svelte";
  import GraphOverlay from "./ui/GraphOverlay.svelte";
  import LibraryCard from "./ui/LibraryCard.svelte";
  import CommandPalette from "./ui/CommandPalette.svelte";
  import type { SearchItem } from "./protocol";
  import { buildRuntimeEdges } from "./runtimeGraph";
  import { slide } from "./action/slide";
  import { TouchZoom, INITIAL_ZOOM } from "./action/touchZoom";
  import { arrangeNewTerminal } from "./arrange";
  import { settings } from "./settings";
  import { EyeIcon } from "svelte-feather-icons";

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
  let graphMode = false; // @hmr:keep
  let shellNames = new Map<number, string>(); // local-only terminal labels

  onMount(() => {
    function handleGlobalKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        commandPaletteOpen = true;
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

  let mode: "terminal" | "creative" = "terminal";
  let workspaceOpen = false;
  let notes = new Map<number, WsNote>(); // Nid → WsNote
  let movingNote: number | null = null; // Nid being dragged
  let movingNoteOrigin = [0, 0]; // [dx, dy] offset from note origin
  let movingNotePos: { x: number; y: number } | null = null;

  // Workspace intelligence state
  let workspaceRootName = "workspace"; // project folder name, e.g. "my-project"
  let sourceFiles = new Map<string, WsSourceFile>(); // path → metadata
  let componentGraph: WsComponentGraph | null = null; // code + display dependency graph

  // Per-Claude-session activity state
  type ClaudeInstance = {
    sessionId: string;
    events: WsClaudeEvent[]; // rolling last 50
    transcriptPath: string | null;
    /** Name derived from the first user_message in this session. */
    sessionName: string | null;
    /** UNIX timestamp (seconds) of the transcript file's last modification. */
    fileMtime: number | null;
  };
  let claudeInstances = new Map<string, ClaudeInstance>();

  // PID of the running claude process, null if not found.
  let claudePid: string | null = null;
  let claudePidDead = false;

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

  // Pending input for the next newly-created terminal (used by "Open in Claude Code").
  let pendingShellInput: string | null = null;
  let shellIdsBeforeCreate = new Set<number>();

  let chatMessages: ChatMessage[] = [];
  let newMessages = false;

  let serverLatencies: number[] = [];
  let shellLatencies: number[] = [];

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
        } else if (message.sourceFiles) {
          const [root, files] = message.sourceFiles;
          workspaceRootName = root || "workspace";
          sourceFiles = new Map(files.map((f) => [f.path, f]));
        } else if (message.componentGraph) {
          componentGraph = message.componentGraph;
        } else if (message.claudeEvent) {
          const ev = message.claudeEvent;
          if (ev.kind === "transcript") {
            // Synthetic marker event — store transcript path on the instance keyed
            // by the session UUID embedded in the filename (= ev.sessionId).
            const sid = ev.sessionId || ev.content; // fallback: use path as key
            let inst = claudeInstances.get(sid) ?? { sessionId: sid, events: [], transcriptPath: null, sessionName: null, fileMtime: null };
            inst.transcriptPath = ev.content;
            if (ev.fileMtime != null) inst.fileMtime = ev.fileMtime;
            claudeInstances.set(sid, inst);
            claudeInstances = claudeInstances;
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
            let inst = claudeInstances.get(sid) ?? { sessionId: sid, events: [], transcriptPath: null, sessionName: null, fileMtime: null };
            // Derive session name from first user_message.
            if (inst.sessionName === null && ev.kind === "user_message" && ev.content.trim()) {
              inst.sessionName = ev.content.trim().replace(/\s+/g, " ").slice(0, 60);
            }
            inst.events = [...inst.events.slice(-49), ev];
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
          console.log("[widgetDiff] wid=", wid, "widget=", widget);
          if (widget === null) {
            widgets.delete(wid);
          } else {
            widgets.set(wid, widget);
          }
          widgets = widgets; // trigger reactivity
        } else if (message.shellNames) {
          shellNames = new Map(message.shellNames);
        } else if (message.shellNameDiff) {
          const [sid, name] = message.shellNameDiff;
          shellNames.set(sid, name);
          shellNames = shellNames;
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

  function handleOpenGraphView() {
    graphMode = !graphMode;
  }

  // Live runtime graph: links between terminals, file cards, and Claude sessions.
  $: runtimeGraph = buildRuntimeEdges(claudeInstances, widgets, shells);

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
      if (movingLibrary !== null && movingLibraryPos) {
        const [x, y] = normalizePosition(event);
        movingLibraryPos = {
          x: Math.round(x - movingLibraryOrigin[0]),
          y: Math.round(y - movingLibraryOrigin[1]),
        };
      }

      if (resizingLibrary !== null) {
        const dw = (event.clientX - resizingLibrary.startX) / zoom;
        const dh = (event.clientY - resizingLibrary.startY) / zoom;
        const panel = libraryPanels.get(resizingLibrary.name);
        if (panel) {
          panel.w = Math.max(180, resizingLibrary.startW + dw);
          panel.h = Math.max(120, resizingLibrary.startH + dh);
          libraryPanels = libraryPanels;
        }
      }

      if (movingWidget !== null && movingWidgetPos) {
        const [x, y] = normalizePosition(event);
        movingWidgetPos = {
          x: Math.round(x - movingWidgetOrigin[0]),
          y: Math.round(y - movingWidgetOrigin[1]),
        };
        sendMove({ moveWidget: [movingWidget, movingWidgetPos.x, movingWidgetPos.y] });
      }

      if (movingNote !== null && movingNotePos) {
        const [x, y] = normalizePosition(event);
        movingNotePos = {
          x: Math.round(x - movingNoteOrigin[0]),
          y: Math.round(y - movingNoteOrigin[1]),
        };
        const base = notes.get(movingNote);
        if (base) sendMove({ updateNote: [movingNote, { ...base, ...movingNotePos }] });
      }

      if (moving !== -1 && !movingIsDone) {
        const [x, y] = normalizePosition(event);
        movingSize = {
          ...movingSize,
          x: Math.round(x - movingOrigin[0]),
          y: Math.round(y - movingOrigin[1]),
        };
        sendMove({ move: [moving, movingSize] });
      }

      if (resizing !== -1) {
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

      sendCursor({ setCursor: normalizePosition(event) });
    }

    function handleMouseEnd(event: MouseEvent) {
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
        movingWidget = null;
        movingWidgetPos = null;
      }

      if (movingNote !== null && movingNotePos) {
        sendMove.cancel();
        const base = notes.get(movingNote);
        if (base) srocket?.send({ updateNote: [movingNote, { ...base, ...movingNotePos }] });
        movingNote = null;
        movingNotePos = null;
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
  class:cursor-nwse-resize={resizing !== -1 || resizingWidget !== null || resizingLibrary !== null}
  class:cursor-grabbing={resizing === -1 && resizingWidget === null && (moving !== -1 || movingWidget !== null || movingNote !== null || movingLibrary !== null)}
  on:wheel={(event) => event.preventDefault()}
>
  <div
    class="absolute top-8 inset-x-0 flex justify-center pointer-events-none z-10"
  >
    <Toolbar
      {connected}
      {newMessages}
      {hasWriteAccess}
      {mode}
      {workspaceOpen}
      {graphMode}
      claudeInstances={claudeInstances}
      {claudeActive}
      on:create={handleCreate}
      on:createNote={handleCreateNote}
      on:toggleWorkspace={handleToggleWorkspace}
      on:openClaudeInstance={({ detail: sid }) => {
        if (!hasWriteAccess) return;
        const [ox, oy] = getConstantOffset();
        const x = Math.round(center[0] + window.innerWidth / 2 / zoom - ox);
        const y = Math.round(center[1] + window.innerHeight / 2 / zoom - oy);
        srocket?.send({ openClaudeFeed: [x, y, sid] });
      }}
      on:openGraphView={handleOpenGraphView}
      on:modeChange={({ detail }) => (mode = detail)}
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

  <div class="absolute inset-0 overflow-hidden touch-none" bind:this={fabricEl}>
    {#if graphMode}
      <GraphOverlay
        {center}
        {zoom}
        {widgets}
        {sourceFiles}
        {componentGraph}
      />
    {/if}
    {#each shells as [id, winsize] (id)}
      {@const ws = id === moving ? movingSize : winsize}
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
          class="absolute w-5 h-5 -bottom-1 -right-1 cursor-nwse-resize"
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
      <div
        class="absolute"
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
          on:update={({ detail: updatedNote }) => {
            srocket?.send({ updateNote: [nid, updatedNote] });
          }}
          on:delete={() => {
            srocket?.send({ deleteNote: nid });
          }}
        />
      </div>
    {/each}

    {#each [...widgets] as [wid, widget] (wid)}
      {@const pos = wid === movingWidget ? movingWidgetPos ?? widget : widget}
      <div
        class="absolute"
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
              const [x, y] = normalizePosition(event);
              srocket?.send({ openFileCard: [Math.round(x + 30), Math.round(y), path] });
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
            {graphMode}
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
              srocket?.send({ openFileCard: [widget.x + 30, widget.y + 30, path] });
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
          />
        {:else if widget.kind.type === "graphView"}
          <GraphView
            {widget}
            {componentGraph}
            {runtimeGraph}
            on:startMove={({ detail: event }) => {
              if (!hasWriteAccess) return;
              const [x, y] = normalizePosition(event);
              movingWidget = wid;
              movingWidgetOrigin = [x - widget.x, y - widget.y];
              movingWidgetPos = { x: widget.x, y: widget.y };
            }}
            on:delete={() => srocket?.send({ closeWidget: wid })}
            on:navigateTo={({ detail: path }) => {
              const w = [...widgets.values()].find(
                (w) => w.kind.type === "fileCard" && w.kind.path === path,
              );
              if (w) {
                touchZoom.moveTo([w.x, w.y], zoom);
              } else if (hasWriteAccess) {
                const [ox, oy] = getConstantOffset();
                const x = Math.round(center[0] + window.innerWidth / 2 / zoom - ox);
                const y = Math.round(center[1] + window.innerHeight / 2 / zoom - oy);
                srocket?.send({ openFileCard: [x, y, path] });
              }
            }}
          />
        {:else if widget.kind.type === "claudeFeed"}
          {@const inst = claudeInstances.get(widget.kind.instanceId)}
          <ClaudeActivityFeed
            {widget}
            collapsed={widget.collapsed ?? false}
            events={inst?.events ?? []}
            {claudeActive}
            transcriptPath={inst?.transcriptPath ?? null}
            {autoOpenCards}
            sessionId={widget.kind.instanceId}
            sessionName={inst?.sessionName ?? null}
            {claudePid}
            {claudePidDead}
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
            on:highlightFile={({ detail: path }) => {
              const w = [...widgets.values()].find(
                (w) => w.kind.type === "fileCard" && w.kind.path === path,
              );
              if (w) {
                touchZoom.moveTo([w.x, w.y], zoom);
              } else if (hasWriteAccess) {
                const [ox, oy] = getConstantOffset();
                const x = Math.round(center[0] + window.innerWidth / 2 / zoom - ox);
                const y = Math.round(center[1] + window.innerHeight / 2 / zoom - oy);
                srocket?.send({ openFileCard: [x, y, path] });
              }
            }}
            on:toggleAutoOpen={() => { autoOpenCards = !autoOpenCards; }}
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
            srocket?.send({ openFileCard: [pos.x + 260, pos.y, path] });
          }}
        />
      </div>
    {/each}
  </div>

  <CommandPalette
    open={commandPaletteOpen}
    items={searchItems}
    on:close={() => (commandPaletteOpen = false)}
    on:navigate={({ detail }) => touchZoom.moveTo([detail.x, detail.y], zoom)}
  />
</main>
