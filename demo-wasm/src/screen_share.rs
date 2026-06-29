//! WASM screen-share receiver glue.
//!
//! The desktop **web** build is a receiver, mirroring the native shell: it
//! registers a peer id, shows the QR, and accepts JPEG frames from the phone.
//! Decoding happens in TypeScript (the browser's `createImageBitmap`); this
//! layer just exposes the peer id, accepts decoded RGBA frames, and tracks the
//! connection flag — the same `QueuedScreenTransport` seam the native bridge
//! fills.
//!
//! Mirrors Marbles' `marbles-wasm` transport plumbing.

use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex};

use demo_ui::{FrameDecoder, FrameEncoder};
use wasm_bindgen::prelude::*;

use crate::mark_dirty;

thread_local! {
    static SCREEN_LATEST: RefCell<Option<Arc<Mutex<Option<demo_ui::ScreenFrame>>>>> =
        const { RefCell::new(None) };
    static SCREEN_CONNECTED: RefCell<Option<Arc<Mutex<bool>>>> = const { RefCell::new(None) };
    static SCREEN_PEER_ID: RefCell<String> = const { RefCell::new(String::new()) };
    /// Shared with the demo's QR widget; the TS receiver fills it with the full
    /// page URL + `?host=<id>` (it has `location.origin` for free, and doing it
    /// in JS avoids pulling a web-sys `Location` import into this shell).
    static PHONE_URL_SLOT: RefCell<Option<std::rc::Rc<RefCell<String>>>> =
        const { RefCell::new(None) };

    // ── Receiver: frame-diff decoder ───────────────────────────────────────
    static SCREEN_DECODER: RefCell<Option<FrameDecoder>> = const { RefCell::new(None) };
    /// Receiver: pending Stream On/Off command for the TS receiver to transmit
    /// to the phone. `Some(true|false)` once the user flips the switch; drained
    /// to `None` by [`screen_share_take_control`].
    static SCREEN_CONTROL_OUT: RefCell<Option<bool>> = const { RefCell::new(None) };

    // ── Sender (?host= mode): capture + frame-diff encoder ─────────────────
    /// True when this page is a screen-share sender; `render` then captures and
    /// encodes the canvas each frame.
    static SENDER_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static SCREEN_ENCODER: RefCell<Option<FrameEncoder>> = const { RefCell::new(None) };
    /// Latest encoded packet awaiting send; the TS sender drains it each frame.
    static SCREEN_OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Called from `render` after `end_frame`: if this page is a sender, capture the
/// just-rendered canvas via the screenshot GPU-readback path (same as the
/// Screenshot demo's Save/Copy — `canvas.toDataURL` returns a blank buffer on a
/// non-preserved WebGL canvas), then frame-diff encode it for the TS sender to
/// drain via [`screen_share_take_packet`].
pub(crate) fn maybe_capture(ctx: &mut demo_wgpu::WgpuGfxCtx) {
    if !SENDER_ACTIVE.with(|c| c.get()) {
        return;
    }
    use agg_gui::DrawCtx;

    // 1. Harvest a readback started on an earlier frame.  On the web the GPU→CPU
    //    map only completes once the event loop turns, so a blocking read here
    //    (as the native Save/Copy path does) would deadlock the single JS thread
    //    — the freeze that left the phone stuck on the loading screen.  Instead
    //    we poll: when the pixels are ready, frame-diff encode them.
    if let Some((rgba, w, h)) = ctx.poll_capture_readback() {
        if !rgba.is_empty() && w != 0 && h != 0 {
            SCREEN_ENCODER.with(|c| {
                let mut enc = c.borrow_mut();
                let enc = enc.get_or_insert_with(FrameEncoder::new);
                let packet = enc.encode(&rgba, w, h);
                SCREEN_OUT.with(|o| *o.borrow_mut() = packet);
            });
        }
    }

    // 2. With no readback in flight, snapshot this just-rendered frame (GPU-only
    //    texture copy) and start a new async readback.  The encoded packet for it
    //    lands a frame or two later — imperceptible for a live shared view.
    if !ctx.has_pending_readback() && ctx.capture_screenshot() {
        ctx.begin_capture_readback();
    }
}

/// Inject a real transport into the demo's screen-share seam. Called once from
/// `ensure_demo_app` after `build_demo_ui`.
pub(crate) fn install(handles: &demo_ui::ScreenShareHandles) {
    let peer_id = gen_peer_id();
    let latest: Arc<Mutex<Option<demo_ui::ScreenFrame>>> = Arc::new(Mutex::new(None));
    let connected: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let streaming: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));

    // Control sink: queue the command for the TS receiver to send to the phone
    // over the data channel (drained via `screen_share_take_control`).
    let control: Arc<dyn Fn(bool) + Send + Sync> = Arc::new(|on: bool| {
        SCREEN_CONTROL_OUT.with(|c| *c.borrow_mut() = Some(on));
        mark_dirty();
    });

    *handles.transport.borrow_mut() = Box::new(
        demo_ui::QueuedScreenTransport::new(latest.clone(), connected.clone(), peer_id.clone())
            .with_streaming_flag(streaming.clone())
            .with_control(control),
    );

    SCREEN_LATEST.with(|c| *c.borrow_mut() = Some(latest));
    SCREEN_CONNECTED.with(|c| *c.borrow_mut() = Some(connected));
    SCREEN_PEER_ID.with(|c| *c.borrow_mut() = peer_id);
    PHONE_URL_SLOT.with(|c| *c.borrow_mut() = Some(std::rc::Rc::clone(&handles.phone_url)));
}

