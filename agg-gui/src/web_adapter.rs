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
//! A canvas embedded in a larger page uses
//! [`install_keyboard_listeners_while`] instead, so the keyboard and
//! clipboard are the app's only while it has focus and the page's otherwise.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::cursor::CursorIcon;
use crate::event::{Key, Modifiers};

/// Attribute that marks the web shell's hidden input element (the one the
/// browser's input method composes into while a canvas text widget has
/// focus). Keyboard and clipboard listeners treat events from it as the app's
/// own, not as a DOM editor's.
pub const IME_INPUT_ATTRIBUTE: &str = "data-agg-gui-ime";

/// Parse a DOM `KeyboardEvent.key` string into this crate's [`Key`].
///
/// Named keys (`"Enter"`, `"ArrowLeft"`, …) map to their enum variant.
/// A single character becomes `Key::Char(c)`, `"F1"` … `"F24"` become
/// `Key::F(n)`.  Anything else — e.g. `"MediaPlayPause"` — round-trips as
/// `Key::Other(name)` so hosts can still inspect it.
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
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "Tab" => Key::Tab,
        "Enter" => Key::Enter,
        "Escape" => Key::Escape,
        " " => Key::Char(' '),
        s if s.chars().count() == 1 => Key::Char(s.chars().next()?),
        s if Key::function_key_from_name(s).is_some() => Key::function_key_from_name(s)?,
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
pub(crate) fn targets_dom_editor(event: &web_sys::Event) -> bool {
    let Some(target) = event.target() else {
        return false;
    };
    if let Some(el) = target.dyn_ref::<web_sys::Element>() {
        // The shell's own hidden IME input holds focus while a canvas text
        // widget is being edited; its keys belong to the app.
        if el.has_attribute(IME_INPUT_ATTRIBUTE) {
            return false;
        }
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

/// The physical key of `e` (`code`), which a keydown and its keyup share
/// even when a modifier changes the `key` text between them; the `key` text
/// where the browser reports no code.
fn physical_key(e: &web_sys::KeyboardEvent) -> String {
    let code = e.code();
    if code.is_empty() {
        e.key()
    } else {
        code
    }
}

fn modifiers_of(e: &web_sys::KeyboardEvent) -> Modifiers {
    Modifiers {
        shift: e.shift_key(),
        ctrl: e.ctrl_key(),
        alt: e.alt_key(),
        meta: e.meta_key(),
    }
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
///   pasting so the system clipboard is captured synchronously; it stores
///   the text and any picture in [`crate::wasm_clipboard`] and then
///   synthesizes the `Ctrl+V` through `on_key` itself (after the picture
///   has decoded, when there is one).
/// - Bare `Ctrl/Cmd+C` and `Ctrl/Cmd+X` keydowns are forwarded and never
///   `preventDefault()`ed: the browser's copy or cut command then fires
///   `copy` / `cut`, which publish the [`crate::wasm_clipboard`] buffer
///   (already written by the widget's Ctrl+C/X handler) to the system
///   clipboard. When the handler copied nothing, the event is left to the
///   browser, so the system clipboard keeps what it held.
/// - Typing/navigation keys are `preventDefault()`ed so space / arrows /
///   quote don't scroll the page or trigger browser quick-find, but
///   browser chrome stays reachable: `Tab`, the F-keys other than F1
///   (the app's help key), and modified shortcuts other than the app's
///   clipboard/undo set (Ctrl/Cmd A, Z, Y and the Shift variants of C
///   and X) and the keys the app claimed with
///   [`crate::claim_web_shortcut`] keep their default action. The policy
///   lives in `web_key_policy.rs`.
///
/// Listeners live for the page lifetime (the closures are leaked — call
/// this once at startup).
pub fn install_keyboard_listeners(on_key: impl FnMut(Key, Modifiers, bool) + 'static) {
    install_keyboard_listeners_while(|| true, on_key);
}

/// The test a scoped keyboard bridge asks on every event: does the keyboard
/// belong to the app right now?
pub(crate) type Belongs = Rc<dyn Fn() -> bool>;

/// [`install_keyboard_listeners`] for a canvas that shares its page: every
/// key and clipboard event is first checked with `belongs()`, and while it
/// returns `false` the event is the page's — not forwarded and never
/// `preventDefault()`ed, so the page scrolls, finds and copies as usual. A
/// shell embedding its canvas passes "the canvas (or its IME input) has
/// focus".
pub fn install_keyboard_listeners_while(
    belongs: impl Fn() -> bool + 'static,
    on_key: impl FnMut(Key, Modifiers, bool) + 'static,
) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let belongs: Belongs = Rc::new(belongs);
    let on_key: crate::web_paste::OnKey = Rc::new(RefCell::new(on_key));
    let held = Rc::new(RefCell::new(crate::web_key_policy::HeldKeys::default()));
    // Whether the latest keydown was a copy or cut chord that copied
    // nothing (`false` after any other keydown): its copy / cut event then
    // must not publish an older copy over what the system clipboard holds.
    let chord_copied_nothing = Rc::new(Cell::new(false));

    // --- keydown -----------------------------------------------------------
    let down_cb = {
        let on_key = Rc::clone(&on_key);
        let belongs = Rc::clone(&belongs);
        let held = Rc::clone(&held);
        let chord_copied_nothing = Rc::clone(&chord_copied_nothing);
        Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
            if !belongs() {
                return;
            }
            chord_copied_nothing.set(false);
            // Mid-composition keys (`isComposing`, or keyCode 229 "Process" on
            // the browsers that report the key before the composition) belong
            // to the input method; its result arrives as composition events.
            if targets_dom_editor(&e) || e.is_composing() || e.key_code() == 229 {
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
            if crate::web_key_policy::should_prevent_default(&k, mods) {
                e.prevent_default();
            }
            held.borrow_mut().pressed(&physical_key(&e));
            let copies = crate::web_key_policy::runs_browser_clipboard_command(&k, mods);
            let before = crate::wasm_clipboard::generation();
            (on_key.borrow_mut())(k, mods, true);
            if copies {
                chord_copied_nothing.set(crate::wasm_clipboard::generation() == before);
            }
        })
    };
    let _ = window.add_event_listener_with_callback("keydown", down_cb.as_ref().unchecked_ref());
    down_cb.forget();

    // --- keyup -------------------------------------------------------------
    let up_cb = {
        let on_key = Rc::clone(&on_key);
        let belongs = Rc::clone(&belongs);
        let held = Rc::clone(&held);
        Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
            // A key the app saw go down is released in the app wherever focus
            // went meanwhile; any other keyup follows the keydown rules.
            let belongs_now = belongs() && !targets_dom_editor(&e);
            if !held.borrow_mut().released(&physical_key(&e), belongs_now) {
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
    // event fires; publish it to the system clipboard. Setting the event's
    // data and cancelling it makes the browser copy that data instead of
    // the page's selection.
    for event_name in ["copy", "cut"] {
        let belongs = Rc::clone(&belongs);
        let chord_copied_nothing = Rc::clone(&chord_copied_nothing);
        let cb = Closure::<dyn FnMut(web_sys::ClipboardEvent)>::new(
            move |e: web_sys::ClipboardEvent| {
                // The page's own selection is copied while the app is not
                // the keyboard's owner.
                if !belongs() {
                    return;
                }
                // A copy chord with nothing selected copies nothing.
                if chord_copied_nothing.replace(false) {
                    return;
                }
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
    // Capture the system clipboard's text (and picture, decoded off the
    // frame) into the in-process buffers, then synthesize Ctrl+V so the
    // focused widget's paste handler runs against them (`web_paste`).
    crate::web_paste::install(&window, on_key, belongs);
}
