//! Where the clipboard goes when no simulated clipboard is installed, per
//! build:
//!
//! - native with the `clipboard` feature: one process-wide [`Router`] over
//!   `arboard`, in-process until [`super::use_system_clipboard`];
//! - native unit tests: a per-thread [`Router`] over a recording fake, so
//!   parallel tests never share (or reach) a clipboard;
//! - native without the feature: no clipboard (reads find nothing);
//! - the browser: `wasm_clipboard`, bridged to the page's copy / cut / paste
//!   events by `web_adapter`.
//!
//! `router.rs` holds the routing decision these share; `mod.rs` the public API.

use super::ClipboardImage;

#[cfg(all(not(target_arch = "wasm32"), any(feature = "clipboard", test)))]
use super::router::Router;

#[cfg(all(not(target_arch = "wasm32"), feature = "clipboard", not(test)))]
mod system {
    //! The process-wide router and its `arboard` connection.

    use std::borrow::Cow;
    use std::sync::{Mutex, MutexGuard};

    use super::super::router::SystemBackend;
    use super::{ClipboardImage, Router};

    /// One long-lived `arboard` connection (see [`Router`] for why it is kept).
    pub(super) struct Arboard(arboard::Clipboard);

    impl Arboard {
        fn connect() -> Option<Self> {
            arboard::Clipboard::new().ok().map(Self)
        }
    }

    impl SystemBackend for Arboard {
        fn get_text(&mut self) -> Option<String> {
            self.0.get_text().ok()
        }

        fn set_text(&mut self, text: &str) {
            let _ = self.0.set_text(text.to_string());
        }

        fn set_rich_text(&mut self, plain_text: &str, html_text: &str) {
            if self
                .0
                .set_html(Cow::Borrowed(html_text), Some(Cow::Borrowed(plain_text)))
                .is_err()
            {
                self.set_text(plain_text);
            }
        }

        fn get_image(&mut self) -> Option<ClipboardImage> {
            let image = self.0.get_image().ok()?;
            ClipboardImage::new(
                u32::try_from(image.width).ok()?,
                u32::try_from(image.height).ok()?,
                image.bytes.into_owned(),
            )
        }

        /// macOS: whether the pasteboard can hand out TIFF, the flavour
        /// arboard reads a picture from (AppKit converts PNG and other
        /// picture types to it), checked without fetching the data.
        #[cfg(target_os = "macos")]
        fn has_image(&mut self) -> bool {
            use objc2_app_kit::{NSPasteboard, NSPasteboardTypeTIFF};
            use objc2_foundation::NSArray;

            let pasteboard = NSPasteboard::generalPasteboard();
            // SAFETY: `NSPasteboardTypeTIFF` is an immutable AppKit string
            // constant that lives for the whole process.
            let tiff = unsafe { NSPasteboardTypeTIFF };
            pasteboard
                .availableTypeFromArray(&NSArray::from_slice(&[tiff]))
                .is_some()
        }

        /// Windows: whether one of the formats arboard reads a picture from
        /// (a registered "PNG", else `CF_DIBV5`, which Windows synthesizes
        /// from any bitmap) is available. `IsClipboardFormatAvailable` needs
        /// no open clipboard and reads nothing.
        #[cfg(windows)]
        fn has_image(&mut self) -> bool {
            clipboard_win::register_format("PNG")
                .map(u32::from)
                .is_some_and(clipboard_win::is_format_avail)
                || clipboard_win::is_format_avail(clipboard_win::formats::CF_DIBV5)
        }

        /// Linux and other platforms: arboard has no format query, so this
        /// reads (and decodes) the picture.
        #[cfg(not(any(target_os = "macos", windows)))]
        fn has_image(&mut self) -> bool {
            self.get_image().is_some()
        }

        fn set_image(&mut self, image: &ClipboardImage) -> bool {
            self.0
                .set_image(arboard::ImageData {
                    width: image.width() as usize,
                    height: image.height() as usize,
                    bytes: Cow::Borrowed(image.rgba()),
                })
                .is_ok()
        }
    }

    /// Process-wide, so text copied on one thread pastes on another, exactly
    /// as with the system clipboard.
    static ROUTER: Mutex<Router<Arboard>> = Mutex::new(Router::new(Arboard::connect));

    pub(super) fn router() -> MutexGuard<'static, Router<Arboard>> {
        // A panic while holding the lock leaves the contents usable.
        ROUTER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "clipboard", not(test)))]
fn with_router<R>(f: impl FnOnce(&mut Router<system::Arboard>) -> R) -> R {
    f(&mut system::router())
}

#[cfg(all(not(target_arch = "wasm32"), test))]
type TestRouter = Router<super::router::tests::FakeSystem>;

