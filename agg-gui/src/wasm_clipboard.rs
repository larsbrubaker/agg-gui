//! WASM-specific in-process clipboard buffer.
//!
//! Because `arboard` (the native clipboard crate) does not work in a browser
//! context, WASM clipboard operations use a thread-local
//! [`Contents`](crate::clipboard::Contents) (text, HTML and a picture, one
//! copy or paste at a time) as the in-process clipboard.
//! `web_adapter::install_keyboard_listeners` (or the JS harness in
//! `demo-wasm`) bridges it to the browser's system clipboard:
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
//! * **Copy a picture**: `clipboard::set_image_rgba` keeps the picture here
//!   ([`set_image`]), so this page's own paste gets it back, and writes it to
//!   the system clipboard as a PNG ([`write_system_png`]). No `copy` event
//!   can carry a picture, so that write is the only way it reaches the
//!   system clipboard.
//!
//! * **Paste**: the JS `paste` DOM event handler reads the system clipboard text
//!   from `event.clipboardData` and writes it here via `wasm_clipboard_set()`;
//!   it then synthesises a Ctrl+V key event so Rust's paste handler picks it up.
//!   `web_paste` does the same with the paste's text, its `text/html` and its
//!   decoded picture, stored together through [`set_paste_rich`] before that
//!   Ctrl+V, so the paste handler reads them through `clipboard::get_text`,
//!   `clipboard::get_html` and `clipboard::get_image_rgba`.
//!
//! * **Paste button**: the touch toolbar's Paste reads the system clipboard
//!   itself ([`read_system`], `navigator.clipboard.readText()`), since no
//!   `paste` event comes with a tap; `clipboard::system_paste` delivers it.
//!
//! This module is compiled only when `target_arch = "wasm32"`.

use std::cell::{Cell, RefCell};

use wasm_bindgen::{JsCast, JsValue};
use web_sys::js_sys::{Array, Object, Reflect, Uint8Array};

use crate::clipboard::{ClipboardImage, Contents};

