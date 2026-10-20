//! WASM-specific in-process clipboard buffer.
//!
//! Because `arboard` (the native clipboard crate) does not work in a browser
//! context, WASM clipboard operations use thread-local buffers (text, HTML
//! and a pasted picture) as the in-process clipboard.
//! `web_adapter::install_keyboard_listeners` (or the JS harness in
//! `demo-wasm`) bridges them to the browser's system clipboard:
//!
//! * **Copy / Cut**: `clipboard::set_text` / `set_rich_text` write the copy
//!   here and also hand it to the browser's asynchronous Clipboard API
//!   ([`write_system`]), so a copy made from a menu or button reaches the
//!   system clipboard too. A Ctrl/Cmd+C or Ctrl/Cmd+X keydown then runs the
//!   browser's copy or cut command, whose `copy`/`cut` DOM event handler reads
//!   the buffer (`get`, or `wasm_clipboard_get()` in the JS harness) and places
//!   it in `event.clipboardData`. Both carry the same text, so whichever lands
//!   last leaves the same clipboard.
//!
//! * **Paste**: the JS `paste` DOM event handler reads the system clipboard text
//!   from `event.clipboardData` and writes it here via `wasm_clipboard_set()`;
//!   it then synthesises a Ctrl+V key event so Rust's paste handler picks it up.
//!   A pasted picture is decoded by `web_paste` and stored with the text
//!   through [`set_paste`] before that Ctrl+V, so the paste handler reads it
//!   through `clipboard::get_image_rgba`.
//!
//! This module is compiled only when `target_arch = "wasm32"`.

use std::cell::{Cell, RefCell};

use wasm_bindgen::{JsCast, JsValue};
use web_sys::js_sys::{Array, Object, Reflect};

use crate::clipboard::ClipboardImage;

thread_local! {
    static BUFFER: RefCell<String> = const { RefCell::new(String::new()) };
    static HTML_BUFFER: RefCell<String> = const { RefCell::new(String::new()) };
    static IMAGE_BUFFER: RefCell<Option<ClipboardImage>> = const { RefCell::new(None) };
    /// Counts the app's copies ([`set`], [`set_rich`]), so the keyboard bridge
    /// can tell whether a copy chord copied anything.
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// How many copies the app has made so far; changes on every [`set`] and
/// [`set_rich`] (a paste does not count).
pub(crate) fn generation() -> u64 {
    GENERATION.with(Cell::get)
}

fn note_copy() {
    GENERATION.with(|g| g.set(g.get().wrapping_add(1)));
}

/// Read the current clipboard buffer.  Returns `None` when the buffer is empty.
pub fn get() -> Option<String> {
    BUFFER.with(|b| {
        let s = b.borrow();
        if s.is_empty() {
            None
        } else {
            Some(s.clone())
        }
    })
}

/// Read the current HTML clipboard buffer. Returns `None` when empty.
pub fn get_html() -> Option<String> {
    HTML_BUFFER.with(|b| {
        let s = b.borrow();
        if s.is_empty() {
            None
        } else {
            Some(s.clone())
        }
    })
}

/// The picture of the latest paste, `None` when it carried none or text has
/// been copied since.
pub fn get_image() -> Option<ClipboardImage> {
    IMAGE_BUFFER.with(|b| b.borrow().clone())
}

/// Whether the latest paste left a picture, without copying it.
pub fn has_image() -> bool {
    IMAGE_BUFFER.with(|b| b.borrow().is_some())
}

/// Overwrite the clipboard buffer with `text` (dropping any HTML and
/// picture).
pub fn set(text: &str) {
    note_copy();
    BUFFER.with(|b| *b.borrow_mut() = text.to_string());
    HTML_BUFFER.with(|b| b.borrow_mut().clear());
    IMAGE_BUFFER.with(|b| *b.borrow_mut() = None);
}

/// Overwrite the clipboard buffers with plain text and rendered HTML
/// (dropping any picture).
pub fn set_rich(text: &str, html: &str) {
    note_copy();
    BUFFER.with(|b| *b.borrow_mut() = text.to_string());
    HTML_BUFFER.with(|b| *b.borrow_mut() = html.to_string());
    IMAGE_BUFFER.with(|b| *b.borrow_mut() = None);
}

/// Overwrite the clipboard buffers with what a browser paste carried: its
/// plain text (empty when none) and its decoded picture.
pub fn set_paste(text: &str, image: Option<ClipboardImage>) {
    BUFFER.with(|b| *b.borrow_mut() = text.to_string());
    HTML_BUFFER.with(|b| b.borrow_mut().clear());
    IMAGE_BUFFER.with(|b| *b.borrow_mut() = image);
}

/// Hand a copy to the system clipboard through the browser's asynchronous
/// Clipboard API: `text` alone, or `text` with its rendered `html` where the
/// browser can write both. Best effort: the browser may refuse (no secure
/// context, the page not focused, no user gesture where one is required), and
/// a refusal is dropped quietly; the `copy` / `cut` event of a keyboard copy
/// still carries the copy then.
pub(crate) fn write_system(text: &str, html: Option<&str>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    // `navigator.clipboard` is absent outside a secure context (a page served
    // over plain http); calling through it would throw.
    let Ok(clipboard) = Reflect::get(&window.navigator(), &JsValue::from_str("clipboard")) else {
        return;
    };
    if clipboard.is_undefined() || clipboard.is_null() {
        return;
    }
    let clipboard: web_sys::Clipboard = clipboard.unchecked_into();
    let promise = match html.and_then(|html| rich_item(text, html)) {
        Some(item) => {
            let items = Array::new();
            items.push(&item);
            clipboard.write(items.as_ref())
        }
        None => clipboard.write_text(text),
    };
    // Await the write only to consume a refusal, so it never shows up as an
    // unhandled promise rejection.
    wasm_bindgen_futures::spawn_local(async move {
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    });
}

/// A `ClipboardItem` holding `text` as text/plain and `html` as text/html;
/// `None` where the browser has no `ClipboardItem` (the copy is then written
/// as plain text).
fn rich_item(text: &str, html: &str) -> Option<JsValue> {
    let record = Object::new();
    for (mime, body) in [("text/plain", text), ("text/html", html)] {
        let parts = Array::new();
        parts.push(&JsValue::from_str(body));
        let options = web_sys::BlobPropertyBag::new();
        options.set_type(mime);
        let blob =
            web_sys::Blob::new_with_str_sequence_and_options(parts.as_ref(), &options).ok()?;
        Reflect::set(&record, &JsValue::from_str(mime), &blob).ok()?;
    }
    web_sys::ClipboardItem::new_with_record_from_str_to_blob_promise(&record)
        .ok()
        .map(JsValue::from)
}
