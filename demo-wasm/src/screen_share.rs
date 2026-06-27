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

use std::sync::{Arc, Mutex};

use wasm_bindgen::prelude::*;

use crate::mark_dirty;

thread_local! {
    static SCREEN_LATEST: std::cell::RefCell<Option<Arc<Mutex<Option<demo_ui::ScreenFrame>>>>> =
        const { std::cell::RefCell::new(None) };
    static SCREEN_CONNECTED: std::cell::RefCell<Option<Arc<Mutex<bool>>>> =
        const { std::cell::RefCell::new(None) };
    static SCREEN_PEER_ID: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    /// Shared with the demo's QR widget; the TS receiver fills it with the full
    /// page URL + `?host=<id>` (it has `location.origin` for free, and doing it
    /// in JS avoids pulling a web-sys `Location` import into this shell).
    static PHONE_URL_SLOT: std::cell::RefCell<Option<std::rc::Rc<std::cell::RefCell<String>>>> =
        const { std::cell::RefCell::new(None) };
}

/// Inject a real transport into the demo's screen-share seam. Called once from
/// `ensure_demo_app` after `build_demo_ui`.
pub(crate) fn install(handles: &demo_ui::ScreenShareHandles) {
    let peer_id = gen_peer_id();
    let latest: Arc<Mutex<Option<demo_ui::ScreenFrame>>> = Arc::new(Mutex::new(None));
    let connected: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));

    *handles.transport.borrow_mut() = Box::new(demo_ui::QueuedScreenTransport::new(
        latest.clone(),
        connected.clone(),
        peer_id.clone(),
    ));

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

/// Hand a decoded frame (top-down RGBA8) to the demo. JS decodes the JPEG via
/// `createImageBitmap` + a 2-D canvas and passes the pixels here.
#[wasm_bindgen]
pub fn push_screen_frame(rgba: &[u8], width: u32, height: u32) {
    SCREEN_LATEST.with(|c| {
        if let Some(slot) = c.borrow().as_ref() {
            if let Ok(mut latest) = slot.lock() {
                *latest = Some((rgba.to_vec(), width, height));
            }
        }
    });
    SCREEN_CONNECTED.with(|c| {
        if let Some(flag) = c.borrow().as_ref() {
            if let Ok(mut f) = flag.lock() {
                *f = true;
            }
        }
    });
    mark_dirty();
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
    mark_dirty();
}
