// agg-gui demo — Phase 5 frontend
//
// Loads the WASM module, renders the active tab's scene, and handles
// tab switching + event forwarding to the widget tree.

type RenderFn = (width: number, height: number) => Uint8Array;

let wasmModule: Record<string, unknown> | null = null;
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

// --- Canvas event forwarding to WASM widget tree ---

// Convert a MouseEvent's clientX/Y to canvas physical pixels (Y-down from
// canvas top-left, matching what App::on_mouse_* expects before Y-flip).
function canvasPos(e: MouseEvent): [number, number] {
  const rect = canvas.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const x = (e.clientX - rect.left) * dpr;
  const y = (e.clientY - rect.top)  * dpr;
  return [x, y];
}

function isBasicsTab(): boolean { return activeTab === "basics" && wasmModule !== null; }
function isLayoutTab(): boolean { return activeTab === "layout" && wasmModule !== null; }
function isTreeTab(): boolean   { return activeTab === "tree"   && wasmModule !== null; }

// Helper to call a WASM export by name.
function wasm<T extends (...args: unknown[]) => unknown>(name: string): T {
  return (wasmModule as Record<string, T>)[name];
}

canvas.addEventListener("mousemove", (e) => {
  if (wasmModule === null) return;
  const [x, y] = canvasPos(e);
  if (isBasicsTab())      { wasm<(x:number,y:number)=>void>("on_mouse_move")(x,y); render(); }
  else if (isLayoutTab()) { wasm<(x:number,y:number)=>void>("on_layout_mouse_move")(x,y); render(); }
  else if (isTreeTab())   { wasm<(x:number,y:number)=>void>("on_tree_mouse_move")(x,y); render(); }
});

canvas.addEventListener("mousedown", (e) => {
  if (wasmModule === null) return;
  e.preventDefault();
  canvas.focus();
  const [x, y] = canvasPos(e);
  if (isBasicsTab())      { wasm<(x:number,y:number,b:number)=>void>("on_mouse_down")(x,y,e.button); render(); }
  else if (isLayoutTab()) { wasm<(x:number,y:number,b:number)=>void>("on_layout_mouse_down")(x,y,e.button); render(); }
  else if (isTreeTab())   { wasm<(x:number,y:number,b:number)=>void>("on_tree_mouse_down")(x,y,e.button); render(); }
});

canvas.addEventListener("mouseup", (e) => {
  if (wasmModule === null) return;
  const [x, y] = canvasPos(e);
  if (isBasicsTab())      { wasm<(x:number,y:number,b:number)=>void>("on_mouse_up")(x,y,e.button); render(); }
  else if (isLayoutTab()) { wasm<(x:number,y:number,b:number)=>void>("on_layout_mouse_up")(x,y,e.button); render(); }
  else if (isTreeTab())   { wasm<(x:number,y:number,b:number)=>void>("on_tree_mouse_up")(x,y,e.button); render(); }
});

canvas.addEventListener("mouseleave", () => {
  if (wasmModule === null) return;
  if (isBasicsTab())      { wasm<()=>void>("on_mouse_leave")(); render(); }
  else if (isLayoutTab()) { wasm<()=>void>("on_layout_mouse_leave")(); render(); }
  else if (isTreeTab())   { wasm<()=>void>("on_tree_mouse_leave")(); render(); }
});

// Mouse wheel — Layout and Tree tabs.
canvas.addEventListener("wheel", (e) => {
  if (!isLayoutTab() && !isTreeTab()) return;
  e.preventDefault();
  const [x, y] = canvasPos(e);
  const dpr = window.devicePixelRatio || 1;
  const delta_y = -(e.deltaY / (e.deltaMode === 0 ? 40.0 : 1.0)) / dpr;
  if (isLayoutTab()) wasm<(x:number,y:number,d:number)=>void>("on_layout_mouse_wheel")(x,y,delta_y);
  else               wasm<(x:number,y:number,d:number)=>void>("on_tree_mouse_wheel")(x,y,delta_y);
  render();
}, { passive: false });

// Keyboard — Basics tab (text fields) and Tree tab (navigation).
canvas.addEventListener("keydown", (e) => {
  if (wasmModule === null) return;
  if (isBasicsTab()) {
    if (e.key !== "Tab") e.preventDefault();
    wasm<(k:string,s:boolean,c:boolean,a:boolean)=>void>("on_key_down")(e.key,e.shiftKey,e.ctrlKey,e.altKey);
    render();
  } else if (isTreeTab()) {
    // Prevent arrow keys from scrolling the page.
    if (["ArrowUp","ArrowDown","ArrowLeft","ArrowRight"," "].includes(e.key)) e.preventDefault();
    wasm<(k:string,s:boolean,c:boolean,a:boolean)=>void>("on_tree_key_down")(e.key,e.shiftKey,e.ctrlKey,e.altKey);
    render();
  }
});

// Prevent right-click context menu on canvas.
canvas.addEventListener("contextmenu", (e) => e.preventDefault());

// --- Resize observer ---

const ro = new ResizeObserver(() => render());
ro.observe(canvas.parentElement!);

// --- Load WASM ---

async function init() {
  try {
    const wasm = await import("../public/pkg/demo_wasm.js");
    const wasmUrl = new URL("./public/pkg/demo_wasm_bg.wasm", location.href);
    await wasm.default({ module_or_path: wasmUrl });

    wasmModule = wasm as unknown as Record<string, unknown>;
    renderers["basics"]  = wasm.render_basics as RenderFn;
    renderers["text"]    = wasm.render_text   as RenderFn;
    renderers["layout"]  = wasm.render_layout as RenderFn;
    renderers["tree"]    = wasm.render_tree   as RenderFn;

    loadingEl.classList.add("hidden");
    render();
  } catch (e) {
    loadingEl.textContent = `Error loading WASM: ${e}`;
    console.error(e);
  }
}

init();
