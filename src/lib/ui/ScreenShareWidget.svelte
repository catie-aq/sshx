<script lang="ts">
  import { createEventDispatcher, onDestroy } from "svelte";
  import CircleButton from "./CircleButton.svelte";
  import CircleButtons from "./CircleButtons.svelte";
  import type { WsVideoStream } from "../protocol";

  export let vid: number;
  export let stream: WsVideoStream;
  export let w: number = 640;
  export let h: number = 400;
  /** The local user's ID. */
  export let userId: number;
  /** The RTCPeerConnection for this stream (null if we are the sharer). */
  export let peerConnection: RTCPeerConnection | null = null;
  /** The local MediaStream (only set when we are sharing). */
  export let localStream: MediaStream | null = null;
  /** Whether the current user currently has browser control. */
  export let hasControl: boolean = false;
  /** Whether the current user can request write access. */
  export let canWrite: boolean = false;

  const dispatch = createEventDispatcher<{
    startMove: MouseEvent;
    startResize: PointerEvent;
    close: void;
    stopShare: void;
    requestControl: void;
    releaseControl: void;
    browserInput: string;
  }>();

  let videoEl: HTMLVideoElement;
  let canvasEl: HTMLCanvasElement;

  $: isSharer = stream.ownerUid === userId && !stream.isBrowser;
  $: isReceiving = peerConnection !== null || (stream.isBrowser && browserDecoder !== null);

  // Track which PC we've already called attachRemoteStream on to avoid double-setup.
  let attachedPc: RTCPeerConnection | null = null;

  // Attach local stream as soon as videoEl is ready.
  $: if (videoEl && localStream) {
    videoEl.srcObject = localStream;
  }

  // Attach remote stream when peerConnection first becomes available, or changes.
  $: if (videoEl && peerConnection && peerConnection !== attachedPc) {
    attachedPc = peerConnection;
    attachRemoteStream(peerConnection);
  }

  function attachRemoteStream(pc: RTCPeerConnection) {
    const remoteStream = new MediaStream();
    const applyStream = () => {
      if (videoEl && remoteStream.getTracks().length > 0) {
        videoEl.srcObject = remoteStream;
        videoEl.play().catch(() => {});
      }
    };
    pc.ontrack = (event) => {
      const tracks = event.streams[0]?.getTracks() ?? [event.track];
      for (const track of tracks) remoteStream.addTrack(track);
      applyStream();
    };
    // Pick up tracks that arrived before we registered ontrack (race condition
    // when setRemoteDescription fires ontrack before Svelte passes us the PC).
    for (const receiver of pc.getReceivers()) {
      if (receiver.track) remoteStream.addTrack(receiver.track);
    }
    applyStream();
  }

  // --- VP8 frame relay (browser streams only) ---
  let browserDecoder: any = null; // VideoDecoder

  function initBrowserDecoder() {
    if (typeof (window as any).VideoDecoder === "undefined") {
      console.warn("WebCodecs VideoDecoder not available; browser stream cannot be shown");
      return;
    }
    browserDecoder = new (window as any).VideoDecoder({
      output: (frame: any) => {
        if (canvasEl) {
          const ctx = canvasEl.getContext("2d");
          if (ctx) {
            canvasEl.width = frame.displayWidth;
            canvasEl.height = frame.displayHeight;
            ctx.drawImage(frame, 0, 0);
          }
        }
        frame.close();
      },
      error: (e: Error) => {
        console.error("VP8 decode error:", e);
        browserDecoder = null;
      },
    });
    browserDecoder.configure({ codec: "vp8" });
  }

  /** Called by Session.svelte when a VP8 frame arrives for this vid. */
  export function feedFrame(data: Uint8Array, timestamp: number, keyframe: boolean) {
    if (!stream.isBrowser) return;
    if (!browserDecoder || browserDecoder.state === "closed") {
      if (keyframe) initBrowserDecoder();
      else return; // wait for a keyframe before initializing
    }
    if (!browserDecoder) return;
    try {
      const chunk = new (window as any).EncodedVideoChunk({
        type: keyframe ? "key" : "delta",
        timestamp,
        data,
      });
      browserDecoder.decode(chunk);
      // Trigger reactivity so isWatching updates.
      browserDecoder = browserDecoder;
    } catch (e) {
      console.warn("VP8 chunk decode failed:", e);
      browserDecoder = null;
    }
  }

  function handleControlKey(e: KeyboardEvent) {
    if (!hasControl || !stream.isBrowser) return;
    e.preventDefault();
    dispatch("browserInput", JSON.stringify({
      type: e.type === "keydown" ? "keydown" : "keyup",
      key: e.key,
      code: e.code,
    }));
  }

  onDestroy(() => {
    if (videoEl) videoEl.srcObject = null;
    if (browserDecoder) {
      browserDecoder.close();
      browserDecoder = null;
    }
  });
