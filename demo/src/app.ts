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
type BoolFn        = () => boolean;
type StringFn      = () => string;
type OptionalStringFn = () => string | null;
type InstallFontFn = (name: string, primary: Uint8Array, icons: Uint8Array, emoji: Uint8Array) => boolean;
type SetPlatformFn = (platform: string, pointerCoarse: boolean) => void;

let wasmModule: Record<string, unknown> | null = null;
const fontFetchCache = new Map<string, Promise<Uint8Array>>();
let fontDrainRunning = false;

// --- Canvas setup ---
// The WASM module calls getContext("webgl2") on this element internally.
// We must NOT call getContext("2d") here — a canvas can only have one context.

const canvas    = document.getElementById("canvas") as HTMLCanvasElement;
const loadingEl = document.getElementById("loading")!;
const loadingTextEl = document.getElementById("loading-text")!;
const mobileTextInput = document.createElement("textarea");
mobileTextInput.setAttribute("aria-hidden", "true");
mobileTextInput.setAttribute("autocomplete", "off");
mobileTextInput.setAttribute("autocorrect", "off");
mobileTextInput.setAttribute("autocapitalize", "off");
mobileTextInput.setAttribute("spellcheck", "false");
mobileTextInput.inputMode = "text";
Object.assign(mobileTextInput.style, {
  position: "fixed",
  left: "0",
  top: "0",
  width: "1px",
  height: "1px",
  opacity: "0",
  pointerEvents: "none",
  zIndex: "-1",
});
document.body.appendChild(mobileTextInput);

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

function detectClientPlatform(): string {
  // Pass the full UA string when available — both the OS-family detector
  // (Cmd vs. Ctrl) and the input-profile detector (mobile-touch
  // keyboard) live in agg-gui and parse the same string.
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  return (
    nav.userAgentData?.platform ||
    navigator.userAgent ||
    navigator.platform ||
    "other"
  );
}

