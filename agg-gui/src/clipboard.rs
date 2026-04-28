//! Clipboard helpers shared by text widgets and rich Markdown copy.
//!
//! Native builds use `arboard` when the `clipboard` feature is enabled. WASM
//! builds write into the in-process clipboard bridge that the demo shell
//! forwards to browser clipboard events.

#[cfg(feature = "clipboard")]
use std::borrow::Cow;

#[cfg(feature = "clipboard")]
use arboard::Clipboard;

/// Read plain text from the clipboard.
pub fn get_text() -> Option<String> {
    get_text_impl()
}

#[cfg(feature = "clipboard")]
fn get_text_impl() -> Option<String> {
    Clipboard::new().ok()?.get_text().ok()
}

#[cfg(all(not(feature = "clipboard"), not(target_arch = "wasm32")))]
fn get_text_impl() -> Option<String> {
    None
}

#[cfg(all(not(feature = "clipboard"), target_arch = "wasm32"))]
fn get_text_impl() -> Option<String> {
    crate::wasm_clipboard::get()
}

/// Write plain text to the clipboard.
pub fn set_text(text: &str) {
    set_text_impl(text);
}

#[cfg(feature = "clipboard")]
fn set_text_impl(text: &str) {
    if let Ok(mut cb) = Clipboard::new() {
        let _ = cb.set_text(text.to_string());
    }
}

#[cfg(all(not(feature = "clipboard"), not(target_arch = "wasm32")))]
fn set_text_impl(_: &str) {}

#[cfg(all(not(feature = "clipboard"), target_arch = "wasm32"))]
fn set_text_impl(text: &str) {
    crate::wasm_clipboard::set(text);
}

/// Write HTML plus a plain-text fallback to the clipboard.
pub fn set_rich_text(plain_text: &str, html_text: &str) {
    set_rich_text_impl(plain_text, html_text);
}

#[cfg(feature = "clipboard")]
fn set_rich_text_impl(plain_text: &str, html_text: &str) {
    if let Ok(mut cb) = Clipboard::new() {
        if cb
            .set_html(
                Cow::Borrowed(html_text),
                Some(Cow::Borrowed(plain_text)),
            )
            .is_ok()
        {
            return;
        }
    }
    set_text(plain_text);
}

#[cfg(all(not(feature = "clipboard"), not(target_arch = "wasm32")))]
fn set_rich_text_impl(plain_text: &str, _: &str) {
    set_text(plain_text);
}

#[cfg(all(not(feature = "clipboard"), target_arch = "wasm32"))]
fn set_rich_text_impl(plain_text: &str, html_text: &str) {
    crate::wasm_clipboard::set_rich(plain_text, html_text);
}

/// Try to write an RGBA image to the clipboard.
pub fn set_image_rgba(data: &[u8], width: u32, height: u32) -> bool {
    set_image_rgba_impl(data, width, height)
}

#[cfg(feature = "clipboard")]
fn set_image_rgba_impl(data: &[u8], width: u32, height: u32) -> bool {
    use arboard::ImageData;

    let Ok(mut cb) = Clipboard::new() else {
        return false;
    };
    cb.set_image(ImageData {
        width: width as usize,
        height: height as usize,
        bytes: Cow::Borrowed(data),
    })
    .is_ok()
}

#[cfg(not(feature = "clipboard"))]
fn set_image_rgba_impl(_: &[u8], _: u32, _: u32) -> bool {
    false
}
