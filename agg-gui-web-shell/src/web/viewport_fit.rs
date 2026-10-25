//! Keeps a full-window app's canvas inside the visible viewport.
//!
//! Android Chrome measures `100vh` with its address bar hidden, so a page that
//! sizes its canvas (or the canvas's parent) with it lays the canvas out taller
//! than the window while the bar shows, and the bottom of the app, where the
//! on-screen keyboard sits, is off screen. Each tick, before the backing store is
//! sized ([`super::frame::size_backing_store`]), this writes the canvas's inline
//! `max-height` as [`crate::dom_math::visible_max_height`] of the canvas's top and
//! `window.innerHeight`. The canvas's CSS box, the backing store and the app's
//! logical height are then the part the user can see. The bar showing or hiding
//! fires a window `resize` ([`super::lifecycle::install_resize`]), and the
//! tick it wakes moves the cap.
//!
//! `innerHeight` rather than `visualViewport.height`: the visual viewport also
//! shrinks under a pinch zoom or a system keyboard, which must not lay the app
//! out again; the layout viewport changes with the address bar only.
//!
//! A `max-height` only takes away the part of the canvas below the window's
//! bottom, so a page that lays its canvas out to fit is unaffected. The page and
//! its scrolling are left alone. A canvas embedded in a larger page
//! ([`crate::WebShellConfig::keyboard_needs_focus`]) is never capped: there,
//! the page scrolls to show the rest of it.

use std::cell::Cell;

use crate::dom_math::visible_max_height;

thread_local! {
    /// Cap the canvas (a full-window app). Set once by [`super::start`].
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    /// The `max-height` last written, in CSS px; `None` when none is set.
    static WRITTEN: Cell<Option<f64>> = const { Cell::new(None) };
}

/// Whether [`fit`] caps the canvas. Call before the first [`fit`].
pub(super) fn set_enabled(on: bool) {
    ENABLED.with(|e| e.set(on));
}

/// Write the canvas's `max-height` so its bottom edge is the visible
/// viewport's bottom, or remove the one written when there is no usable
/// reading. No-op when disabled or when nothing changed.
pub(super) fn fit(canvas: &web_sys::HtmlCanvasElement) {
    if !ENABLED.with(|e| e.get()) {
        return;
    }
    let viewport_height = web_sys::window()
        .and_then(|w| w.inner_height().ok())
        .and_then(|h| h.as_f64())
        .unwrap_or(0.0);
    let top = canvas.get_bounding_client_rect().top();
    let cap = visible_max_height(top, viewport_height);
    let written = WRITTEN.with(|w| w.get());
    let unchanged = match (cap, written) {
        (Some(cap), Some(written)) => (cap - written).abs() < 0.01,
        (None, None) => true,
        _ => false,
    };
    if unchanged {
        return;
    }
    let style = canvas.style();
    let result = match cap {
        Some(cap) => style.set_property("max-height", &format!("{cap}px")),
        None => style.remove_property("max-height").map(|_| ()),
    };
    if result.is_ok() {
        WRITTEN.with(|w| w.set(cap));
    }
}

/// In a real browser (`wasm-pack test --headless --chrome agg-gui-web-shell`):
/// a canvas laid out taller than the window, as a `100vh` page is on Android
/// Chrome with the address bar showing, ends at the window's bottom once
/// [`fit`] has run, and the backing store sized from it is what shows; one
/// laid out to fit is left alone. (How the cap follows the window's height is
/// covered natively, in `dom_math`.)
#[cfg(test)]
mod tests {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::*;

    use super::*;
    use crate::dom_math::fit_backing;

    wasm_bindgen_test_configure!(run_in_browser);

    /// Android Chrome's address bar, in CSS px.
    const BAR: f64 = 56.0;

    #[wasm_bindgen_test]
    fn a_canvas_taller_than_the_window_is_held_to_its_bottom() {
        let window = web_sys::window().expect("window");
        let document = window.document().expect("document");
        let inner = window
            .inner_height()
            .ok()
            .and_then(|h| h.as_f64())
            .expect("innerHeight");
        let wrap = document
            .create_element("div")
            .expect("div")
            .dyn_into::<web_sys::HtmlElement>()
            .expect("HtmlElement");
        let style = wrap.style();
        for (name, value) in [
            ("position", "fixed"),
            ("left", "0px"),
            ("top", "0px"),
            ("width", "300px"),
            ("display", "flex"),
        ] {
            style.set_property(name, value).expect("style");
        }
        style
            .set_property("height", &format!("{}px", inner + BAR))
            .expect("height");
        let canvas = document
            .create_element("canvas")
            .expect("canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("HtmlCanvasElement");
        canvas
            .style()
            .set_property("display", "block")
            .expect("style");
        canvas.style().set_property("width", "100%").expect("style");
        canvas
            .style()
            .set_property("height", "100%")
            .expect("style");
        wrap.append_child(&canvas).expect("append canvas");
        document
            .body()
            .expect("body")
            .append_child(&wrap)
            .expect("append wrap");

        // As laid out, the bottom of the canvas is under the window's edge.
        let before = canvas.get_bounding_client_rect();
        assert!(
            before.bottom() > inner + BAR - 1.0,
            "bottom {}",
            before.bottom()
        );

        set_enabled(true);
        fit(&canvas);
        let after = canvas.get_bounding_client_rect();
        assert!(
            (after.bottom() - inner).abs() < 1.0,
            "bottom {} vs {inner}",
            after.bottom()
        );
        let ((_, h), scale) = fit_backing(300.0, canvas.client_height() as f64, 2.0, 8192);
        assert!(
            (h as f64 / scale - inner).abs() <= 1.0,
            "app height {h}/{scale}"
        );

        // A canvas laid out to fit the window keeps the size the page gives it.
        let fits = inner - 100.0;
        style
            .set_property("height", &format!("{fits}px"))
            .expect("height");
        fit(&canvas);
        let kept = canvas.get_bounding_client_rect();
        assert!(
            (kept.height() - fits).abs() < 1.0,
            "height {}",
            kept.height()
        );

        set_enabled(false);
        wrap.remove();
    }
}
