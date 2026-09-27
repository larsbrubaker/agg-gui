//! Page lifecycle: the held-button count behind
//! [`crate::WebShellControl::pointer_idle`], the window-level release listener
//! that keeps it honest, the `visibilitychange` / `pagehide` flush that calls
//! [`crate::WebShellHost::on_page_hide`], and the window `resize` listener.
//!
//! Extracted from AtomArtist's `demo-wasm/src/web_lifecycle.rs`. The count is
//! maintained by three independent paths, each sufficient to reopen the guard
//! after a lost release: the canvas pointer listeners ([`super::input`]), the
//! `buttons` resync on every move, and the window-level listener here — which
//! catches a release over browser chrome, where the canvas never hears it.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use super::{mark_dirty, BUTTONS_HELD, HOST};
use crate::dom_math::pressed_button_count;

/// Adopt the authoritative held-button count from an event's `buttons`.
pub(super) fn sync_buttons(count: u32) {
    BUTTONS_HELD.with(|b| b.set(count));
}

fn listen(target: &web_sys::EventTarget, event: &str, f: impl FnMut(web_sys::Event) + 'static) {
    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(f);
    if target
        .add_event_listener_with_callback(event, cb.as_ref().unchecked_ref())
        .is_ok()
    {
        cb.forget();
    }
}

/// `pointerup` / `mouseup` / `pointercancel` on `window`. Both up events are
/// registered: `pointerup` covers touch and pen, `mouseup` a browser without
/// Pointer Events. A release seen twice is harmless — both assign the same
/// derived count rather than decrementing.
pub(super) fn install_window_pointer_release() {
    let Some(window) = web_sys::window() else {
        return;
    };
    for event in ["pointerup", "mouseup", "pointercancel"] {
        listen(window.as_ref(), event, |e: web_sys::Event| {
            // A cancel without mouse data reads as "nothing held" — the safe
            // direction: worst case settings persist a frame early.
            let buttons = e
                .dyn_ref::<web_sys::MouseEvent>()
                .map(|m| m.buttons())
                .unwrap_or(0);
            sync_buttons(pressed_button_count(buttons));
        });
    }
}

fn page_hide() {
    super::APP.with(|a| {
        HOST.with(|h| {
            let (Ok(mut app), Ok(mut host)) = (a.try_borrow_mut(), h.try_borrow_mut()) else {
                return;
            };
            if let (Some(app), Some(host)) = (app.as_mut(), host.as_mut()) {
                host.on_page_hide(app);
            }
        })
    });
}

/// `visibilitychange` → hidden and `pagehide` are the documented teardown
/// hooks; they fire where `beforeunload`/`unload` famously don't (a mobile tab
/// backgrounded by the OS, a page frozen into the back/forward cache).
pub(super) fn install_page_hide() {
    let Some(window) = web_sys::window() else {
        return;
    };
    if let Some(document) = window.document() {
        let doc = document.clone();
        listen(document.as_ref(), "visibilitychange", move |_| {
            if doc.hidden() {
                page_hide();
            } else {
                // Back from hidden: repaint whatever changed meanwhile.
                mark_dirty();
            }
        });
    }
    listen(window.as_ref(), "pagehide", |_| page_hide());
}

/// Window resizes / monitor moves / browser zoom. The rAF tick re-syncs
/// size and DPR anyway; this just makes sure the next tick paints.
pub(super) fn install_resize() {
    let Some(window) = web_sys::window() else {
        return;
    };
    listen(window.as_ref(), "resize", |_| mark_dirty());
}
