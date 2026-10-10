//! Page lifecycle: the held-pointer state behind
//! [`crate::WebShellControl::pointer_idle`] (mouse buttons *and* touch
//! contacts, see [`crate::pointer`]), the window-level release listener that
//! keeps it honest, the `visibilitychange` / `pagehide` flush that calls
//! [`crate::WebShellHost::on_page_hide`], the window `resize` listener, and
//! the `webglcontextlost` listener, and the window `focus` / `blur` pair that
//! becomes `App::on_window_activated` / `App::on_window_deactivated`.
//!
//! Extracted from AtomArtist's `demo-wasm/src/web_lifecycle.rs`. The count is
//! maintained by three independent paths, each sufficient to reopen the guard
//! after a lost release: the canvas pointer listeners ([`super::input`]), the
//! `buttons` resync on every move, and the window-level listener here — which
//! catches a release over browser chrome, where the canvas never hears it.

use agg_gui::shell_input::ForwarderEvent;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use std::cell::Cell;

use super::{mark_dirty, HOST, POINTERS};
use crate::dom_math::{pressed_button_count, PointerKind};

thread_local! {
    static WEBGL_CONTEXT_LOST: Cell<bool> = const { Cell::new(false) };
}

/// Adopt the authoritative held-button count from an event's `buttons`.
pub(super) fn sync_buttons(count: u32) {
    POINTERS.with(|p| p.borrow_mut().sync_mouse_buttons(count));
}

pub(super) fn touch_down(pointer_id: i32) {
    POINTERS.with(|p| p.borrow_mut().touch_down(pointer_id));
}

pub(super) fn touch_up(pointer_id: i32) {
    POINTERS.with(|p| p.borrow_mut().touch_up(pointer_id));
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
            if let Some(p) = e.dyn_ref::<web_sys::PointerEvent>() {
                if PointerKind::from_dom(&p.pointer_type()) == PointerKind::Touch {
                    // A finger lifted (possibly off the canvas); its
                    // `buttons` says nothing about the mouse.
                    touch_up(p.pointer_id());
                    return;
                }
            }
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
                // No release will be delivered to a hidden page.
                POINTERS.with(|p| p.borrow_mut().clear());
                page_hide();
            } else {
                // Back from hidden: repaint whatever changed meanwhile.
                mark_dirty();
            }
        });
    }
    listen(window.as_ref(), "pagehide", |_| page_hide());
}

/// `blur` / `focus` on `window`: the page losing or regaining focus — the
/// user switched tabs or applications, or clicked into the browser chrome or
/// another frame. Focus moving between elements inside the page (the hidden
/// IME `<textarea>`) fires neither on `window`. Becomes
/// `ForwarderEvent::WindowDeactivated` / `WindowActivated`, which also ends a
/// pointer capture whose release the page will never hear.
pub(super) fn install_window_activation() {
    let Some(window) = web_sys::window() else {
        return;
    };
    listen(window.as_ref(), "blur", |_| {
        super::forward(ForwarderEvent::WindowDeactivated);
        super::note_input();
    });
    listen(window.as_ref(), "focus", |_| {
        super::forward(ForwarderEvent::WindowActivated);
        super::note_input();
    });
}

/// Window resizes / monitor moves / browser zoom. The rAF tick re-syncs
/// size and DPR anyway; this just makes sure the next tick paints.
pub(super) fn install_resize() {
    let Some(window) = web_sys::window() else {
        return;
    };
    listen(window.as_ref(), "resize", |_| mark_dirty());
}

/// `webglcontextlost` on the canvas (fires only for a WebGL2 surface). The
/// frame loop reports it as fatal — see [`take_webgl_context_lost`]. WebGPU
/// device loss is handled separately, through wgpu's device-lost callback.
pub(super) fn install_webgl_context_lost(canvas: &web_sys::HtmlCanvasElement) {
    listen(canvas.as_ref(), "webglcontextlost", |_| {
        WEBGL_CONTEXT_LOST.with(|c| c.set(true));
    });
}

/// Whether the WebGL2 context was lost since the last call.
pub(super) fn take_webgl_context_lost() -> bool {
    WEBGL_CONTEXT_LOST.with(|c| c.replace(false))
}
