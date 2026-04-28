//! WASM-specific in-process clipboard buffer.
//!
//! Because `arboard` (the native clipboard crate) does not work in a browser
//! context, WASM clipboard operations use a thread-local `String` as the
//! in-process buffer.  The JS harness in `demo-wasm` bridges this buffer to
//! the browser's system clipboard:
//!
//! * **Copy / Cut**: the Rust `clipboard_set` stub writes selected text here;
//!   the JS `copy`/`cut` DOM event handler reads it via `wasm_clipboard_get()`
//!   and places it in `event.clipboardData`, which lands in the system clipboard.
//!
//! * **Paste**: the JS `paste` DOM event handler reads the system clipboard text
//!   from `event.clipboardData` and writes it here via `wasm_clipboard_set()`;
//!   it then synthesises a Ctrl+V key event so Rust's paste handler picks it up.
//!
//! This module is compiled only when `target_arch = "wasm32"`.

use std::cell::RefCell;

thread_local! {
    static BUFFER: RefCell<String> = RefCell::new(String::new());
    static HTML_BUFFER: RefCell<String> = RefCell::new(String::new());
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

/// Overwrite the clipboard buffer with `text`.
pub fn set(text: &str) {
    BUFFER.with(|b| *b.borrow_mut() = text.to_string());
    HTML_BUFFER.with(|b| b.borrow_mut().clear());
}

/// Overwrite the clipboard buffers with plain text and rendered HTML.
pub fn set_rich(text: &str, html: &str) {
    BUFFER.with(|b| *b.borrow_mut() = text.to_string());
    HTML_BUFFER.with(|b| *b.borrow_mut() = html.to_string());
}
