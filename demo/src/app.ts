// agg-gui demo — Phase 1 frontend
//
// Loads the WASM module, renders the demo into a pixel buffer, and
// displays it on an HTML canvas. Handles resize so the canvas always
// fills the container.

let renderFrame: ((width: number, height: number) => Uint8Array) | null = null;

// --- Canvas setup ---

const canvas = document.getElementById("canvas") as HTMLCanvasElement;
const ctx2d = canvas.getContext("2d")!;
const loadingEl = document.getElementById("loading")!;
const statusEl = document.getElementById("status")!;

// --- Render loop ---

function render() {
  if (!renderFrame) return;

  const wrap = canvas.parentElement!;
  const dpr = window.devicePixelRatio || 1;
  const w = Math.floor(wrap.clientWidth * dpr);
  const h = Math.floor(wrap.clientHeight * dpr);

  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }

  if (w === 0 || h === 0) return;

  const t0 = performance.now();

  // WASM renders in Y-up, lib.rs applies pixels_flipped() so the returned
  // buffer is already in top-down (Y-down) order for putImageData.
  const pixels = renderFrame(w, h);
  const imageData = new ImageData(new Uint8ClampedArray(pixels.buffer, pixels.byteOffset, pixels.byteLength), w, h);
  ctx2d.putImageData(imageData, 0, 0);

  const ms = (performance.now() - t0).toFixed(1);
  statusEl.textContent = `${w}×${h}  ${ms}ms`;
}

// --- Resize observer ---

const ro = new ResizeObserver(() => render());
ro.observe(canvas.parentElement!);

// --- Load WASM ---

async function init() {
  try {
    const wasm = await import("../public/pkg/demo_wasm.js");
    await wasm.default();
    renderFrame = wasm.render_frame as (w: number, h: number) => Uint8Array;
    loadingEl.classList.add("hidden");
    render();
  } catch (e) {
    loadingEl.textContent = `Error loading WASM: ${e}`;
    console.error(e);
  }
}

init();
