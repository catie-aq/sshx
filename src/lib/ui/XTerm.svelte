<!-- @component Interactive terminal rendered with xterm.js -->
<script lang="ts" context="module">
  import { makeToast } from "$lib/toast";

  // Deduplicated terminal font loading.
  const waitForFonts = (() => {
    let state: "initial" | "loading" | "loaded" = "initial";
    const waitlist: (() => void)[] = [];

    return async function waitForFonts() {
      if (state === "loaded") return;
      else if (state === "initial") {
        const FontFaceObserver = (await import("fontfaceobserver")).default;
        state = "loading";
        try {
          await new FontFaceObserver("Fira Code VF").load();
        } catch (error) {
          makeToast({
            kind: "error",
            message: "Could not load terminal font.",
          });
        }
        state = "loaded";
        for (const fn of waitlist) fn();
      } else {
        await new Promise<void>((resolve) => {
          if (state === "loaded") resolve();
          else waitlist.push(resolve);
        });
      }
    };
  })();
</script>

<script lang="ts">
  import { browser } from "$app/environment";

  import { createEventDispatcher, onDestroy, onMount } from "svelte";
  import type { Terminal } from "sshx-xterm";
  import { Buffer } from "buffer";

  import themes from "./themes";
  import CircleButton from "./CircleButton.svelte";
  import CircleButtons from "./CircleButtons.svelte";
  import { settings } from "$lib/settings";
  import { TypeAheadAddon } from "$lib/typeahead";

  /** Used to determine Cmd versus Ctrl keyboard shortcuts. */
  const isMac = browser && navigator.platform.startsWith("Mac");

  const dispatch = createEventDispatcher<{
    data: Uint8Array;
    close: void;
    shrink: void;
    expand: void;
    bringToFront: void;
    startMove: MouseEvent;
    focus: void;
    blur: void;
    nameChange: string;
  }>();

  const typeahead = new TypeAheadAddon();

  export let rows: number, cols: number;
  export let write: (data: string) => void; // bound function prop
  export let shellName: string = "";

  let localName = shellName;
  $: localName = shellName;

  export let termEl: HTMLDivElement = null as any; // suppress "missing prop" warning
  let term: Terminal | null = null;

  $: theme = themes[$settings.theme];

  $: if (term) {
    // If the theme changes, update existing terminals' appearance.
    term.options.theme = theme;
    term.options.scrollback = $settings.scrollback;
  }

  let loaded = false;
  let focused = false;
  let collapsed = false;
  let currentTitle = "Remote Terminal";

  $: if (term) {
    term.options.fontSize = collapsed ? 7 : 14;
  }

  function handleWheelSkipXTerm(event: WheelEvent) {
    event.preventDefault(); // Stop native macOS Chrome zooming on pinch.

    // We stop the event from propagating to the main `.xterm` terminal element,
    // so the xterm.js's event handlers do not fire and scroll the buffer.
    event.stopPropagation();

    // However, we still want it to propagate upward to our pan/zoom handlers,
    // so we re-dispatch the event higher up, skipping xterm.
    termEl?.dispatchEvent(new WheelEvent(event.type, event));
  }

  function setFocused(isFocused: boolean, cursorLayer: HTMLDivElement) {
    if (isFocused && !focused) {
      focused = isFocused;
      cursorLayer.removeEventListener("wheel", handleWheelSkipXTerm);
      dispatch("focus");
    } else if (!isFocused && focused) {
      focused = isFocused;
      cursorLayer.addEventListener("wheel", handleWheelSkipXTerm);
      dispatch("blur");
    }
  }

  const preloadBuffer: string[] = [];

  write = (data: string) => {
    if (!term) {
      // Before the terminal is loaded, push data into a buffer.
      preloadBuffer.push(data);
    } else {
      if (data) data = typeahead.onBeforeProcessData(data);
      term.write(data);
    }
  };

  $: term?.resize(cols, rows);

  onMount(async () => {
    const [{ Terminal }, { WebLinksAddon }, { WebglAddon }, { ImageAddon }] =
      await Promise.all([
        import("sshx-xterm"),
        import("xterm-addon-web-links"),
        import("xterm-addon-webgl"),
        import("xterm-addon-image"),
      ]);

    await waitForFonts();

    term = new Terminal({
      allowTransparency: false,
      cursorBlink: false,
      cursorStyle: "block",
      // This is the monospace font family configured in Tailwind.
      fontFamily:
        '"Fira Code VF", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
      fontSize: 14,
      fontWeight: 400,
      fontWeightBold: 500,
      lineHeight: 1.06,
      scrollback: $settings.scrollback,
      theme,
    });

    // Keyboard shortcuts for natural text editing and clipboard.
    term.attachCustomKeyEventHandler((event) => {
      const mod = isMac ? event.metaKey : event.ctrlKey;
      const modOnly = mod && !(isMac ? event.ctrlKey : event.metaKey) && !event.altKey;

      if (modOnly) {
        // Cmd/Ctrl+ArrowLeft → Home (Ctrl-A)
        if (event.key === "ArrowLeft") {
          dispatch("data", new Uint8Array([0x01]));
          return false;
        }
        // Cmd/Ctrl+ArrowRight → End (Ctrl-E)
        if (event.key === "ArrowRight") {
          dispatch("data", new Uint8Array([0x05]));
          return false;
        }
        // Cmd/Ctrl+Backspace → Kill line (Ctrl-U)
        if (event.key === "Backspace") {
          dispatch("data", new Uint8Array([0x15]));
          return false;
        }
        // Cmd/Ctrl+C with selection → copy to clipboard, don't send SIGINT
        if (event.key === "c" && term!.hasSelection()) {
          return false; // let browser handle copy
        }
        // Cmd/Ctrl+V → let browser handle paste (xterm receives via paste event)
        if (event.key === "v") {
          return false;
        }
        // Cmd/Ctrl+A → select all terminal content
        if (event.key === "a") {
          event.preventDefault();
          term!.selectAll();
          return false;
        }
        // Cmd/Ctrl+K → let through for command palette
        if (event.key === "k") {
          return false;
        }
      }

      // Block all other Cmd-key combos from propagating to canvas (Mac)
      // but let xterm handle Ctrl combos normally (Ctrl+C=SIGINT, Ctrl+D=EOF, etc.)
      if (isMac && event.metaKey) {
        return false; // let browser handle unknown Cmd+key
      }

      return true;
    });

    term.loadAddon(new WebLinksAddon());
    term.loadAddon(new WebglAddon());
    term.loadAddon(new ImageAddon({ enableSizeReports: false }));

    term.open(termEl);

    term.resize(cols, rows);
    term.onTitleChange((title) => {
      currentTitle = title;
    });

    // Hack: We artificially disable scrolling when the terminal is not focused.
    // ("termEl" > div.terminal.xterm > div.xterm-screen)
    const screenEl = termEl.querySelector(".xterm-screen")! as HTMLDivElement;
    screenEl.addEventListener("wheel", handleWheelSkipXTerm);

    const focusObserver = new MutationObserver((mutations) => {
      for (const mutation of mutations) {
        if (
          mutation.type === "attributes" &&
          mutation.attributeName === "class"
        ) {
          // The "focus" class is set directly by xterm.js, but there isn't any way to listen for it.
          const target = mutation.target as HTMLElement;
          const isFocused = target.classList.contains("focus");
          setFocused(isFocused, screenEl);
        }
      }
    });
    focusObserver.observe(term.element!, { attributeFilter: ["class"] });

    loaded = true;
    for (const data of preloadBuffer) {
      term.write(data);
    }

    typeahead.reset();
    term.loadAddon(typeahead);

    const utf8 = new TextEncoder();
    term.onData((data: string) => {
      dispatch("data", utf8.encode(data));
    });
    term.onBinary((data: string) => {
      dispatch("data", Buffer.from(data, "binary"));
    });
  });

  onDestroy(() => term?.dispose());
