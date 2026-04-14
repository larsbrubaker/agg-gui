// agg-gui demo — Phase 8 frontend (WebGL2)
//
// The WASM module renders the full UI via WebGL2 directly to the canvas.
// render() returns void — GL writes to the canvas; we no longer use 2D ctx.
// A requestAnimationFrame loop drives the cube animation continuously.

type RenderFn  = (width: number, height: number, frame_ms: number) => void;
type MouseXYFn = (x: number, y: number) => void;
type MouseXYBFn = (x: number, y: number, button: number) => void;
type WheelFn   = (x: number, y: number, delta_y: number) => void;
type KeyFn     = (key: string, shift: boolean, ctrl: boolean, alt: boolean) => void;
type VoidFn    = () => void;

let wasmModule: Record<string, unknown> | null = null;

// --- Canvas setup ---
// The WASM module calls getContext("webgl2") on this element internally.
// We must NOT call getContext("2d") here — a canvas can only have one context.

const canvas    = document.getElementById("canvas") as HTMLCanvasElement;
const loadingEl = document.getElementById("loading")!;

// --- Canvas size helper ---

function updateCanvasSize(): boolean {
  const wrap = canvas.parentElement!;
  const dpr  = window.devicePixelRatio || 1;
  const w    = Math.floor(wrap.clientWidth  * dpr);
  const h    = Math.floor(wrap.clientHeight * dpr);

  if (canvas.width !== w || canvas.height !== h) {
    canvas.width  = w;
    canvas.height = h;
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

// --- Animation loop (drives cube rotation) ---

function animationLoop() {
  render();
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
  if (e.key !== "Tab") e.preventDefault();
  (wasmModule["on_key_down"] as KeyFn)(e.key, e.shiftKey, e.ctrlKey, e.altKey);
});

canvas.addEventListener("contextmenu", (e) => e.preventDefault());

// --- Resize observer ---
// Canvas size changes are picked up automatically by the animation loop.
const ro = new ResizeObserver(() => updateCanvasSize());
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