function detectPointerCoarse(): boolean {
  // `(pointer: coarse)` is true on touch-primary devices (phones,
  // tablets), false on mouse / trackpad. iPad-mode Safari hides "iPad"
  // from the UA, so this is the only reliable signal there.
  return typeof window.matchMedia === "function"
    ? window.matchMedia("(pointer: coarse)").matches
    : false;
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

// When this page is opened as a screen-share *sender* (?host=<id>), the capture
// hook is installed here. It must run inside the rAF tick, right after render(),
// so the WebGL drawing buffer is still intact when we snapshot it.
let shareCapture: ((now: number) => void) | null = null;

function animationLoop(now: number) {
  if (wasmModule) {
    void drainPendingFontRequests();
    if (shareCapture) {
      // Sender mode: keep the canvas fresh every frame so the streamed view
      // stays live even when the app would otherwise idle, then capture.
      render();
      shareCapture(now);
    } else {
      const needs = (wasmModule["needs_draw"] as (() => boolean) | undefined);
      if (!needs || needs()) {
        render();
      }
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
  syncMobileKeyboard();
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
  // Sign convention: app expects positive delta_y = wheel rotated forward =
  // user wants content ABOVE (matches winit's MouseScrollDelta). Browser
  // WheelEvent.deltaY is the opposite: positive = scroll DOWN. Negate to
  // convert. Do NOT add a sign flip elsewhere "to make scrolling feel right" —
  // OS-level "reverse / natural scroll" preferences (Windows FlipFlopWheel,
  // macOS Natural Scrolling) are applied at the driver level before either
  // browser or winit sees the event, so a single negation here mirrors the
  // OS preference on both old-school and natural-scroll setups.
  const delta_y = -e.deltaY / (e.deltaMode === 0 ? 40.0 : 1.0);
  (wasmModule["on_mouse_wheel"] as WheelFn)(x, y, delta_y);
}, { passive: false });

function forwardKeyDown(e: KeyboardEvent, fromMobileTextInput = false) {
  if (!wasmModule) return;
  if (
    fromMobileTextInput &&
    e.key.length === 1 &&
    !e.ctrlKey &&
    !e.metaKey &&
    !e.altKey
  ) {
    // Virtual keyboards deliver printable text through beforeinput/input.
    // Let that path own insertion so we don't double-insert on browsers that
    // also fire keydown for printable keys.
    e.preventDefault();
    return;
  }
  // Ctrl+V / Meta+V: don't intercept here — we handle paste via the 'paste'
  // DOM event so we get the system clipboard text synchronously.
  if ((e.ctrlKey || e.metaKey) && (e.key === "v" || e.key === "V")) return;
  if (e.key !== "Tab") e.preventDefault();
  (wasmModule["on_key_down"] as KeyFn)(e.key, e.shiftKey, e.ctrlKey, e.altKey, e.metaKey);
}

canvas.addEventListener("keydown", (e) => forwardKeyDown(e));
mobileTextInput.addEventListener("keydown", (e) => forwardKeyDown(e, true));

function sendTextInput(text: string) {
  if (!wasmModule || text.length === 0) return;
  const keyDown = wasmModule["on_key_down"] as KeyFn;
  for (const ch of Array.from(text)) {
    keyDown(ch, false, false, false, false);
  }
}

mobileTextInput.addEventListener("beforeinput", (e: InputEvent) => {
  if (!wasmModule) return;
  if (e.inputType === "insertText" || e.inputType === "insertCompositionText") {
    e.preventDefault();
    sendTextInput(e.data ?? "");
    mobileTextInput.value = "";
  } else if (e.inputType === "deleteContentBackward") {
    e.preventDefault();
    (wasmModule["on_key_down"] as KeyFn)("Backspace", false, false, false, false);
    mobileTextInput.value = "";
  } else if (e.inputType === "deleteContentForward") {
    e.preventDefault();
    (wasmModule["on_key_down"] as KeyFn)("Delete", false, false, false, false);
    mobileTextInput.value = "";
  } else if (e.inputType === "insertFromPaste") {
    // The paste event below supplies the actual clipboard text.
    e.preventDefault();
  }
});

mobileTextInput.addEventListener("input", () => {
  // Fallback for browsers that skip beforeinput for simple text entry.
  sendTextInput(mobileTextInput.value);
  mobileTextInput.value = "";
});

function syncMobileKeyboard() {
  if (!wasmModule) return;
  // If the agg-gui on-screen keyboard is taking over, get out of the
  // way: blur the hidden textarea so the native iOS / Android keyboard
  // does not also pop up.
  const aggKeyboardActive =
    (wasmModule["software_keyboard_visible"] as BoolFn | undefined)?.() ?? false;
  if (aggKeyboardActive) {
    if (document.activeElement === mobileTextInput) {
      mobileTextInput.blur();
      canvas.focus();
    }
    return;
  }
  const textFocused = (wasmModule["text_input_focused"] as BoolFn | undefined)?.() ?? false;
  if (textFocused) {
    mobileTextInput.value = "";
    mobileTextInput.focus({ preventScroll: true });
  } else if (document.activeElement === mobileTextInput) {
    mobileTextInput.blur();
    canvas.focus();
  }
}

// --- Clipboard event bridge ---
// copy: WASM already wrote selected text to the in-process buffer via Ctrl+C
//       keydown; we forward it to the system clipboard here.
function handleCopy(e: Event) {
  if (!wasmModule) return;
  const ce = e as ClipboardEvent;
  const text = (wasmModule["wasm_clipboard_get"] as ClipGetFn)();
  const html = (wasmModule["wasm_clipboard_get_html"] as ClipGetFn | undefined)?.();
  if (text !== null && text !== undefined && text.length > 0) {
    ce.clipboardData?.setData("text/plain", text);
    if (html !== null && html !== undefined && html.length > 0) {
      ce.clipboardData?.setData("text/html", html);
    }
    ce.preventDefault();
  }
}

canvas.addEventListener("copy", handleCopy);
mobileTextInput.addEventListener("copy", handleCopy);

// cut: same as copy — WASM cut the text and stored it in the buffer.
function handleCut(e: Event) {
  if (!wasmModule) return;
  const ce = e as ClipboardEvent;
  const text = (wasmModule["wasm_clipboard_get"] as ClipGetFn)();
  const html = (wasmModule["wasm_clipboard_get_html"] as ClipGetFn | undefined)?.();
  if (text !== null && text !== undefined && text.length > 0) {
    ce.clipboardData?.setData("text/plain", text);
    if (html !== null && html !== undefined && html.length > 0) {
      ce.clipboardData?.setData("text/html", html);
    }
    ce.preventDefault();
  }
}

canvas.addEventListener("cut", handleCut);
mobileTextInput.addEventListener("cut", handleCut);

// paste: get text from the system clipboard, store in the in-process buffer,
//        then synthesise a Ctrl+V key event so Rust's paste handler fires.
function handlePaste(e: Event) {
  if (!wasmModule) return;
  const ce = e as ClipboardEvent;
  const text = ce.clipboardData?.getData("text/plain") ?? "";
  if (text.length > 0) {
    ce.preventDefault();
    (wasmModule["wasm_clipboard_set"] as ClipSetFn)(text);
    (wasmModule["on_key_down"] as KeyFn)("v", false, true, false, false);
  }
  mobileTextInput.value = "";
}

canvas.addEventListener("paste", handlePaste);
mobileTextInput.addEventListener("paste", handlePaste);

canvas.addEventListener("contextmenu", (e) => e.preventDefault());

// --- Touch support ---
//
// Single-touch taps are forwarded as mouse button 0 on release.  Once the
// finger moves far enough to count as scrolling, we synthesize a middle-button
// drag instead of wheel ticks so ScrollView captures the gesture and pans by
// the exact finger delta on both axes.
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
  // Track the first finger for tap/scroll mouse emulation.  Once a
  // primary is established, later touches skip this.
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
          (wasmModule["on_mouse_down"] as MouseXYBFn)(
            primaryTouchStart[0],
            primaryTouchStart[1],
            1,
          );
        }
        (wasmModule["on_mouse_move"] as MouseXYFn)(x, y);
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
        syncMobileKeyboard();
      } else {
        (wasmModule["on_mouse_up"] as MouseXYBFn)(x, y, 1);
        syncMobileKeyboard();
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
  e.preventDefault();
  for (const t of Array.from(e.changedTouches)) {
    (wasmModule["on_touch_cancel"] as TouchEndFn)(t.identifier);
    if (t.identifier === primaryTouchId) {
      const [x, y] = touchPos(t);
      if (primaryTouchScrolling) {
        (wasmModule["on_mouse_up"] as MouseXYBFn)(x, y, 1);
      }
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

function setLoadingText(text: string) {
  loadingTextEl.textContent = text;
}

function parseFontRequest(request: string): [string, string] {
  const tab = request.indexOf("\t");
  if (tab < 0) throw new Error(`Invalid font request: ${request}`);
  return [request.slice(0, tab), request.slice(tab + 1)];
}

function fetchFontBytes(path: string): Promise<Uint8Array> {
  let pending = fontFetchCache.get(path);
  if (!pending) {
    pending = fetch(new URL(path, location.href))
      .then((response) => {
        if (!response.ok) throw new Error(`Font fetch failed for ${path}: ${response.status}`);
        return response.arrayBuffer();
      })
      .then((buffer) => new Uint8Array(buffer));
    fontFetchCache.set(path, pending);
  }
  return pending;
}

function fallbackFontPaths(module: Record<string, unknown>): [string, string] {
  return parseFontRequest((module["fallback_font_paths"] as StringFn)());
}

async function installFontRequest(module: Record<string, unknown>, request: string) {
  const [name, path] = parseFontRequest(request);
  const [iconsPath, emojiPath] = fallbackFontPaths(module);
  const [primary, icons, emoji] = await Promise.all([
    fetchFontBytes(path),
    fetchFontBytes(iconsPath),
    fetchFontBytes(emojiPath),
  ]);
  const ok = (module["install_loaded_font"] as InstallFontFn)(name, primary, icons, emoji);
  if (!ok) throw new Error(`WASM rejected font ${name}`);
}

async function drainPendingFontRequests() {
  if (!wasmModule || fontDrainRunning) return;
  const takeRequest = wasmModule["take_pending_font_request"] as OptionalStringFn | undefined;
  if (!takeRequest) return;

  fontDrainRunning = true;
  try {
    for (;;) {
      const request = takeRequest();
      if (!request) break;
      await installFontRequest(wasmModule, request);
    }
  } catch (e) {
    console.error(e);
  } finally {
    fontDrainRunning = false;
  }
}

async function init() {
  try {
    setLoadingText("Loading WASM…");
    const wasm    = await import("../public/pkg/demo_wasm.js");
    const wasmUrl = new URL("./public/pkg/demo_wasm_bg.wasm", location.href);
    await wasm.default({ module_or_path: wasmUrl });

    const module = wasm as unknown as Record<string, unknown>;
    (module["set_client_platform"] as SetPlatformFn | undefined)?.(
      detectClientPlatform(),
      detectPointerCoarse(),
    );
    setLoadingText("Loading fonts…");
    await installFontRequest(module, (module["default_font_request"] as StringFn)());

    wasmModule = module;
    // Expose on window so Playwright tests can access WASM functions directly.
    (window as unknown as Record<string, unknown>).__wasm = wasmModule;
    setLoadingText("Drawing first frame…");
    render();
    loadingEl.classList.add("hidden");

    setupScreenShare(module);

    // Start the reactive animation loop.
    requestAnimationFrame(animationLoop);
  } catch (e) {
    setLoadingText(`Error loading WASM: ${e}`);
    console.error(e);
  }
}

// ---------------------------------------------------------------------------
// Screen Share (WebRTC) — one page, two roles
// ---------------------------------------------------------------------------
//
// Opened with `?host=<id>` the page is a *sender*: it still runs the agg-gui
// app, and additionally streams this canvas (full resolution) to the desktop
// that owns <id>. Opened normally it is a *receiver*: it registers its own peer
// id (the QR encodes this page's URL + ?host=<id>), accepts a phone's frames,
// and hands the decoded pixels to the demo's Screen Share window.
//
// Frames are chunked over the reliable, ordered data channel (full-res images
// exceed the per-message limit). Header is 12 bytes LE — must match the Rust
// `screen_share::chunk` module: u32 frame_seq, u16 chunk_index, u16 chunk_count,
// u32 total_len, then the payload.

const SHARE_CHUNK_PAYLOAD = 16 * 1024;
const SHARE_STALE_MS = 2500;
const SHARE_BACKPRESSURE = 8 * 1024 * 1024;

type PeerLike = {
  on(ev: string, cb: (arg: unknown) => void): void;
  connect(id: string, opts: unknown): ConnLike;
};
type ConnLike = {
  open: boolean;
  dataChannel?: RTCDataChannel;
  on(ev: string, cb: (arg: unknown) => void): void;
  send(data: Uint8Array): void;
};

function getPeerCtor(): (new (id?: string | object, opts?: object) => PeerLike) | null {
  const ctor = (window as unknown as Record<string, unknown>)["Peer"];
  return typeof ctor === "function"
    ? (ctor as new (id?: string | object, opts?: object) => PeerLike)
    : null;
}

function setupScreenShare(module: Record<string, unknown>) {
  const Peer = getPeerCtor();
  if (!Peer) {
    console.warn("screen-share: PeerJS not loaded; skipping");
    return;
  }
  const host = new URLSearchParams(location.search).get("host");
  if (host) {
    startSender(Peer, host, module);
  } else {
    startReceiver(Peer, module);
  }
}

function chunkAndSend(conn: ConnLike, seq: number, bytes: Uint8Array) {
  const total = bytes.length;
  const count = Math.max(1, Math.ceil(total / SHARE_CHUNK_PAYLOAD));
  for (let i = 0; i < count; i++) {
    const start = i * SHARE_CHUNK_PAYLOAD;
    const slice = bytes.subarray(start, Math.min(start + SHARE_CHUNK_PAYLOAD, total));
    const msg = new Uint8Array(12 + slice.length);
    const dv = new DataView(msg.buffer);
    dv.setUint32(0, seq >>> 0, true);
    dv.setUint16(4, i, true);
    dv.setUint16(6, count, true);
    dv.setUint32(8, total, true);
    msg.set(slice, 12);
    conn.send(msg);
  }
}

function startSender(
  Peer: new (id?: string | object, opts?: object) => PeerLike,
  host: string,
  module: Record<string, unknown>,
) {
  // Enable in-wasm capture+encode: each render(), the wasm side grabs the
  // canvas via the screenshot GPU-readback path and frame-diff encodes it.
  (module["screen_share_set_sender"] as ((b: boolean) => void) | undefined)?.(true);
  const takePacket = module["screen_share_take_packet"] as (() => Uint8Array) | undefined;

  const peer = new Peer({ debug: 1 });
  let conn: ConnLike | null = null;
  let seq = 0;

  peer.on("open", () => {
    // serialization "none" → raw bytes reach the desktop (native webrtc-rs or
    // the browser receiver) verbatim, with no BinaryPack wrapping.
    conn = peer.connect(host, { reliable: true, serialization: "none" });
    conn.on("open", () => console.log(`screen-share: streaming to ${host}`));
    conn.on("close", () => { conn = null; });
    conn.on("error", (err) => console.warn("screen-share: conn error", err));
  });
  peer.on("error", (err) => console.warn("screen-share: peer error", err));

  shareCapture = () => {
    if (!conn || conn.open === false || !takePacket) return;
    const dc = conn.dataChannel;
    if (dc && dc.bufferedAmount > SHARE_BACKPRESSURE) return; // let the link drain
    const packet = takePacket();
    if (!packet || packet.length === 0) return; // nothing new this frame
    try {
      chunkAndSend(conn, seq, packet);
      seq = (seq + 1) >>> 0;
    } catch {
      /* drop frame */
    }
  };
}

function startReceiver(
  Peer: new (id?: string | object, opts?: object) => PeerLike,
  module: Record<string, unknown>,
) {
  const id = (module["screen_peer_id"] as StringFn | undefined)?.();
  if (!id) {
    console.warn("screen-share: no peer id from wasm; receiver disabled");
    return;
  }
  const setConnected = module["set_screen_connected"] as ((b: boolean) => void) | undefined;
  // The wasm side decodes each reassembled codec packet (frame-diff) and writes
  // it to the live-view slot.
  const pushEncoded = module["push_screen_encoded"] as ((packet: Uint8Array) => void) | undefined;

  // Tell the wasm QR widget which URL to encode: this same page + ?host=<id>.
  const setPhoneUrl = module["set_phone_url"] as ((url: string) => void) | undefined;
  setPhoneUrl?.(`${location.origin}${location.pathname}?host=${id}`);

  const peer = new Peer(id, { debug: 1 });
  peer.on("open", () => console.log(`screen-share: receiver registered as ${id}`));
  peer.on("error", (err) => console.warn("screen-share: peer error", err));
  peer.on("connection", (connRaw) => {
    const conn = connRaw as ConnLike;
    setConnected?.(true);
    let lastMsg = performance.now();

    // Chunk reassembly (ordered, reliable channel → in-order chunks).
    let active = false;
    let curSeq = 0;
    let curCount = 0;
    let received = 0;
    let parts: Uint8Array[] = [];

    const onChunk = (data: Uint8Array) => {
      if (data.length < 12) return;
      const dv = new DataView(data.buffer, data.byteOffset, data.byteLength);
      const seq = dv.getUint32(0, true);
      const idx = dv.getUint16(4, true);
      const count = dv.getUint16(6, true);
      const payload = data.subarray(12);
      if (idx === 0) {
        active = true;
        curSeq = seq;
        curCount = count;
        received = 0;
        parts = [];
      }
      if (!active || seq !== curSeq) return;
      parts.push(payload);
      received++;
      if (curCount === 0 || received >= curCount) {
        active = false;
        pushEncoded?.(concatBytes(parts));
      }
    };

    conn.on("data", (raw) => {
      lastMsg = performance.now();
      onChunk(toUint8(raw));
    });
    conn.on("close", () => setConnected?.(false));
    conn.on("error", () => setConnected?.(false));

    const timer = window.setInterval(() => {
      if (performance.now() - lastMsg > SHARE_STALE_MS) setConnected?.(false);
    }, 500);
    conn.on("close", () => window.clearInterval(timer));
  });
}

function toUint8(raw: unknown): Uint8Array {
  if (raw instanceof Uint8Array) return raw;
  if (raw instanceof ArrayBuffer) return new Uint8Array(raw);
  if (ArrayBuffer.isView(raw)) {
    const v = raw as ArrayBufferView;
    return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  }
  return new Uint8Array(0);
}

function concatBytes(parts: Uint8Array[]): Uint8Array {
  let len = 0;
  for (const p of parts) len += p.length;
  const out = new Uint8Array(len);
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return out;
}

init();
