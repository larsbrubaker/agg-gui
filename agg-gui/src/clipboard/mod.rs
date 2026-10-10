//! Clipboard helpers shared by text widgets, rich Markdown copy and picture
//! paste.
//!
//! Native builds with the `clipboard` feature start on an **in-process
//! clipboard**: copy and paste work inside the program, and the user's real
//! clipboard is never read or overwritten. A windowed app calls
//! [`use_system_clipboard`] once at startup to switch to the system clipboard
//! (`arboard`); `agg-gui-shell`'s `run` does this for every app it hosts. So a
//! test binary that links the feature (often only through Cargo feature
//! unification) can copy freely without touching the developer's clipboard.
//! Without the feature there is no native clipboard at all.
//!
//! WASM builds use the in-process clipboard (`wasm_clipboard`) that
//! `web_adapter::install_keyboard_listeners` bridges to the browser's
//! `copy` / `cut` / `paste` events; a pasted picture arrives through the
//! `paste` event (`web_paste`), so in the browser [`get_image_rgba`] answers
//! the picture of the latest paste.
//!
//! [`simulate`] swaps in an in-process clipboard for the calling thread (C#
//! agg-sharp's `Clipboard.SetSystemClipboard(new SimulatedClipboard())`), so
//! a test can copy and paste text and pictures in isolation; it takes
//! precedence over both the in-process and the system clipboard.
//!
//! `router.rs` decides between the in-process and the system clipboard;
//! `backend.rs` holds the per-build instance it runs on.

mod backend;
// The router only exists where there is a native clipboard to route.
#[cfg(all(not(target_arch = "wasm32"), any(feature = "clipboard", test)))]
mod router;

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

/// Use the operating system's clipboard from now on, for the whole process.
///
/// Until this is called, a native build's clipboard is an in-process one
/// (text or a picture, shared by every thread), so programs that are not
/// interactive apps, test binaries above all, never overwrite the user's
/// clipboard. A windowed app calls it once at startup; `agg-gui-shell`'s `run`
/// already does. The system connection is made on first use and then kept for
/// the life of the process (on X11 the copying process serves its clipboard,
/// so it must stay connected); when it can't be made, the in-process clipboard
/// answers and the next call tries again. A [`simulate`]d clipboard still takes
/// precedence.
///
/// The switch is sticky: nothing switches back, for the rest of the process.
/// A test that calls it (or that runs `agg_gui_shell::run`) puts every later
/// test in the same test binary on the OS clipboard too, unless those tests
/// [`simulate`]. Does nothing without the `clipboard` feature (there is no
/// native clipboard then) or in the browser (the web shell bridges the page's
/// clipboard events already).
pub fn use_system_clipboard() {
    backend::use_system();
}

/// Close the system clipboard connection [`use_system_clipboard`] keeps.
/// Call it as the app shuts down: on X11, closing the last connection hands
/// the copied contents to the desktop's clipboard manager, so they outlive the
/// app (this can take up to about 100 ms). A later clipboard call reconnects.
/// `agg-gui-shell`'s `run` calls it when its event loop ends.
pub fn release_system_clipboard() {
    backend::release_system();
}

/// Read plain text from the clipboard.
pub fn get_text() -> Option<String> {
    if let Some(text) = SIMULATED.with(|slot| slot.borrow().as_ref().map(|c| c.text.clone())) {
        return text;
    }
    backend::get_text()
}

/// Write plain text to the clipboard.
pub fn set_text(text: &str) {
    if set_simulated(text) {
        return;
    }
    backend::set_text(text);
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

/// Write HTML plus a plain-text fallback to the clipboard.
pub fn set_rich_text(plain_text: &str, html_text: &str) {
    if set_simulated(plain_text) {
        return;
    }
    let html = html_fragment_for_clipboard(html_text);
    backend::set_rich_text(plain_text, &html);
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

/// Try to write an RGBA image to the clipboard: `width * height * 4` bytes
/// of RGBA8, top row first. `false` when the clipboard can't take it (a
/// malformed buffer, no system clipboard, or the browser, which only
/// receives pictures from a paste).
pub fn set_image_rgba(data: &[u8], width: u32, height: u32) -> bool {
    let Some(image) = ClipboardImage::new(width, height, data.to_vec()) else {
        return false;
    };
    let image = match SIMULATED.with(|slot| match slot.borrow_mut().as_mut() {
        Some(simulated) => {
            simulated.put_image(image);
            None
        }
        None => Some(image),
    }) {
        Some(image) => image,
        None => return true,
    };
    backend::set_image(image)
}

/// Read the picture on the clipboard, `None` when there is none (or it can't
/// be read). Natively this is the system clipboard's picture (`arboard`, with
/// the `clipboard` feature, after [`use_system_clipboard`]); in the browser it
/// is the picture of the latest `paste` event, which arrives just before that
/// paste's synthesized `Ctrl+V`.
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
    backend::get_image()
}

/// Whether the clipboard holds a picture, without handing it out: for a menu
/// that enables "Paste picture" when it opens, where [`get_image_rgba`] would
/// copy (natively, decode) a possibly screen-sized image. On macOS and
/// Windows the system clipboard is asked for a picture format and nothing is
/// read; on Linux (X11 / Wayland) arboard offers no format query, so the
/// picture is read and decoded as [`get_image_rgba`] would. The in-process,
/// simulated and browser clipboards answer from what they hold (in the
/// browser: the picture of the latest paste).
pub fn has_image() -> bool {
    if let Some(has) = SIMULATED.with(|slot| slot.borrow().as_ref().map(|c| c.image.is_some())) {
        return has;
    }
    backend::has_image()
}

#[cfg(test)]
mod tests {
    use super::{
        get_image_rgba, get_text, has_image, html_fragment_for_clipboard, set_image_rgba, set_text,
        simulate, ClipboardImage,
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
        // Unit tests never opt in to the system clipboard, so this is the
        // in-process one (per thread in unit tests).
        assert!(set_image_rgba(&RED_THEN_CLEAR, 2, 1));
        assert_eq!(
            get_image_rgba(),
            ClipboardImage::new(2, 1, RED_THEN_CLEAR.to_vec())
        );
        set_text("text");
        assert_eq!(get_image_rgba(), None);
    }

    #[test]
    fn has_image_answers_without_handing_the_picture_out() {
        set_text("words");
        assert!(!has_image());
        assert!(set_image_rgba(&RED_THEN_CLEAR, 2, 1));
        assert!(has_image(), "the in-process clipboard holds a picture");
        {
            let _clipboard = simulate();
            assert!(!has_image(), "the simulated clipboard starts empty");
            assert!(set_image_rgba(&RED_THEN_CLEAR, 2, 1));
            assert!(has_image());
            set_text("replaced");
            assert!(!has_image());
        }
        assert!(has_image(), "the in-process picture is still there");
    }

    #[test]
    fn simulated_clipboard_takes_precedence_over_the_in_process_one() {
        set_text("outer");
        {
            let _clipboard = simulate();
            assert_eq!(get_text(), None);
            set_text("inner");
            assert_eq!(get_text().as_deref(), Some("inner"));
        }
        assert_eq!(get_text().as_deref(), Some("outer"));
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
