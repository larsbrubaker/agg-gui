// agg-gui demo — Phase 3 frontend
//
// Loads the WASM module, renders the active tab's scene, and handles
// tab switching. Handles resize so the canvas always fills the container.

type RenderFn = (width: number, height: number) => Uint8Array;

let renderers: Record<string, RenderFn> = {};
let activeTab = "basics";

// --- Canvas setup ---

const canvas = document.getElementById("canvas") as HTMLCanvasElement;
const ctx2d = canvas.getContext("2d")!;
const loadingEl = document.getElementById("loading")!;
const statusEl = document.getElementById("status")!;

// --- Render loop ---

function render() {
  const fn = renderers[activeTab] ?? renderers["basics"];
  if (!fn) return;

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
  const pixels = fn(w, h);
  const imageData = new ImageData(
    new Uint8ClampedArray(pixels.buffer, pixels.byteOffset, pixels.byteLength),
    w,
    h,
  );
  ctx2d.putImageData(imageData, 0, 0);

  const ms = (performance.now() - t0).toFixed(1);
  statusEl.textContent = `${w}×${h}  ${ms}ms`;
}

// --- Tab handling ---

document.querySelectorAll<HTMLElement>(".tab:not(.disabled)").forEach((tab) => {
  tab.addEventListener("click", () => {
    document.querySelectorAll(".tab").forEach((t) => t.classList.remove("active"));
    tab.classList.add("active");
    activeTab = tab.dataset.tab!;
    render();
  });
});

// --- Resize observer ---

const ro = new ResizeObserver(() => render());
ro.observe(canvas.parentElement!);

// --- Load WASM ---

async function init() {
  try {
    const wasm = await import("../public/pkg/demo_wasm.js");
    const wasmUrl = new URL("./public/pkg/demo_wasm_bg.wasm", location.href);
    await wasm.default({ module_or_path: wasmUrl });

    // Register each tab's render function.
    renderers["basics"] = wasm.render_basics as RenderFn;
    renderers["text"]   = wasm.render_text   as RenderFn;

    loadingEl.classList.add("hidden");
    render();
  } catch (e) {
    loadingEl.textContent = `Error loading WASM: ${e}`;
    console.error(e);
  }
}

init();
