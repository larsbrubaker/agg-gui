// agg-gui demo — Phase 8 frontend (WebGL2)
//
// The WASM module renders the full UI via WebGL2 directly to the canvas.
// render() returns void — GL writes to the canvas; we no longer use 2D ctx.
// A requestAnimationFrame loop drives the cube animation continuously.

type RenderFn      = (width: number, height: number, frame_ms: number) => void;
type MouseXYFn     = (x: number, y: number) => void;
type MouseXYBFn    = (x: number, y: number, button: number) => void;
type WheelFn       = (x: number, y: number, delta_y: number) => void;
type KeyFn         = (key: string, shift: boolean, ctrl: boolean, alt: boolean, meta: boolean) => void;
type VoidFn        = () => void;
type ClipGetFn     = () => string | null;
type ClipSetFn     = (text: string) => void;

let wasmModule: Record<string, unknown> | null = null;

// --- Canvas setup ---
// The WASM module calls getContext("webgl2") on this element internally.
// We must NOT call getContext("2d") here — a canvas can only have one context.

const canvas    = document.getElementById("canvas") as HTMLCanvasElement;
const loadingEl = document.getElementById("loading")!;

// --- Canvas size helper ---

// Track the last DPR we published into WASM so we only re-set on change.
// `window.devicePixelRatio` can shift when the browser zooms, the OS scale
// changes, or the tab moves to a different-DPI monitor.
let lastDpr = 0;

function updateCanvasSize(): boolean {
  const wrap = canvas.parentElement!;
  const dpr  = window.devicePixelRatio || 1;
  const w    = Math.floor(wrap.clientWidth  * dpr);
  const h    = Math.floor(wrap.clientHeight * dpr);

  if (canvas.width !== w || canvas.height !== h) {
    canvas.width  = w;
    canvas.height = h;
  }
  if (wasmModule && dpr !== lastDpr) {
    lastDpr = dpr;
    const setDpr = wasmModule["set_device_pixel_ratio"] as ((d: number) => void) | undefined;
    if (setDpr) setDpr(dpr);
  }
  return w > 0 && h > 0;
}

// --- Render ---

// Frame time of the previous render call, displayed in the GL status overlay.
// Using the previous frame's time (like the native path) avoids the overhead
// of the overlay itself appearing in its own measurement.
let lastFrameMs = 0;

function render() {
  if (!wasmModule) return;
  if (!updateCanvasSize()) return;

  const t0 = performance.now();
  (wasmModule["render"] as RenderFn)(canvas.width, canvas.height, lastFrameMs);
  lastFrameMs = performance.now() - t0;
}

// --- Animation loop (reactive) ---
//
// The loop runs every rAF tick but only calls into the WASM render when the
// module reports that a draw is actually needed — an event arrived, the
// cube is animating, a text field is focused, or a screenshot was requested.
// Matches the native harness's Wait / WaitUntil behaviour so idle windows
// don't burn CPU / GPU for no reason.

function animationLoop() {
  if (wasmModule) {
    const needs = (wasmModule["needs_draw"] as (() => boolean) | undefined);
    if (!needs || needs()) {
      render();
    }
  }
  requestAnimationFrame(animationLoop);
}

// --- Canvas coordinate helper ---

function canvasPos(e: MouseEvent): [number, number] {
  const rect = canvas.getBoundingClientRect();
  const dpr  = window.devicePixelRatio || 1;
  return [(e.clientX - rect.left) * dpr, (e.clientY - rect.top) * dpr];
}

// --- Event forwarding ---

canvas.addEventListener("mousemove", (e) => {
  if (!wasmModule) return;
  const [x, y] = canvasPos(e);
  (wasmModule["on_mouse_move"] as MouseXYFn)(x, y);
  // render() will fire on the next rAF tick; no extra call needed.
});

canvas.addEventListener("mousedown", (e) => {
  if (!wasmModule) return;
  e.preventDefault();
  canvas.focus();
  const [x, y] = canvasPos(e);
  (wasmModule["on_mouse_down"] as MouseXYBFn)(x, y, e.button);
});

canvas.addEventListener("mouseup", (e) => {
  if (!wasmModule) return;
  const [x, y] = canvasPos(e);
  (wasmModule["on_mouse_up"] as MouseXYBFn)(x, y, e.button);
});

canvas.addEventListener("mouseleave", () => {
  if (!wasmModule) return;
  (wasmModule["on_mouse_leave"] as VoidFn)();
});

