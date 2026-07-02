//! Bridge between web/JS event payloads and this crate's input types.
//!
//! Compiled only on `wasm32` targets.  Host crates forward DOM
//! KeyboardEvent payloads through [`key`] and use [`cursor_style`]
//! to set the canvas's cursor from a [`CursorIcon`].
//!
//! Shells that don't want to hand-roll the DOM plumbing can instead call
//! [`install_keyboard_listeners`] once at startup: it registers
//! window-level `keydown` / `keyup` plus the `copy` / `cut` / `paste`
//! clipboard bridge, so physical-keyboard typing and clipboard shortcuts
//! work in every agg-gui web app without per-app JS. (Mobile typing is
//! separate — the in-canvas on-screen keyboard synthesizes keys itself.)

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::cursor::CursorIcon;
use crate::event::{Key, Modifiers};

/// Parse a DOM `KeyboardEvent.key` string into this crate's [`Key`].
///
/// Named keys (`"Enter"`, `"ArrowLeft"`, …) map to their enum variant.
/// A single character becomes `Key::Char(c)`.  Anything else — e.g.
/// `"F5"`, `"MediaPlayPause"` — round-trips as `Key::Other(name)` so
/// hosts can still inspect it.
pub fn key(name: &str) -> Option<Key> {
    Some(match name {
        "Backspace" => Key::Backspace,
        "Delete" => Key::Delete,
        "Insert" => Key::Insert,
        "ArrowLeft" => Key::ArrowLeft,
        "ArrowRight" => Key::ArrowRight,
        "ArrowUp" => Key::ArrowUp,
        "ArrowDown" => Key::ArrowDown,
        "Home" => Key::Home,
        "End" => Key::End,
        "Tab" => Key::Tab,
        "Enter" => Key::Enter,
        "Escape" => Key::Escape,
        " " => Key::Char(' '),
        s if s.chars().count() == 1 => Key::Char(s.chars().next()?),
        s => Key::Other(s.to_string()),
    })
}

/// Produce the CSS `style` attribute value for applying a [`CursorIcon`]
/// to a DOM element — `"cursor:<name>"`.  Callers combine this with their
/// existing style string if they set other properties.
pub fn cursor_style(icon: CursorIcon) -> String {
    format!("cursor:{}", icon.to_css())
}

/// `true` when a DOM event targets a real HTML editing element (an
/// `<input>`, `<textarea>`, `<select>`, or a `contenteditable` node).
/// Keys typed into those must stay with the browser — an agg-gui canvas
/// app usually shares the page with at least a hidden mobile text input,
/// and may sit inside a larger host page with its own form controls.
fn targets_dom_editor(event: &web_sys::Event) -> bool {
    let Some(target) = event.target() else {
        return false;
    };
    if let Some(el) = target.dyn_ref::<web_sys::Element>() {
        let tag = el.tag_name();
        if tag.eq_ignore_ascii_case("input")
            || tag.eq_ignore_ascii_case("textarea")
            || tag.eq_ignore_ascii_case("select")
        {
            return true;
        }
    }
    if let Some(html) = target.dyn_ref::<web_sys::HtmlElement>() {
        if html.is_content_editable() {
            return true;
        }
    }
    false
}

fn modifiers_of(e: &web_sys::KeyboardEvent) -> Modifiers {
    Modifiers {
        shift: e.shift_key(),
        ctrl: e.ctrl_key(),
        alt: e.alt_key(),
        meta: e.meta_key(),
    }
}

/// Whether a forwarded keydown should suppress the browser's default
/// action.  The listeners sit at window level, so this must block only
/// page side effects of in-app typing (space scrolls, arrows scroll,
/// `'` / `/` open Firefox quick-find, Backspace navigates back) while
/// leaving browser chrome usable: `Tab` focus-navigation, F-keys
/// (`Key::Other`), and modified shortcuts like Ctrl+R / Cmd+L stay with
/// the browser.  Ctrl/Cmd C, X, A, Z, Y are the app's clipboard/undo
/// set and are claimed.  Alt combos (Alt+Left = history back, AltGr
/// chars report ctrl+alt) are never suppressed.
fn should_prevent_default(k: &Key, mods: Modifiers) -> bool {
    if mods.ctrl || mods.meta {
        return matches!(k, Key::Char(c)
            if matches!(c.to_ascii_lowercase(), 'c' | 'x' | 'a' | 'z' | 'y'))
            && !mods.alt;
    }
    if mods.alt {
        return false;
    }
    matches!(
        k,
        Key::Char(_)
            | Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Backspace
            | Key::Delete
            | Key::Home
            | Key::End
            | Key::Enter
    )
}

