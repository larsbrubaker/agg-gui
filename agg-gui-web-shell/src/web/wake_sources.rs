//! Browser events that exist here only to wake the sleeping frame loop
//! ([`super::schedule`]): things the tick used to notice by polling every
//! animation frame and that reach no other listener.
//!
//! - the canvas's own size (a `ResizeObserver`): page layout can resize the
//!   canvas without any window `resize`;
//! - the device pixel ratio (a `matchMedia` resolution query, re-armed at
//!   each change): moving the window to another monitor changes it without a
//!   resize;
//! - `fullscreenchange`: Esc leaves fullscreen without telling the app;
//! - `gamepadconnected`: browsers expose a pad only once a button is pressed,
//!   and from then on the loop polls it every frame.
//!
//! Each marks the shell dirty, which wakes the loop; the tick then re-reads
//! the real state as it always has.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use super::input::add_listener;
use super::mark_dirty;

/// Install every wake source. Call once at startup.
pub(super) fn install(canvas: &web_sys::HtmlCanvasElement) {
    observe_canvas_size(canvas);
    watch_device_pixel_ratio();
    let Some(window) = web_sys::window() else {
        return;
    };
    if let Some(document) = window.document() {
        add_listener(
            document.as_ref(),
            "fullscreenchange",
            |_: web_sys::Event| mark_dirty(),
        );
    }
    add_listener(window.as_ref(), "gamepadconnected", |_: web_sys::Event| {
        mark_dirty()
    });
}

fn observe_canvas_size(canvas: &web_sys::HtmlCanvasElement) {
    let cb = Closure::<dyn FnMut()>::new(mark_dirty);
    match web_sys::ResizeObserver::new(cb.as_ref().unchecked_ref()) {
        Ok(observer) => {
            observer.observe(canvas);
            // Both live as long as the page.
            cb.forget();
            std::mem::forget(observer);
        }
        Err(e) => web_sys::console::warn_2(
            &"agg-gui-web-shell: no ResizeObserver; a canvas resized by page layout repaints \
              on the next input"
                .into(),
            &e,
        ),
    }
}

/// Wake on the next change of `devicePixelRatio`: a query for the current
/// resolution stops matching when it changes, and is then replaced by a
/// query for the new one.
fn watch_device_pixel_ratio() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let query = format!("(resolution: {}dppx)", window.device_pixel_ratio());
    let Ok(Some(list)) = window.match_media(&query) else {
        return;
    };
    let watched = list.clone();
    let on_change = Closure::once_into_js(move || {
        // The old query would fire again if the ratio came back to it.
        watched.set_onchange(None);
        mark_dirty();
        watch_device_pixel_ratio();
    });
    list.set_onchange(Some(on_change.unchecked_ref()));
}
