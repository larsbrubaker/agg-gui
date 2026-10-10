//! Clipboard helpers shared by text widgets, rich Markdown copy and picture
//! paste.
//!
//! Native builds use `arboard` when the `clipboard` feature is enabled. WASM
//! builds use the in-process clipboard (`wasm_clipboard`) that
//! `web_adapter::install_keyboard_listeners` bridges to the browser's
//! `copy` / `cut` / `paste` events; a pasted picture arrives through the
//! `paste` event (`web_paste`), so in the browser [`get_image_rgba`] answers
//! the picture of the latest paste.
//!
//! [`simulate`] swaps in an in-process clipboard for the calling thread (C#
//! agg-sharp's `Clipboard.SetSystemClipboard(new SimulatedClipboard())`), so
//! a test can copy and paste text and pictures without touching the user's
//! real clipboard.

#[cfg(all(feature = "clipboard", not(test)))]
use std::borrow::Cow;

#[cfg(all(feature = "clipboard", not(test)))]
use arboard::Clipboard;

/// A picture on the clipboard: RGBA8 pixels, rows top to bottom, alpha not
/// premultiplied (what `arboard` and the browser's `getImageData` both hand
/// out).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardImage {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl ClipboardImage {
    /// A picture of `width` x `height` pixels from `rgba` (4 bytes a pixel).
    /// `None` when either side is zero (no picture) or `rgba` is not exactly
    /// `width * height * 4` bytes long.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        let expected = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if width == 0 || height == 0 || rgba.len() != expected {
            return None;
        }
        Some(Self {
            width,
            height,
            rgba,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, `width * height * 4` bytes of RGBA8, top row first.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// The pixels, taken out of the picture.
    pub fn into_rgba(self) -> Vec<u8> {
        self.rgba
    }
}

/// What an in-process clipboard holds. Like a system clipboard it holds one
/// thing at a time: writing text drops the picture and writing a picture
/// drops the text.
#[derive(Clone, Debug, Default)]
struct Contents {
    text: Option<String>,
    image: Option<ClipboardImage>,
}

impl Contents {
    fn put_text(&mut self, text: &str) {
        self.text = Some(text.to_string());
        self.image = None;
    }

    fn put_image(&mut self, image: ClipboardImage) {
        self.text = None;
        self.image = Some(image);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
thread_local! {
    /// The clipboard unit tests see in place of the system one.
    static TEST_CONTENTS: std::cell::RefCell<Contents> = std::cell::RefCell::new(Contents::default());
}

thread_local! {
    /// The calling thread's simulated clipboard while one is installed
    /// (`None` when none is).
    static SIMULATED: std::cell::RefCell<Option<Contents>> = const { std::cell::RefCell::new(None) };
}

/// A simulated clipboard installed by [`simulate`]; dropping it puts back
/// whatever clipboard the thread had before.
#[must_use = "dropping the guard uninstalls the simulated clipboard"]
pub struct SimulatedClipboard {
    previous: Option<Contents>,
}

impl Drop for SimulatedClipboard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        let _ = SIMULATED.try_with(|slot| *slot.borrow_mut() = previous);
    }
}

/// Install an empty in-process clipboard for this thread until the returned
/// guard is dropped: every read and write of plain or rich text and of
/// pictures goes to it instead of the system clipboard.
pub fn simulate() -> SimulatedClipboard {
    let previous = SIMULATED.with(|slot| slot.borrow_mut().replace(Contents::default()));
    SimulatedClipboard { previous }
}

/// Read plain text from the clipboard.
pub fn get_text() -> Option<String> {
    if let Some(text) = SIMULATED.with(|slot| slot.borrow().as_ref().map(|c| c.text.clone())) {
        return text;
    }
    get_text_impl()
}

#[cfg(all(feature = "clipboard", not(test)))]
fn get_text_impl() -> Option<String> {
    Clipboard::new().ok()?.get_text().ok()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
fn get_text_impl() -> Option<String> {
    TEST_CONTENTS.with(|contents| contents.borrow().text.clone())
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
    if set_simulated(text) {
        return;
    }
    set_text_impl(text);
}

/// Write `text` to the simulated clipboard when one is installed.
fn set_simulated(text: &str) -> bool {
    SIMULATED.with(|slot| match slot.borrow_mut().as_mut() {
        Some(simulated) => {
            simulated.put_text(text);
            true
        }
        None => false,
    })
}

#[cfg(all(feature = "clipboard", not(test)))]
fn set_text_impl(text: &str) {
    if let Ok(mut cb) = Clipboard::new() {
        let _ = cb.set_text(text.to_string());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
fn set_text_impl(text: &str) {
    TEST_CONTENTS.with(|contents| contents.borrow_mut().put_text(text));
}

#[cfg(all(not(feature = "clipboard"), not(test), not(target_arch = "wasm32")))]
fn set_text_impl(_: &str) {}

#[cfg(all(not(feature = "clipboard"), target_arch = "wasm32"))]
fn set_text_impl(text: &str) {
    crate::wasm_clipboard::set(text);
}

/// Write HTML plus a plain-text fallback to the clipboard.
pub fn set_rich_text(plain_text: &str, html_text: &str) {
    if set_simulated(plain_text) {
        return;
    }
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

/// Try to write an RGBA image to the clipboard: `width * height * 4` bytes
/// of RGBA8, top row first. `false` when the clipboard can't take it (a
/// malformed buffer, no system clipboard, or the browser, which only
/// receives pictures from a paste).
pub fn set_image_rgba(data: &[u8], width: u32, height: u32) -> bool {
    if let Some(stored) = SIMULATED.with(|slot| {
        slot.borrow_mut().as_mut().map(|simulated| {
            match ClipboardImage::new(width, height, data.to_vec()) {
                Some(image) => {
                    simulated.put_image(image);
                    true
                }
                None => false,
            }
        })
    }) {
        return stored;
    }
    set_image_rgba_impl(data, width, height)
}

#[cfg(all(feature = "clipboard", not(test)))]
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

#[cfg(all(test, not(target_arch = "wasm32")))]
fn set_image_rgba_impl(data: &[u8], width: u32, height: u32) -> bool {
    let Some(image) = ClipboardImage::new(width, height, data.to_vec()) else {
        return false;
    };
    TEST_CONTENTS.with(|contents| contents.borrow_mut().put_image(image));
    true
}

#[cfg(all(not(feature = "clipboard"), not(test), not(target_arch = "wasm32")))]
fn set_image_rgba_impl(_: &[u8], _: u32, _: u32) -> bool {
    false
}

/// The browser only receives pictures from a paste; writing one to the
/// system clipboard would need the asynchronous `ClipboardItem` API.
#[cfg(all(not(feature = "clipboard"), target_arch = "wasm32"))]
fn set_image_rgba_impl(_: &[u8], _: u32, _: u32) -> bool {
    false
}

/// Read the picture on the clipboard, `None` when there is none (or it can't
/// be read). Natively this is the system clipboard's picture (`arboard`, with
/// the `clipboard` feature); in the browser it is the picture of the latest
/// `paste` event, which arrives just before that paste's synthesized `Ctrl+V`.
///
/// A browser paste can carry text and a picture together (a picture copied
/// from a web page brings its markup as text): both are then readable, through
/// [`get_text`] and this, and the consumer chooses which to paste. A browser
/// without `OffscreenCanvas` (Safari before 16.4) can't read a pasted
/// picture's pixels, so there only the paste's text is delivered.
pub fn get_image_rgba() -> Option<ClipboardImage> {
    if let Some(image) = SIMULATED.with(|slot| slot.borrow().as_ref().map(|c| c.image.clone())) {
        return image;
    }
    get_image_rgba_impl()
}

#[cfg(all(feature = "clipboard", not(test)))]
fn get_image_rgba_impl() -> Option<ClipboardImage> {
    let image = Clipboard::new().ok()?.get_image().ok()?;
    ClipboardImage::new(
        u32::try_from(image.width).ok()?,
        u32::try_from(image.height).ok()?,
        image.bytes.into_owned(),
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
fn get_image_rgba_impl() -> Option<ClipboardImage> {
    TEST_CONTENTS.with(|contents| contents.borrow().image.clone())
}

#[cfg(all(not(feature = "clipboard"), not(test), not(target_arch = "wasm32")))]
fn get_image_rgba_impl() -> Option<ClipboardImage> {
    None
}

#[cfg(all(not(feature = "clipboard"), target_arch = "wasm32"))]
fn get_image_rgba_impl() -> Option<ClipboardImage> {
    crate::wasm_clipboard::get_image()
}

#[cfg(test)]
mod tests {
    use super::{
        get_image_rgba, get_text, html_fragment_for_clipboard, set_image_rgba, set_text, simulate,
        ClipboardImage,
    };

    const RED_THEN_CLEAR: [u8; 8] = [255, 0, 0, 255, 0, 0, 0, 0];

    #[test]
    fn picture_needs_four_bytes_a_pixel_and_a_size() {
        let image = ClipboardImage::new(2, 1, RED_THEN_CLEAR.to_vec()).expect("2x1 RGBA");
        assert_eq!((image.width(), image.height()), (2, 1));
        assert_eq!(image.rgba(), &RED_THEN_CLEAR);
        assert_eq!(image.into_rgba(), RED_THEN_CLEAR.to_vec());
        assert_eq!(ClipboardImage::new(1, 1, RED_THEN_CLEAR.to_vec()), None);
        assert_eq!(ClipboardImage::new(3, 1, RED_THEN_CLEAR.to_vec()), None);
        assert_eq!(ClipboardImage::new(0, 2, Vec::new()), None);
        assert_eq!(ClipboardImage::new(2, 0, Vec::new()), None);
        assert_eq!(ClipboardImage::new(u32::MAX, u32::MAX, Vec::new()), None);
    }

    #[test]
    fn copied_picture_is_pasted_back() {
        let _clipboard = simulate();
        assert_eq!(get_image_rgba(), None);
        assert!(set_image_rgba(&RED_THEN_CLEAR, 2, 1));
        assert_eq!(
            get_image_rgba(),
            ClipboardImage::new(2, 1, RED_THEN_CLEAR.to_vec())
        );
    }

    #[test]
    fn malformed_picture_is_refused_and_keeps_the_clipboard() {
        let _clipboard = simulate();
        set_text("kept");
        assert!(!set_image_rgba(&RED_THEN_CLEAR, 3, 1));
        assert_eq!(get_text().as_deref(), Some("kept"));
        assert_eq!(get_image_rgba(), None);
    }

    #[test]
    fn clipboard_holds_text_or_a_picture_not_both() {
        let _clipboard = simulate();
        set_text("words");
        assert!(set_image_rgba(&RED_THEN_CLEAR, 2, 1));
        assert_eq!(get_text(), None);
        set_text("again");
        assert_eq!(get_image_rgba(), None);
        assert_eq!(get_text().as_deref(), Some("again"));
    }

    #[test]
    fn ending_a_simulation_restores_the_outer_clipboard() {
        let _outer = simulate();
        assert!(set_image_rgba(&RED_THEN_CLEAR, 2, 1));
        {
            let _inner = simulate();
            assert_eq!(get_image_rgba(), None);
        }
        assert_eq!(
            get_image_rgba(),
            ClipboardImage::new(2, 1, RED_THEN_CLEAR.to_vec())
        );
    }

    #[test]
    fn unsimulated_clipboard_carries_pictures_too() {
        // Unit tests replace the system clipboard with a per-thread buffer.
        assert!(set_image_rgba(&RED_THEN_CLEAR, 2, 1));
        assert_eq!(
            get_image_rgba(),
            ClipboardImage::new(2, 1, RED_THEN_CLEAR.to_vec())
        );
        set_text("text");
        assert_eq!(get_image_rgba(), None);
    }

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
