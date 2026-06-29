// agg-gui demo — Screen Share (WebRTC) transport glue.
//
// Split out of `app.ts` to keep that file under the project's 800-line limit;
// behaviour is unchanged. `app.ts` calls `setupScreenShare(module)` once after
// WASM init, then `tickScreenShare(now, render)` every rAF tick.
//
// One page, two roles:
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

type StringFn = () => string;
type WasmModule = Record<string, unknown>;

const SHARE_CHUNK_PAYLOAD = 16 * 1024;
const SHARE_STALE_MS = 2500;
const SHARE_BACKPRESSURE = 8 * 1024 * 1024;

// Cap the sender's render+capture rate.  In sender mode every render() also does
// a full-canvas GPU readback + frame-diff encode (in the wasm), which is far too
// expensive to run at the ~60fps rAF cadence on a phone — back-to-back readbacks
// saturate the main thread, the browser never composites, and the page looks
// frozen on the loading overlay.  ~15fps is plenty for a shared UI view and lets
// the loop yield between readbacks so the page stays responsive.
const SENDER_CAPTURE_INTERVAL_MS = 66;

// --- Loop-driven state (was module-level in app.ts) ---

// When this page is a sender (?host=<id>), the capture hook is installed here. It
// must run inside the rAF tick, right after render(), so the WebGL drawing buffer
// is still intact when we snapshot it.
let shareCapture: ((now: number) => void) | null = null;

// Sender (?host=) gate. Starts false so a freshly-loaded phone connects but does
// NO capture/encode/transmit until the desktop turns Stream on. Flipped by the
// Stream On/Off command the desktop sends over the data channel.
let senderStreaming = false;

// Receiver (desktop web build) pump: each tick, drain any queued Stream On/Off
// command from the wasm side and forward it to the phone. Null until the receiver
// is set up.
let receiverControlPump: (() => void) | null = null;

let lastSenderCapture = 0;

// Desktop→phone Stream switch: a 3-byte command, "SS" prefix + '1' (start) or
// '0' (stop). Matches the Rust `bridge::stream_command_bytes`. Only the phone
// (sender) ever receives data on the channel, so this never collides with the
// frame chunks flowing the other way.
const STREAM_CMD_BYTE = 0x53; // 'S'
function streamCmdBytes(on: boolean): Uint8Array {
  return new Uint8Array([STREAM_CMD_BYTE, STREAM_CMD_BYTE, on ? 0x31 : 0x30]);
}
function parseStreamCmd(data: Uint8Array): boolean | null {
  if (data.length < 3 || data[0] !== STREAM_CMD_BYTE || data[1] !== STREAM_CMD_BYTE) {
    return null;
  }
  return data[2] === 0x31;
}

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