canvas.addEventListener("wheel", (e) => {
  if (!wasmModule) return;
  e.preventDefault();
  const [x, y] = canvasPos(e);
  const delta_y = e.deltaY / (e.deltaMode === 0 ? 40.0 : 1.0);
  (wasmModule["on_mouse_wheel"] as WheelFn)(x, y, delta_y);
}, { passive: false });

canvas.addEventListener("keydown", (e) => {
  if (!wasmModule) return;
  // Ctrl+V / Meta+V: don't intercept here — we handle paste via the 'paste'
  // DOM event so we get the system clipboard text synchronously.
  if ((e.ctrlKey || e.metaKey) && (e.key === "v" || e.key === "V")) return;
  if (e.key !== "Tab") e.preventDefault();
  (wasmModule["on_key_down"] as KeyFn)(e.key, e.shiftKey, e.ctrlKey, e.altKey, e.metaKey);
});

// --- Clipboard event bridge ---
// copy: WASM already wrote selected text to the in-process buffer via Ctrl+C
//       keydown; we forward it to the system clipboard here.
canvas.addEventListener("copy", (e: Event) => {
  if (!wasmModule) return;
  const ce = e as ClipboardEvent;
  const text = (wasmModule["wasm_clipboard_get"] as ClipGetFn)();
  if (text !== null && text !== undefined && text.length > 0) {
    ce.clipboardData?.setData("text/plain", text);
    ce.preventDefault();
  }
});

// cut: same as copy — WASM cut the text and stored it in the buffer.
canvas.addEventListener("cut", (e: Event) => {
  if (!wasmModule) return;
  const ce = e as ClipboardEvent;
  const text = (wasmModule["wasm_clipboard_get"] as ClipGetFn)();
  if (text !== null && text !== undefined && text.length > 0) {
    ce.clipboardData?.setData("text/plain", text);
    ce.preventDefault();
  }
});

// paste: get text from the system clipboard, store in the in-process buffer,
//        then synthesise a Ctrl+V key event so Rust's paste handler fires.
canvas.addEventListener("paste", (e: Event) => {
  if (!wasmModule) return;
  const ce = e as ClipboardEvent;
  const text = ce.clipboardData?.getData("text/plain") ?? "";
  if (text.length > 0) {
    ce.preventDefault();
    (wasmModule["wasm_clipboard_set"] as ClipSetFn)(text);
    (wasmModule["on_key_down"] as KeyFn)("v", false, true, false, false);
  }
});

canvas.addEventListener("contextmenu", (e) => e.preventDefault());

// --- Touch support ---
//
// Single-touch is forwarded as mouse button 0 (down / move / up) so
// every existing widget works on mobile with zero changes.  Two-finger
// pinch becomes `on_mouse_wheel` at the pinch midpoint, matching the
// wheel-zoom behaviour already wired for desktop.  Anything beyond two
// fingers is intentionally ignored — real multi-touch (egui's
// "Multi Touch" demo) would need a new Event::Touch variant and
// widget-level handling.
//
// `touch-action: none` on the canvas (index.html) prevents the browser
// from stealing these events for scrolling or pinch-to-zoom.

type TouchFn = (id: number, x: number, y: number, force: number) => void;
type TouchEndFn = (id: number) => void;

/// Tracks which `Touch.identifier` the mouse emulation is currently
/// following.  When that touch lifts, we release the mouse; a second
/// finger arriving (or the first being replaced) never promotes itself
/// to mouse, matching the "drag starts with one finger" contract.
let primaryTouchId: number | null = null;
let primaryTouchStart: [number, number] | null = null;
let primaryTouchLast: [number, number] | null = null;
let primaryTouchScrolling = false;
const TOUCH_SCROLL_THRESHOLD = 8;

function touchPos(t: Touch): [number, number] {
  const rect = canvas.getBoundingClientRect();
  const dpr  = window.devicePixelRatio || 1;
  return [(t.clientX - rect.left) * dpr, (t.clientY - rect.top) * dpr];
}