/// Install window-level keyboard + clipboard listeners that feed an
/// agg-gui [`App`](crate::App) hosted in a `<canvas>`.
///
/// `on_key(key, modifiers, pressed)` is invoked for every key event that
/// belongs to the canvas app (`pressed` = `true` for `keydown`, `false`
/// for `keyup`); shells typically forward straight into
/// [`App::on_key_down`](crate::App::on_key_down) /
/// [`App::on_key_up`](crate::App::on_key_up) and request a redraw.
///
/// Behaviour mirrors the reference JS harness in `demo/src/app.ts`:
/// - Events targeting real DOM editors (`<input>`, `<textarea>`,
///   `contenteditable`) are left for the browser.
/// - `Ctrl/Cmd+V` keydowns are NOT forwarded — the `paste` listener owns
///   pasting so the system clipboard text is captured synchronously; it
///   stores the text in [`crate::wasm_clipboard`] and then synthesizes
///   the `Ctrl+V` through `on_key` itself.
/// - `copy` / `cut` publish the [`crate::wasm_clipboard`] buffer (already
///   written by the widget's Ctrl+C/X handler) to the system clipboard.
/// - Typing/navigation keys are `preventDefault()`ed so space / arrows /
///   quote don't scroll the page or trigger browser quick-find, but
///   browser chrome stays reachable: `Tab`, F-keys, and modified
///   shortcuts other than the app's clipboard/undo set (Ctrl/Cmd
///   C, X, A, Z, Y) keep their default action.
///
/// Listeners live for the page lifetime (the closures are leaked — call
/// this once at startup).
pub fn install_keyboard_listeners(on_key: impl FnMut(Key, Modifiers, bool) + 'static) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let on_key: Rc<RefCell<dyn FnMut(Key, Modifiers, bool)>> = Rc::new(RefCell::new(on_key));

    // --- keydown -----------------------------------------------------------
    let down_cb = {
        let on_key = Rc::clone(&on_key);
        Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
            if targets_dom_editor(&e) || e.is_composing() {
                return;
            }
            let mods = modifiers_of(&e);
            let name = e.key();
            // Paste is owned by the `paste` listener below (synchronous
            // clipboard access); letting the keydown through as well
            // would double-fire the widget paste handler.
            if (mods.ctrl || mods.meta) && name.eq_ignore_ascii_case("v") {
                return;
            }
            let Some(k) = key(&name) else {
                return;
            };
            if should_prevent_default(&k, mods) {
                e.prevent_default();
            }
            (on_key.borrow_mut())(k, mods, true);
        })
    };
    let _ = window
        .add_event_listener_with_callback("keydown", down_cb.as_ref().unchecked_ref());
    down_cb.forget();

    // --- keyup -------------------------------------------------------------
    let up_cb = {
        let on_key = Rc::clone(&on_key);
        Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
            if targets_dom_editor(&e) {
                return;
            }
            if let Some(k) = key(&e.key()) {
                (on_key.borrow_mut())(k, modifiers_of(&e), false);
            }
        })
    };
    let _ = window.add_event_listener_with_callback("keyup", up_cb.as_ref().unchecked_ref());
    up_cb.forget();

    // --- copy / cut ----------------------------------------------------------
    // The focused widget's Ctrl+C / Ctrl+X handler has already written the
    // selection into the in-process buffer by the time the DOM clipboard
    // event fires; publish it to the system clipboard.
    for event_name in ["copy", "cut"] {
        let cb = Closure::<dyn FnMut(web_sys::ClipboardEvent)>::new(
            move |e: web_sys::ClipboardEvent| {
                let Some(text) = crate::wasm_clipboard::get() else {
                    return;
                };
                let Some(data) = e.clipboard_data() else {
                    return;
                };
                let _ = data.set_data("text/plain", &text);
                if let Some(html) = crate::wasm_clipboard::get_html() {
                    let _ = data.set_data("text/html", &html);
                }
                e.prevent_default();
            },
        );
        let _ = window.add_event_listener_with_callback(event_name, cb.as_ref().unchecked_ref());
        cb.forget();
    }

    // --- paste ---------------------------------------------------------------
    // Capture the system clipboard text synchronously, stash it in the
    // in-process buffer, then synthesize Ctrl+V so the focused widget's
    // paste handler runs against the fresh buffer.
    let paste_cb = {
        let on_key = Rc::clone(&on_key);
        Closure::<dyn FnMut(web_sys::ClipboardEvent)>::new(move |e: web_sys::ClipboardEvent| {
            if targets_dom_editor(&e) {
                return;
            }
            let Some(data) = e.clipboard_data() else {
                return;
            };
            let Ok(text) = data.get_data("text/plain") else {
                return;
            };
            if text.is_empty() {
                return;
            }
            e.prevent_default();
            crate::wasm_clipboard::set(&text);
            let mods = Modifiers {
                ctrl: true,
                ..Modifiers::default()
            };
            (on_key.borrow_mut())(Key::Char('v'), mods, true);
        })
    };
    let _ = window.add_event_listener_with_callback("paste", paste_cb.as_ref().unchecked_ref());
    paste_cb.forget();
}
