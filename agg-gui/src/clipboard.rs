//! Clipboard helpers shared by text widgets and rich Markdown copy.
//!
//! Native builds use `arboard` when the `clipboard` feature is enabled. WASM
//! builds write into the in-process clipboard bridge that the demo shell
//! forwards to browser clipboard events.

#[cfg(all(feature = "clipboard", not(test)))]
use std::borrow::Cow;

#[cfg(all(feature = "clipboard", not(test)))]
use arboard::Clipboard;

#[cfg(all(test, not(target_arch = "wasm32")))]
thread_local! {
    static TEST_TEXT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Read plain text from the clipboard.
pub fn get_text() -> Option<String> {
    get_text_impl()
}

#[cfg(all(feature = "clipboard", not(test)))]
fn get_text_impl() -> Option<String> {
    Clipboard::new().ok()?.get_text().ok()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
fn get_text_impl() -> Option<String> {
    TEST_TEXT.with(|text| text.borrow().clone())
}

#[cfg(all(not(feature = "clipboard"), not(test), not(target_arch = "wasm32")))]
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

#[cfg(all(feature = "clipboard", not(test)))]
fn set_text_impl(text: &str) {
    if let Ok(mut cb) = Clipboard::new() {
        let _ = cb.set_text(text.to_string());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
fn set_text_impl(text: &str) {
    TEST_TEXT.with(|slot| *slot.borrow_mut() = Some(text.to_string()));
}

#[cfg(all(not(feature = "clipboard"), not(test), not(target_arch = "wasm32")))]
fn set_text_impl(_: &str) {}

#[cfg(all(not(feature = "clipboard"), target_arch = "wasm32"))]
fn set_text_impl(text: &str) {
    crate::wasm_clipboard::set(text);
}

/// Write HTML plus a plain-text fallback to the clipboard.
pub fn set_rich_text(plain_text: &str, html_text: &str) {
    let html = html_fragment_for_clipboard(html_text);
    set_rich_text_impl(plain_text, &html);
}

/// Mark the selected HTML fragment explicitly for rich-text paste targets.
///
/// Windows CF_HTML wrappers also carry byte offsets, but browser editors such
/// as Gmail are more reliable when the payload itself includes these markers.
pub fn html_fragment_for_clipboard(html_text: &str) -> String {
    if html_text.contains("<!--StartFragment-->") && html_text.contains("<!--EndFragment-->") {
        html_text.to_string()
    } else {
        format!("<!--StartFragment-->{html_text}<!--EndFragment-->")
    }
}

#[cfg(all(feature = "clipboard", not(test)))]
fn set_rich_text_impl(plain_text: &str, html_text: &str) {
    if let Ok(mut cb) = Clipboard::new() {
        if cb
            .set_html(Cow::Borrowed(html_text), Some(Cow::Borrowed(plain_text)))
            .is_ok()
        {
            return;
        }
    }
    set_text(plain_text);
}

#[cfg(all(any(not(feature = "clipboard"), test), not(target_arch = "wasm32")))]
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
    use std::borrow::Cow;

    use arboard::{Clipboard, ImageData};

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

#[cfg(test)]
mod tests {
    use super::html_fragment_for_clipboard;

    #[test]
    fn rich_html_is_wrapped_with_fragment_markers_for_gmail() {
        let html = html_fragment_for_clipboard("<h1>Hello</h1>");
        assert!(html.starts_with("<!--StartFragment-->"));
        assert!(html.ends_with("<!--EndFragment-->"));
        assert!(html.contains("<h1>Hello</h1>"));
    }

    #[test]
    fn rich_html_wrapper_is_idempotent() {
        let html = "<!--StartFragment--><b>Hello</b><!--EndFragment-->";
        assert_eq!(html_fragment_for_clipboard(html), html);
    }
}