canvas.addEventListener("touchstart", (e) => {
  if (!wasmModule) return;
  e.preventDefault();
  canvas.focus();
  // Forward every new touch to the multi-touch aggregator; all
  // fingers, not just the first, contribute to pinch / rotate / pan.
  for (const t of Array.from(e.changedTouches)) {
    const [x, y] = touchPos(t);
    (wasmModule["on_touch_start"] as TouchFn)(t.identifier, x, y, t.force ?? 0);
  }
  // ALSO map the first finger to a mouse button-0 press so widgets
  // that only understand mouse input still respond to single-finger
  // taps.  Once a primary is established, later touches skip this.
  if (primaryTouchId === null && e.touches.length >= 1) {
    const t = e.touches[0];
    primaryTouchId = t.identifier;
    const [x, y] = touchPos(t);
    primaryTouchStart = [x, y];
    primaryTouchLast = [x, y];
    primaryTouchScrolling = false;
    (wasmModule["on_mouse_move"] as MouseXYFn)(x, y);
  }
}, { passive: false });

canvas.addEventListener("touchmove", (e) => {
  if (!wasmModule) return;
  e.preventDefault();
  for (const t of Array.from(e.changedTouches)) {
    const [x, y] = touchPos(t);
    (wasmModule["on_touch_move"] as TouchFn)(t.identifier, x, y, t.force ?? 0);
    // Primary finger also drives the mouse cursor.
    if (t.identifier === primaryTouchId) {
      if (primaryTouchStart && primaryTouchLast) {
        const dx = x - primaryTouchStart[0];
        const dy = y - primaryTouchStart[1];
        if (!primaryTouchScrolling && Math.hypot(dx, dy) >= TOUCH_SCROLL_THRESHOLD) {
          primaryTouchScrolling = true;
        }
        if (primaryTouchScrolling) {
          const stepY = y - primaryTouchLast[1];
          (wasmModule["on_mouse_wheel"] as WheelFn)(x, y, -stepY / 40.0);
        } else {
          (wasmModule["on_mouse_move"] as MouseXYFn)(x, y);
        }
        primaryTouchLast = [x, y];
      }
    }
  }
}, { passive: false });

canvas.addEventListener("touchend", (e) => {
  if (!wasmModule) return;
  e.preventDefault();
  for (const t of Array.from(e.changedTouches)) {
    (wasmModule["on_touch_end"] as TouchEndFn)(t.identifier);
    if (t.identifier === primaryTouchId) {
      const [x, y] = touchPos(t);
      if (!primaryTouchScrolling) {
        (wasmModule["on_mouse_down"] as MouseXYBFn)(x, y, 0);
        (wasmModule["on_mouse_up"] as MouseXYBFn)(x, y, 0);
      }
      (wasmModule["on_mouse_leave"] as VoidFn)();
      primaryTouchId = null;
      primaryTouchStart = null;
      primaryTouchLast = null;
      primaryTouchScrolling = false;
    }
  }
}, { passive: false });

canvas.addEventListener("touchcancel", (e) => {
  if (!wasmModule) return;
  for (const t of Array.from(e.changedTouches)) {
    (wasmModule["on_touch_cancel"] as TouchEndFn)(t.identifier);
    if (t.identifier === primaryTouchId) {
      const [x, y] = touchPos(t);
      (wasmModule["on_mouse_leave"] as VoidFn)();
      primaryTouchId = null;
      primaryTouchStart = null;
      primaryTouchLast = null;
      primaryTouchScrolling = false;
    }
  }
});

// --- Resize observer ---
// Canvas size changes are picked up automatically by the animation loop.
const ro = new ResizeObserver(() => {
  updateCanvasSize();
  // Resize changes the layout — force a render on the next rAF tick.
  if (wasmModule) {
    // on_mouse_move with an out-of-bounds position marks the frame dirty
    // via the shared NEEDS_DRAW flag without side-effects on focus.
    (wasmModule["on_mouse_move"] as MouseXYFn)(-1, -1);
  }
});
ro.observe(canvas.parentElement!);

// --- Load WASM ---

async function init() {
  try {
    const wasm    = await import("../public/pkg/demo_wasm.js");
    const wasmUrl = new URL("./public/pkg/demo_wasm_bg.wasm", location.href);
    await wasm.default({ module_or_path: wasmUrl });

    wasmModule = wasm as unknown as Record<string, unknown>;
    // Expose on window so Playwright tests can access WASM functions directly.
    (window as unknown as Record<string, unknown>).__wasm = wasmModule;
    loadingEl.classList.add("hidden");

    // Start the continuous animation loop.
    requestAnimationFrame(animationLoop);
  } catch (e) {
    loadingEl.textContent = `Error loading WASM: ${e}`;
    console.error(e);
  }
}

init();