#[cfg(all(not(target_arch = "wasm32"), test))]
thread_local! {
    /// Unit tests run in parallel threads; each gets its own clipboard.
    static TEST_ROUTER: std::cell::RefCell<TestRouter> =
        const { std::cell::RefCell::new(Router::new(super::router::tests::FakeSystem::connect)) };
}

#[cfg(all(not(target_arch = "wasm32"), test))]
fn with_router<R>(f: impl FnOnce(&mut TestRouter) -> R) -> R {
    TEST_ROUTER.with(|router| f(&mut router.borrow_mut()))
}

#[cfg(all(not(target_arch = "wasm32"), any(feature = "clipboard", test)))]
mod routed {
    //! Native builds with a router (the feature on, or unit tests).

    use super::{with_router, ClipboardImage};

    pub(in crate::clipboard) fn use_system() {
        with_router(|r| r.use_system());
    }

    pub(in crate::clipboard) fn release_system() {
        with_router(|r| r.release_system());
    }

    pub(in crate::clipboard) fn get_text() -> Option<String> {
        with_router(|r| r.get_text())
    }

    pub(in crate::clipboard) fn set_text(text: &str) {
        with_router(|r| r.set_text(text));
    }

    pub(in crate::clipboard) fn set_rich_text(plain_text: &str, html_text: &str) {
        with_router(|r| r.set_rich_text(plain_text, html_text));
    }

    pub(in crate::clipboard) fn get_image() -> Option<ClipboardImage> {
        with_router(|r| r.get_image())
    }

    pub(in crate::clipboard) fn has_image() -> bool {
        with_router(|r| r.has_image())
    }

    pub(in crate::clipboard) fn set_image(image: ClipboardImage) -> bool {
        with_router(|r| r.set_image(image))
    }
}

#[cfg(all(not(target_arch = "wasm32"), any(feature = "clipboard", test)))]
pub(super) use routed::*;

#[cfg(all(not(target_arch = "wasm32"), not(feature = "clipboard"), not(test)))]
mod absent {
    //! Native without the `clipboard` feature: there is no clipboard.

    use super::ClipboardImage;

    pub(in crate::clipboard) fn use_system() {}

    pub(in crate::clipboard) fn release_system() {}

    pub(in crate::clipboard) fn get_text() -> Option<String> {
        None
    }

    pub(in crate::clipboard) fn set_text(_: &str) {}

    pub(in crate::clipboard) fn set_rich_text(_: &str, _: &str) {}

    pub(in crate::clipboard) fn get_image() -> Option<ClipboardImage> {
        None
    }

    pub(in crate::clipboard) fn has_image() -> bool {
        false
    }

    pub(in crate::clipboard) fn set_image(_: ClipboardImage) -> bool {
        false
    }
}

#[cfg(all(not(target_arch = "wasm32"), not(feature = "clipboard"), not(test)))]
pub(super) use absent::*;

#[cfg(target_arch = "wasm32")]
mod browser {
    //! The browser: the page's clipboard events, through `wasm_clipboard`.

    use super::ClipboardImage;

    /// The browser shell already bridges the page's clipboard.
    pub(in crate::clipboard) fn use_system() {}

    pub(in crate::clipboard) fn release_system() {}

    pub(in crate::clipboard) fn get_text() -> Option<String> {
        crate::wasm_clipboard::get()
    }

    pub(in crate::clipboard) fn set_text(text: &str) {
        crate::wasm_clipboard::set(text);
    }

    pub(in crate::clipboard) fn set_rich_text(plain_text: &str, html_text: &str) {
        crate::wasm_clipboard::set_rich(plain_text, html_text);
    }

    pub(in crate::clipboard) fn get_image() -> Option<ClipboardImage> {
        crate::wasm_clipboard::get_image()
    }

    pub(in crate::clipboard) fn has_image() -> bool {
        crate::wasm_clipboard::has_image()
    }

    /// The browser only receives pictures from a paste; writing one to the
    /// system clipboard would need the asynchronous `ClipboardItem` API.
    pub(in crate::clipboard) fn set_image(_: ClipboardImage) -> bool {
        false
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) use browser::*;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::with_router;
    use crate::clipboard::{get_text, set_rich_text, set_text};

    #[test]
    fn without_opting_in_the_api_round_trips_in_process() {
        set_text("copied");
        assert_eq!(get_text().as_deref(), Some("copied"));
        set_rich_text("plain", "<b>plain</b>");
        assert_eq!(get_text().as_deref(), Some("plain"));
        assert!(
            with_router(|r| !r.is_connected()),
            "no system clipboard connection was made"
        );
    }
}
