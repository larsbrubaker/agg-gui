//! WASM-specific in-process clipboard buffer.
//!
//! Because `arboard` (the native clipboard crate) does not work in a browser
//! context, WASM clipboard operations use thread-local buffers (text, HTML
//! and a pasted picture) as the in-process clipboard.
//! `web_adapter::install_keyboard_listeners` (or the JS harness in
//! `demo-wasm`) bridges them to the browser's system clipboard:
//!
//! * **Copy / Cut**: the Rust `clipboard_set` stub writes selected text here;
//!   the JS `copy`/`cut` DOM event handler reads it via `wasm_clipboard_get()`
//!   and places it in `event.clipboardData`, which lands in the system clipboard.
//!
//! * **Paste**: the JS `paste` DOM event handler reads the system clipboard text
//!   from `event.clipboardData` and writes it here via `wasm_clipboard_set()`;
//!   it then synthesises a Ctrl+V key event so Rust's paste handler picks it up.
//!   A pasted picture is decoded by `web_paste` and stored with the text
//!   through [`set_paste`] before that Ctrl+V, so the paste handler reads it
//!   through `clipboard::get_image_rgba`.
//!
//! This module is compiled only when `target_arch = "wasm32"`.

use std::cell::RefCell;

use crate::clipboard::ClipboardImage;

thread_local! {
    static BUFFER: RefCell<String> = const { RefCell::new(String::new()) };
    static HTML_BUFFER: RefCell<String> = const { RefCell::new(String::new()) };
    static IMAGE_BUFFER: RefCell<Option<ClipboardImage>> = const { RefCell::new(None) };
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
    BUFFER.with(|b| *b.borrow_mut() = text.to_string());
    HTML_BUFFER.with(|b| b.borrow_mut().clear());
    IMAGE_BUFFER.with(|b| *b.borrow_mut() = None);
}

/// Overwrite the clipboard buffers with plain text and rendered HTML
/// (dropping any picture).
pub fn set_rich(text: &str, html: &str) {
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
