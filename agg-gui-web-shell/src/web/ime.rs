//! Browser input methods for canvas text widgets: a hidden `<input>` that
//! holds DOM focus while an agg-gui text widget is being edited, so the
//! browser can compose into it (CJK, dead keys, mobile predictive text), and
//! the `composition*` / `input` listeners that turn what it produces into
//! `ForwarderEvent::ImePreedit` and `ForwarderEvent::TextInput`.
//!
//! Plain typing is unchanged: the window-level keyboard bridge still turns
//! `keydown` into `Key::Char` (and `preventDefault`s it, so the input never
//! receives the character). Only text that did *not* come through a key press
//! is committed from here: finished compositions, dictation, an emoji picker.
//! The input sits at the caret (`App::ime_cursor_area`), kept inside the
//! window's visible area, so the browser's candidate window opens beside the
//! text being composed and never scrolls the page to show the input; it is
//! focused with `preventScroll`, and `inputmode=none`
//! keeps phones from raising the system keyboard over agg-gui's own.
//!
//! [`sync`] runs on every tick of the frame loop: it focuses the input while the app
//! wants IME (`App::wants_ime`), moves it with the caret, and blurs it when
//! focus leaves the text widget.

use std::cell::{Cell, RefCell};

use agg_gui::shell_input::ForwarderEvent;
use wasm_bindgen::JsCast;

use super::input::add_listener;
use super::{forward, frame::input_scale, note_input, with_app, with_canvas};

thread_local! {
    static INPUT: RefCell<Option<web_sys::HtmlInputElement>> = const { RefCell::new(None) };
    /// The input holds DOM focus on the app's behalf.
    static HELD: Cell<bool> = const { Cell::new(false) };
    /// The last placement written, in CSS px (x, y, height), to skip no-op writes.
    static PLACED: Cell<Option<(f64, f64, f64)>> = const { Cell::new(None) };
    /// A character keydown went to the app and not yet released: the `input`
    /// event beside it (when the browser still fires one) is the same text.
    static KEY_CHAR_DOWN: Cell<bool> = const { Cell::new(false) };
}

/// Note that a key went down (`pressed`) or up and whether it typed a
/// character, so [`install`]'s `input` listener does not insert it twice.
pub(super) fn note_key(typed_character: bool, pressed: bool) {
    if pressed {
        if typed_character {
            KEY_CHAR_DOWN.with(|k| k.set(true));
        }
    } else {
        KEY_CHAR_DOWN.with(|k| k.set(false));
    }
}

/// A press on the canvas: take DOM focus back to the input on the next
/// [`sync`], even if focus went to another element meanwhile.
pub(super) fn note_canvas_press() {
    HELD.with(|h| h.set(false));
}