/// Install the sender / receiver role for this page. Call once after WASM init.
export function setupScreenShare(module: WasmModule) {
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

/// Drive the screen-share work for this rAF tick.
///
/// Runs the receiver's Stream-command pump, and — in sender streaming mode —
/// throttle-renders + captures. Returns `true` if it owns rendering this tick
/// (so the caller skips its own `needs_draw`-gated render); `false` for a
/// receiver or an idle sender, leaving the normal reactive render to the caller.
export function tickScreenShare(now: number, render: () => void): boolean {
  if (receiverControlPump) receiverControlPump();
  if (shareCapture && senderStreaming) {
    // Sender mode, streaming on: keep the streamed view live, but throttled.
    if (now - lastSenderCapture >= SENDER_CAPTURE_INTERVAL_MS) {
      lastSenderCapture = now;
      render();
      shareCapture(now);
    }
    return true;
  }
  return false;
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
  module: WasmModule,
) {
  // In-wasm capture+encode is gated by the Stream switch: it stays OFF until the
  // desktop sends a "start" command, so an idle phone does no readback/encode.
  const setSender = module["screen_share_set_sender"] as ((b: boolean) => void) | undefined;
  setSender?.(false);
  senderStreaming = false;
  const takePacket = module["screen_share_take_packet"] as (() => Uint8Array) | undefined;

  const applyStreamCmd = (raw: unknown) => {
    const on = parseStreamCmd(toUint8(raw));
    if (on === null) return; // not a control message
    senderStreaming = on;
    setSender?.(on);
    console.log(`screen-share: desktop set streaming ${on ? "on" : "off"}`);
  };

  const peer = new Peer({ debug: 1 });
  let conn: ConnLike | null = null;
  let seq = 0;

  peer.on("open", () => {
    // serialization "raw" → bytes reach the desktop (native webrtc-rs or the
    // browser receiver) verbatim, with no BinaryPack wrapping.  NOTE: must be
    // "raw", NOT "none": "none" is not a valid PeerJS serialization value, and
    // PeerData's DataConnection silently fails to create/send its OFFER when it
    // sees an unknown value — so the phone connected, registered, and only ever
    // sent HEARTBEATs while the desktop waited forever for an offer that never
    // came. "raw" is the value that both emits the offer and sends unwrapped
    // bytes.
    conn = peer.connect(host, { reliable: true, serialization: "raw" });
    conn.on("open", () => console.log(`screen-share: connected to ${host} (awaiting Stream on)`));
    // The same data channel is bidirectional: the desktop sends Stream On/Off
    // commands back to us here.
    conn.on("data", applyStreamCmd);
    conn.on("close", () => {
      conn = null;
      // Lost the link — stop capturing until a new connection turns us back on.
      senderStreaming = false;
      setSender?.(false);
    });
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
  module: WasmModule,
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
  // Drains the Stream On/Off command queued by the demo's Stream control.
  const takeControl = module["screen_share_take_control"] as (() => number) | undefined;

  // Tell the wasm QR widget which URL to encode: this same page + ?host=<id>.
  const setPhoneUrl = module["set_phone_url"] as ((url: string) => void) | undefined;
  setPhoneUrl?.(`${location.origin}${location.pathname}?host=${id}`);

  // Mirror of the desktop's Stream switch, kept so a phone that connects later
  // can be told the current state, and so the stale-timeout only trips while we
  // actually expect frames.
  let streamingOn = false;
  let activeConn: ConnLike | null = null;

  const sendControl = (conn: ConnLike, on: boolean) => {
    try {
      conn.send(streamCmdBytes(on));
    } catch {
      /* channel not ready yet — the pump / connect handler will retry */
    }
  };

  // Forward queued Stream commands to the phone each tick (see tickScreenShare).
  receiverControlPump = () => {
    if (!takeControl) return;
    const cmd = takeControl();
    if (cmd < 0) return; // nothing queued
    streamingOn = cmd === 1;
    if (activeConn) sendControl(activeConn, streamingOn);
  };

  const peer = new Peer(id, { debug: 1 });
  peer.on("open", () => console.log(`screen-share: receiver registered as ${id}`));
  peer.on("error", (err) => console.warn("screen-share: peer error", err));
  peer.on("connection", (connRaw) => {
    const conn = connRaw as ConnLike;
    activeConn = conn;
    setConnected?.(true);
    let lastMsg = performance.now();

    // Tell the freshly-connected phone the current Stream state (it boots idle).
    conn.on("open", () => sendControl(conn, streamingOn));
    if (conn.open) sendControl(conn, streamingOn);

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
    const onGone = () => {
      if (activeConn === conn) activeConn = null;
      setConnected?.(false);
    };
    conn.on("close", onGone);
    conn.on("error", onGone);

    const timer = window.setInterval(() => {
      // Only treat silence as a drop while streaming — when Stream is Off the
      // phone is meant to be idle, so no frames is expected, not a stall.
      if (streamingOn && performance.now() - lastMsg > SHARE_STALE_MS) setConnected?.(false);
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