</script>

<div
  class="term-container"
  class:focused
  style:background={theme.background}
  on:mousedown={() => dispatch("bringToFront")}
  on:pointerdown={(event) => event.stopPropagation()}
>
  <div
    class="flex select-none cursor-grab active:cursor-grabbing"
    on:mousedown={(event) => dispatch("startMove", event)}
    on:dblclick={() => (collapsed = !collapsed)}
  >
    <div class="flex-1 flex items-center px-3">
      <CircleButtons>
        <!--
          TODO: This should be on:click, but that is not working due to the
          containing element's on:pointerdown `stopPropagation()` call.
        -->
        <CircleButton
          kind="red"
          on:mousedown={(event) => event.button === 0 && dispatch("close")}
        />
        {#if !collapsed}
          <CircleButton
            kind="yellow"
            on:mousedown={(event) => event.button === 0 && dispatch("shrink")}
          />
          <CircleButton
            kind="green"
            on:mousedown={(event) => event.button === 0 && dispatch("expand")}
          />
        {/if}
      </CircleButtons>
    </div>
    <div
      class="p-2 text-sm text-zinc-300 text-center font-medium overflow-hidden whitespace-nowrap text-ellipsis w-0 flex-grow-[4]"
    >
      {currentTitle}
    </div>
    <div class="flex-1 flex items-center justify-end pr-2"><span class="text-zinc-500 text-[10px]">{collapsed ? '▴' : '▾'}</span></div>
  </div>
  <div class="flex px-3 py-0.5 border-t border-zinc-700/50">
    <input
      class="flex-1 bg-transparent outline-none truncate"
      class:text-sm={!!shellName}
      class:font-medium={!!shellName}
      class:text-zinc-300={!!shellName}
      class:text-xs={!shellName}
      class:text-zinc-600={!shellName}
      class:italic={!shellName}
      placeholder="Nommer ce terminal…"
      bind:value={localName}
      on:input={() => dispatch("nameChange", localName)}
      on:mousedown|stopPropagation
      on:pointerdown|stopPropagation
    />
  </div>
  <div
    class="inline-block transition-opacity duration-500"
    class:px-4={!collapsed} class:py-2={!collapsed}
    class:px-1={collapsed}  class:py-0={collapsed}
    bind:this={termEl}
    style:opacity={loaded ? 1.0 : 0.0}
    on:wheel={(event) => {
      if (focused) {
        // Don't pan the page when scrolling while the terminal is selected.
        // Conversely, we manually disable terminal scrolling unless it is currently selected.
        event.stopPropagation();
      }
    }}
  />
</div>

<style lang="postcss">
  .term-container {
    @apply inline-block rounded-lg border border-zinc-700 opacity-90;
    transition: transform 200ms, opacity 200ms, box-shadow 200ms, border-color 200ms;
  }

  .term-container:not(.focused) :global(.xterm) {
    @apply cursor-default;
  }

  .term-container.focused {
    @apply opacity-100 border-indigo-500/70;
    box-shadow: 0 0 0 1px rgba(99, 102, 241, 0.3), 0 0 12px rgba(99, 102, 241, 0.15);
  }
</style>