/// Create the hidden input and its listeners. Call once at startup.
pub(super) fn install() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(body) = document.body() else {
        return;
    };
    let Ok(element) = document.create_element("input") else {
        return;
    };
    let Ok(input) = element.dyn_into::<web_sys::HtmlInputElement>() else {
        return;
    };
    let _ = input.set_attribute(agg_gui::web_adapter::IME_INPUT_ATTRIBUTE, "");
    for (name, value) in [
        ("type", "text"),
        ("inputmode", "none"),
        ("autocomplete", "off"),
        ("autocapitalize", "off"),
        ("autocorrect", "off"),
        ("spellcheck", "false"),
        ("aria-hidden", "true"),
        ("tabindex", "-1"),
    ] {
        let _ = input.set_attribute(name, value);
    }
    let style = input.style();
    // Invisible and out of the way, but not display:none (that cannot focus).
    // 16px keeps iOS from zooming the page on focus.
    for (name, value) in [
        ("position", "fixed"),
        ("left", "0px"),
        ("top", "0px"),
        ("width", "2px"),
        ("height", "16px"),
        ("opacity", "0"),
        ("border", "0"),
        ("padding", "0"),
        ("margin", "0"),
        ("outline", "none"),
        ("pointer-events", "none"),
        ("font-size", "16px"),
        ("z-index", "-1"),
    ] {
        let _ = style.set_property(name, value);
    }
    if body.append_child(&input).is_err() {
        return;
    }
    let target: &web_sys::EventTarget = input.as_ref();

    add_listener(
        target,
        "compositionupdate",
        |e: web_sys::CompositionEvent| {
            let text = e.data().unwrap_or_default();
            // The DOM reports no caret inside a composition: put it at the end.
            let end = text.len();
            forward(ForwarderEvent::ImePreedit {
                text,
                cursor: Some((end, end)),
            });
            note_input();
        },
    );
    {
        let input = input.clone();
        add_listener(
            target,
            "compositionend",
            move |e: web_sys::CompositionEvent| {
                let text = e.data().unwrap_or_default();
                forward(ForwarderEvent::ImePreedit {
                    text: String::new(),
                    cursor: None,
                });
                if !text.is_empty() {
                    forward(ForwarderEvent::TextInput { text });
                }
                input.set_value("");
                note_input();
            },
        );
    }
    {
        let input = input.clone();
        add_listener(target, "input", move |e: web_sys::InputEvent| {
            let kind = e.input_type();
            // Composition results are committed by `compositionend`.
            let composing = e.is_composing() || kind.contains("omposition");
            let typed = KEY_CHAR_DOWN.with(|k| k.get());
            if !composing && !typed && kind == "insertText" {
                if let Some(text) = e.data().filter(|t| !t.is_empty()) {
                    forward(ForwarderEvent::TextInput { text });
                    note_input();
                }
            }
            if !composing {
                input.set_value("");
            }
        });
    }
    INPUT.with(|i| *i.borrow_mut() = Some(input));
}

/// Bring the input's DOM focus and position in step with the app: focused
/// and at the caret while a text widget is being edited, blurred otherwise.
pub(super) fn sync() {
    let Some((wants, area)) = with_app(|app| (app.wants_ime(), app.ime_cursor_area())) else {
        return;
    };
    INPUT.with(|cell| {
        let borrow = cell.borrow();
        let Some(input) = borrow.as_ref() else {
            return;
        };
        if !wants {
            if HELD.with(|h| h.replace(false)) {
                let _ = input.blur();
                input.set_value("");
                PLACED.with(|p| p.set(None));
            }
            return;
        }
        if let Some(area) = area {
            let scale = input_scale().max(1e-6);
            let origin = with_canvas(|c| {
                let r = c.get_bounding_client_rect();
                (r.left(), r.top())
            })
            .unwrap_or((0.0, 0.0));
            let height = (area.height / scale).max(1.0);
            // Kept inside the window's visible area: the browser scrolls the page to show a
            // focused text input outside it, and the caret of a field on a canvas taller than
            // the window, or one the page has scrolled, can be anywhere.
            let (view_w, view_h) = window_css_size();
            let placed = (
                (origin.0 + area.x / scale).clamp(0.0, (view_w - 2.0).max(0.0)),
                (origin.1 + area.y / scale).clamp(0.0, (view_h - height).max(0.0)),
                height,
            );
            if PLACED.with(|p| p.get()) != Some(placed) {
                PLACED.with(|p| p.set(Some(placed)));
                let style = input.style();
                let _ = style.set_property("left", &format!("{}px", placed.0));
                let _ = style.set_property("top", &format!("{}px", placed.1));
                let _ = style.set_property("height", &format!("{}px", placed.2));
            }
        }
        if !HELD.with(|h| h.replace(true)) {
            let options = web_sys::FocusOptions::new();
            options.set_prevent_scroll(true);
            let _ = input.focus_with_options(&options);
        }
    });
}

/// The window's layout viewport in CSS px (`innerWidth`, `innerHeight`).
fn window_css_size() -> (f64, f64) {
    let Some(window) = web_sys::window() else {
        return (0.0, 0.0);
    };
    let size = |v: Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>| {
        v.ok().and_then(|v| v.as_f64()).unwrap_or(0.0)
    };
    (size(window.inner_width()), size(window.inner_height()))
}