thread_local! {
    static CONTENTS: RefCell<Contents> = RefCell::new(Contents::default());
    /// Counts the app's copies ([`set`], [`set_rich`], [`set_image`]), so the keyboard bridge
    /// can tell whether a copy chord copied anything.
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// How many copies the app has made so far; changes on every [`set`],
/// [`set_rich`] and [`set_image`] (a paste does not count).
pub(crate) fn generation() -> u64 {
    GENERATION.with(Cell::get)
}

fn note_copy() {
    GENERATION.with(|g| g.set(g.get().wrapping_add(1)));
}

/// A non-empty flavour, `None` for an absent or empty one.
fn non_empty(flavour: &Option<String>) -> Option<String> {
    flavour.as_ref().filter(|s| !s.is_empty()).cloned()
}

/// Read the current clipboard buffer.  Returns `None` when the buffer is empty.
pub fn get() -> Option<String> {
    CONTENTS.with(|c| non_empty(&c.borrow().text))
}

/// Read the current HTML clipboard buffer: the app's latest rich copy, or the
/// `text/html` of the latest paste when that came after it. Returns `None`
/// when empty.
pub fn get_html() -> Option<String> {
    CONTENTS.with(|c| non_empty(&c.borrow().html))
}

/// The picture of the latest paste, or of this page's own latest picture
/// copy when that came after it; `None` when there is none or text has been
/// copied since.
pub fn get_image() -> Option<ClipboardImage> {
    CONTENTS.with(|c| c.borrow().image.clone())
}

/// Whether the latest paste or copy left a picture, without copying it.
pub fn has_image() -> bool {
    CONTENTS.with(|c| c.borrow().image.is_some())
}

/// Overwrite the clipboard buffer with `text` (dropping any HTML and
/// picture).
pub fn set(text: &str) {
    note_copy();
    CONTENTS.with(|c| c.borrow_mut().put_text(text));
}

/// Overwrite the clipboard buffers with plain text and rendered HTML
/// (dropping any picture).
pub fn set_rich(text: &str, html: &str) {
    note_copy();
    CONTENTS.with(|c| c.borrow_mut().put_rich_text(text, html));
}

/// Overwrite the clipboard buffers with a picture the app copied (dropping
/// any text and HTML), as the native clipboard holds one copy at a time.
pub(crate) fn set_image(image: ClipboardImage) {
    note_copy();
    CONTENTS.with(|c| c.borrow_mut().put_image(image));
}

/// Overwrite the clipboard buffers with what a browser paste carried: its
/// plain text (empty when none) and its decoded picture, with no HTML.
pub fn set_paste(text: &str, image: Option<ClipboardImage>) {
    set_paste_rich(text, "", image);
}

/// Overwrite the clipboard buffers with what a browser paste carried: its
/// plain text, its `text/html` (each empty when none) and its decoded
/// picture. A paste without HTML clears the HTML of any earlier copy or
/// paste, so it can't be read along with this paste's text.
pub fn set_paste_rich(text: &str, html: &str, image: Option<ClipboardImage>) {
    CONTENTS.with(|c| c.borrow_mut().put_paste(text, html, image));
}

/// Hand a copy to the system clipboard through the browser's asynchronous
/// Clipboard API: `text` alone, or `text` with its rendered `html` where the
/// browser can write both. Best effort: the browser may refuse (no secure
/// context, the page not focused, no user gesture where one is required), and
/// a refusal is dropped quietly; the `copy` / `cut` event of a keyboard copy
/// still carries the copy then.
pub(crate) fn write_system(text: &str, html: Option<&str>) {
    let Some(clipboard) = system_clipboard() else {
        return;
    };
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

/// Hand a copied picture, encoded as `png`, to the system clipboard through
/// the browser's asynchronous Clipboard API. `true` when the write was
/// started; `false` where the browser has no Clipboard API (no secure
/// context) or no `ClipboardItem`. A refused write (permission denied, the
/// page not focused, a browser that takes no pictures) is logged to the
/// console, since the user sees nothing else go wrong.
///
/// Safari accepts a write only when its `ClipboardItem` is made inside the
/// user's gesture, so the item is built here, synchronously, around the
/// ready PNG; only the wait for the write is deferred.
pub(crate) fn write_system_png(png: &[u8]) -> bool {
    let Some(clipboard) = system_clipboard() else {
        return false;
    };
    let Some(item) = png_item(png) else {
        return false;
    };
    let items = Array::new();
    items.push(&item);
    let promise = clipboard.write(items.as_ref());
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(err) = wasm_bindgen_futures::JsFuture::from(promise).await {
            web_sys::console::error_2(
                &JsValue::from_str("agg-gui: the browser refused to copy the picture"),
                &err,
            );
        }
    });
    true
}

/// `navigator.clipboard`, `None` where the page has none: it is absent
/// outside a secure context (a page served over plain http), and calling
/// through it would throw.
fn system_clipboard() -> Option<web_sys::Clipboard> {
    let window = web_sys::window()?;
    let clipboard = Reflect::get(&window.navigator(), &JsValue::from_str("clipboard")).ok()?;
    if clipboard.is_undefined() || clipboard.is_null() {
        return None;
    }
    Some(clipboard.unchecked_into())
}

/// A `ClipboardItem` holding `png` as image/png; `None` where the browser
/// has no `ClipboardItem`.
fn png_item(png: &[u8]) -> Option<JsValue> {
    let parts = Array::new();
    parts.push(&Uint8Array::from(png));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("image/png");
    let blob =
        web_sys::Blob::new_with_u8_array_sequence_and_options(parts.as_ref(), &options).ok()?;
    let record = Object::new();
    Reflect::set(&record, &JsValue::from_str("image/png"), &blob).ok()?;
    web_sys::ClipboardItem::new_with_record_from_str_to_blob_promise(&record)
        .ok()
        .map(JsValue::from)
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

/// Read the system clipboard's text through the browser's asynchronous
/// Clipboard API and hand it to `done` when the promise settles: `None` when
/// the browser refuses (no secure context, no user gesture, permission
/// denied) or has no Clipboard API. Call it inside the user's gesture.
pub(crate) fn read_system(done: impl FnOnce(Option<String>) + 'static) {
    let Some(clipboard) = system_clipboard() else {
        done(None);
        return;
    };
    // Started now, inside the gesture; only the wait is deferred.
    let promise = clipboard.read_text();
    wasm_bindgen_futures::spawn_local(async move {
        let text = wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .ok()
            .and_then(|v| v.as_string());
        done(text);
    });
}