/// A short peer id like `ag-mqz4xp`, using `Math.random` so we avoid pulling a
/// `getrandom`/`js` dependency into this shell just for six characters.
fn gen_peer_id() -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut s = String::from("ag-");
    for _ in 0..6 {
        let idx = (js_sys::Math::random() * ALPHABET.len() as f64) as usize;
        s.push(ALPHABET[idx.min(ALPHABET.len() - 1)] as char);
    }
    s
}

// ── wasm-bindgen exports for the TypeScript receiver ───────────────────────

/// The peer id the QR encodes; JS registers PeerJS under this id.
#[wasm_bindgen]
pub fn screen_peer_id() -> String {
    crate::ensure_demo_app();
    SCREEN_PEER_ID.with(|c| c.borrow().clone())
}

/// Hand a received codec packet (one reassembled frame) to the demo. Decoded in
/// Rust via the shared [`FrameDecoder`] and written to the live-view slot.
#[wasm_bindgen]
pub fn push_screen_encoded(packet: &[u8]) {
    // A frame arrived → stay connected even if it's a pre-keyframe delta.
    SCREEN_CONNECTED.with(|c| {
        if let Some(flag) = c.borrow().as_ref() {
            if let Ok(mut f) = flag.lock() {
                *f = true;
            }
        }
    });
    let decoded = SCREEN_DECODER.with(|c| {
        let mut dec = c.borrow_mut();
        let dec = dec.get_or_insert_with(FrameDecoder::new);
        dec.decode(packet)
    });
    if let Some((rgba, width, height)) = decoded {
        SCREEN_LATEST.with(|c| {
            if let Some(slot) = c.borrow().as_ref() {
                if let Ok(mut latest) = slot.lock() {
                    *latest = Some((rgba, width, height));
                }
            }
        });
    }
    // The live view is refreshed by `ScreenShareView::sync`, which pulls the
    // newest frame from `layout()`.  `render_app_frame` (demo-wgpu) only re-runs
    // layout when the invalidation epoch changes — so `mark_dirty` alone forces
    // a *repaint* that reuses the old layout and never pulls the new frame, and
    // the view updates only when something else happens to invalidate. Bump the
    // epoch so layout (and the frame pull) actually runs each received frame.
    mark_dirty();
    agg_gui::animation::request_draw();
}

/// Enable sender mode (page opened with `?host=`). `render` then captures and
/// encodes the canvas each frame.
#[wasm_bindgen]
pub fn screen_share_set_sender(active: bool) {
    SENDER_ACTIVE.with(|c| c.set(active));
    if active {
        SCREEN_ENCODER.with(|c| {
            if c.borrow().is_none() {
                *c.borrow_mut() = Some(FrameEncoder::new());
            }
        });
    }
    mark_dirty();
}

/// Drain the latest encoded frame packet for the TS sender to transmit. Returns
/// an empty vec when no new frame is pending.
#[wasm_bindgen]
pub fn screen_share_take_packet() -> Vec<u8> {
    SCREEN_OUT.with(|o| std::mem::take(&mut *o.borrow_mut()))
}

/// Drain a pending Stream On/Off command for the TS receiver to transmit to the
/// phone. Returns `1` (start), `0` (stop), or `-1` when nothing is pending.
#[wasm_bindgen]
pub fn screen_share_take_control() -> i32 {
    SCREEN_CONTROL_OUT.with(|c| match c.borrow_mut().take() {
        Some(true) => 1,
        Some(false) => 0,
        None => -1,
    })
}

/// The TS receiver computes the QR URL (page origin + `?host=<id>`) and sets it
/// here so the demo's QR widget can render it.
#[wasm_bindgen]
pub fn set_phone_url(url: String) {
    PHONE_URL_SLOT.with(|c| {
        if let Some(slot) = c.borrow().as_ref() {
            *slot.borrow_mut() = url;
        }
    });
    mark_dirty();
}

/// Set by the TS receiver on connect / disconnect / timeout.
#[wasm_bindgen]
pub fn set_screen_connected(yes: bool) {
    SCREEN_CONNECTED.with(|c| {
        if let Some(flag) = c.borrow().as_ref() {
            if let Ok(mut f) = flag.lock() {
                *f = yes;
            }
        }
    });
    // Connect/disconnect swaps the QR for the live view — a `Conditional`, which
    // zeroes its bounds in `layout()`.  Bump the epoch so that swap actually
    // re-lays-out; a bare repaint would keep the stale geometry.
    mark_dirty();
    agg_gui::animation::request_draw();
}