</script>

<!-- Window chrome follows the project's standard panel structure -->
<div
  class="flex flex-col bg-zinc-900 border border-zinc-700 rounded-lg overflow-hidden opacity-95 hover:opacity-100"
  style="width: {w}px; height: {h}px;"
>
  <!-- Title bar / drag handle -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div
    class="flex items-center px-2 py-1.5 bg-zinc-800 border-b border-zinc-700 cursor-grab select-none shrink-0"
    on:mousedown={(e) => dispatch("startMove", e)}
  >
    <CircleButtons>
      <CircleButton kind="red" on:click={() => dispatch("close")} />
    </CircleButtons>

    <!-- Centered title -->
    <div class="flex-1 text-center text-sm text-zinc-300 truncate px-2 pointer-events-none">
      {stream.label}
    </div>

    <!-- Right slot: action buttons -->
    <div class="flex items-center gap-1.5 shrink-0">
      {#if stream.isBrowser && canWrite}
        {#if hasControl}
          <button
            class="text-xs px-2 py-0.5 bg-indigo-700 hover:bg-indigo-600 text-indigo-100 rounded transition-colors"
            on:click|stopPropagation={() => dispatch("releaseControl")}
            on:mousedown|stopPropagation
          >
            Release
          </button>
        {:else}
          <button
            class="text-xs px-2 py-0.5 bg-zinc-700 hover:bg-zinc-600 text-zinc-300 rounded transition-colors"
            on:click|stopPropagation={() => dispatch("requestControl")}
            on:mousedown|stopPropagation
          >
            Control
          </button>
        {/if}
      {/if}
      {#if isSharer}
        <button
          class="text-xs px-2 py-0.5 bg-red-800 hover:bg-red-700 text-red-100 rounded transition-colors"
          on:click|stopPropagation={() => dispatch("stopShare")}
          on:mousedown|stopPropagation
        >
          Stop
        </button>
      {/if}
    </div>
  </div>

  <!-- Video area -->
  <!-- svelte-ignore a11y-no-noninteractive-tabindex -->
  <div
    class="relative flex-1 bg-black overflow-hidden outline-none"
    tabindex={hasControl && stream.isBrowser ? 0 : -1}
    on:keydown={handleControlKey}
    on:keyup={handleControlKey}
  >
    {#if stream.isBrowser}
      <!-- Canvas-based VP8 rendering for offscreen browser streams -->
      <canvas
        bind:this={canvasEl}
        class="w-full h-full object-contain"
      />
    {:else}
      <video
        bind:this={videoEl}
        autoplay
        muted={isSharer}
        playsinline
        class="w-full h-full object-contain"
      />
    {/if}
    {#if !isReceiving && !isSharer}
      <div class="absolute inset-0 flex items-center justify-center text-zinc-500 text-sm">
        Connecting…
      </div>
    {/if}
    {#if hasControl}
      <div class="absolute top-2 right-2 bg-indigo-700/80 text-indigo-100 text-xs px-2 py-0.5 rounded">
        In control
      </div>
    {/if}
  </div>

  <!-- Resize handle (bottom-right corner) -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div
    class="absolute bottom-0 right-0 w-4 h-4 cursor-se-resize z-10"
    on:pointerdown|stopPropagation={(e) => dispatch("startResize", e)}
    title="Resize"
  >
    <svg class="w-full h-full text-zinc-600 opacity-60 hover:opacity-100" viewBox="0 0 10 10" fill="currentColor">
      <polygon points="10,0 10,10 0,10"/>
    </svg>
  </div>
</div>
