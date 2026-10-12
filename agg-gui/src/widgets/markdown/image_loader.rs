//! Remote image loading for `MarkdownView` and the rich editor's image cache.
//!
//! **The host fetches.** `MarkdownView` never opens a connection by itself:
//! a remote image (`![alt](https://…)`) loads through a host-supplied
//! [`RemoteImageLoader`], the view's own
//! ([`super::MarkdownView::with_remote_image_loader`]) or the process-wide
//! default ([`set_default_remote_image_loader`]). With neither installed the
//! image stays an alt-text placeholder (behind the view's placeholder icon,
//! when it has one) and no request is made, so tests and offline hosts never
//! touch the network. This mirrors MatterCAD's `ImageLinkSimpleX`, which
//! fetches through the app's `WebCache`, not by itself.
//! [`HttpRemoteImageLoader`] is agg-gui's own fetcher
//! ([`crate::http_fetch::fetch_bytes`]: pure-Rust TLS on native, the
//! browser's fetch on wasm), opt-in in one line.
//!
//! A loader gets the url and a [`RemoteImageDone`], which it completes from
//! any thread with the image's bytes (decoded here: PNG/JPEG/…, SVG, and an
//! animated GIF as an image sequence), decoded pixels, or a sequence. The
//! completion stores the image and wakes the queue of the UI thread that
//! started the load ([`crate::ui_thread::current_queue`], captured up front),
//! not whatever queue a worker would post to, so that thread redraws.
//! [`remote_images_in_flight`] counts the loads started and not yet completed,
//! for a test harness waiting for images to arrive.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::framebuffer::unpremultiply_rgba_inplace;
use crate::image_sequence::ImageSequence;

use super::{ImagePixels, ImageState};

/// Fetches remote images for markdown views on the host's behalf.
///
/// `load` must not block: start the request and return, then complete `done`
/// (from any thread) when the bytes arrive. Dropping `done` uncompleted
/// counts as a failed load, so the view shows its placeholder.
pub trait RemoteImageLoader: Send + Sync {
    /// Start loading `url`; complete `done` with the result.
    fn load(&self, url: &str, done: RemoteImageDone);
}

/// The completion of one remote image load (see [`RemoteImageLoader`]).
///
/// Completing it stores the image (or the failure) and wakes the UI thread
/// that started the load, which redraws. Each method consumes it, so a load
/// completes once.
pub struct RemoteImageDone {
    state: Arc<Mutex<ImageState>>,
    ui_queue: crate::ui_thread::UiQueue,
    completed: bool,
}

/// Loads started and not yet completed, process-wide.
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// The loader views without their own use ([`set_default_remote_image_loader`]).
static DEFAULT_LOADER: RwLock<Option<Arc<dyn RemoteImageLoader>>> = RwLock::new(None);

/// Remote image loads started and not yet completed, across the process.
/// A test harness waits for this to reach zero before checking pixels.
pub fn remote_images_in_flight() -> usize {
    IN_FLIGHT.load(Ordering::SeqCst)
}

/// Install (or with `None`, remove) the process-wide remote image loader,
/// used by every `MarkdownView` and rich editor image cache that has no
/// loader of its own. None is installed by default: remote images do not load.
pub fn set_default_remote_image_loader(loader: Option<Arc<dyn RemoteImageLoader>>) {
    if let Ok(mut slot) = DEFAULT_LOADER.write() {
        *slot = loader;
    }
}

/// The process-wide remote image loader, if one is installed.
pub fn default_remote_image_loader() -> Option<Arc<dyn RemoteImageLoader>> {
    DEFAULT_LOADER.read().ok().and_then(|slot| slot.clone())
}

impl RemoteImageDone {
    /// Complete with the image file's bytes, decoded here (on the calling
    /// thread); bytes that do not decode fail the load.
    pub fn deliver_bytes(self, bytes: &[u8]) {
        let next = decode_image(bytes)
            .map(|image| ImageState::Ready { image, seen: false })
            .unwrap_or(ImageState::Failed);
        self.finish(next);
    }

    /// Complete with decoded RGBA8 pixels, top row first, `width × height`.
    pub fn deliver_rgba(self, data: Vec<u8>, width: u32, height: u32) {
        let next = if width > 0 && height > 0 && data.len() == width as usize * height as usize * 4
        {
            ImageState::Ready {
                image: ImagePixels {
                    data: Arc::new(data),
                    width,
                    height,
                    sequence: None,
                },
                seen: false,
            }
        } else {
            ImageState::Failed
        };
        self.finish(next);
    }

    /// Complete with an image sequence; one with several frames animates.
    pub fn deliver_sequence(self, sequence: ImageSequence) {
        let next = super::animated_image::pixels_of_sequence(sequence)
            .map(|image| ImageState::Ready { image, seen: false })
            .unwrap_or(ImageState::Failed);
        self.finish(next);
    }

    /// Complete as failed: the view shows the image's placeholder.
    pub fn fail(self) {
        self.finish(ImageState::Failed);
    }

    fn finish(mut self, next: ImageState) {
        self.store(next);
    }

    fn store(&mut self, next: ImageState) {
        if self.completed {
            return;
        }
        self.completed = true;
        if let Ok(mut state) = self.state.lock() {
            *state = next;
        }
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
        // Bump the async-state epoch so retained backbuffers
        // (Window FBOs, in-process bitmap caches) re-rasterise on
        // the next frame.  Without this, the freshly-decoded image's
        // dimensions land in the markdown's next layout but the
        // parent Window's cached FBO keeps compositing the previous
        // frame's placeholder rendering — the SVG-badge "wrong
        // scale until first re-draw" bug.
        self.ui_queue.signal_async_state_change();
    }
}

impl Drop for RemoteImageDone {
    fn drop(&mut self) {
        // A loader that gave up without completing: a failed load.
        self.store(ImageState::Failed);
    }
}

/// agg-gui's own HTTP(S) image fetcher ([`crate::http_fetch::fetch_bytes`]).
/// Install it to have remote markdown images fetched directly, e.g.
/// `set_default_remote_image_loader(Some(Arc::new(HttpRemoteImageLoader)))`.
#[derive(Clone, Copy, Debug, Default)]
pub struct HttpRemoteImageLoader;

impl RemoteImageLoader for HttpRemoteImageLoader {
    fn load(&self, url: &str, done: RemoteImageDone) {
        crate::http_fetch::fetch_bytes(url.to_string(), move |result| match result {
            Ok(bytes) => done.deliver_bytes(&bytes),
            Err(_) => done.fail(),
        });
    }
}

impl super::MarkdownView {
    /// Whether an image is still loading or has arrived unseen, so the view
    /// must draw again (part of [`Widget::needs_draw`](crate::Widget::needs_draw)).
    pub(super) fn images_need_draw(&self) -> bool {
        self.image_cache.iter().any(|entry| {
            entry
                .state
                .lock()
                .map(|state| {
                    matches!(
                        *state,
                        ImageState::Loading | ImageState::Ready { seen: false, .. }
                    )
                })
                .unwrap_or(false)
        })
    }

    /// Fetch this view's remote images through `loader` (instead of the
    /// process default, [`set_default_remote_image_loader`]). A host routes
    /// them through its own cache and network policy this way, as MatterCAD's
    /// `ImageLinkSimpleX` goes through its `WebCache`.
    pub fn with_remote_image_loader(mut self, loader: Arc<dyn RemoteImageLoader>) -> Self {
        self.remote_image_loader = Some(loader);
        self
    }

    /// Replace (or with `None`, remove) this view's own remote image loader.
    pub fn set_remote_image_loader(&mut self, loader: Option<Arc<dyn RemoteImageLoader>>) {
        self.remote_image_loader = loader;
    }

    /// How many of this view's remote images are loading (requested from a
    /// loader and not yet delivered).
    pub fn remote_images_in_flight(&self) -> usize {
        self.image_cache
            .iter()
            .filter(|entry| {
                entry
                    .state
                    .lock()
                    .is_ok_and(|state| matches!(*state, ImageState::Loading))
            })
            .count()
    }
}

/// Start loading `url` into `state` through `loader`, or, with no loader,
/// leave it as it is and return `false` (no request is made).
///
/// Called on the painting UI thread: the completion may run on an unbound
/// worker, whose own `current_queue()` is the process's main queue — the
/// wrong thread for an App on a UI thread bound to a queue of its own — so
/// the queue is captured here. Unbound callers capture the main queue.
pub(crate) fn load_remote_image(
    loader: Option<Arc<dyn RemoteImageLoader>>,
    url: String,
    state: Arc<Mutex<ImageState>>,
) -> bool {
    let Some(loader) = loader.or_else(default_remote_image_loader) else {
        return false;
    };
    if let Ok(mut s) = state.lock() {
        *s = ImageState::Loading;
    }
    IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    let done = RemoteImageDone {
        state,
        ui_queue: crate::ui_thread::current_queue(),
        completed: false,
    };
    loader.load(&url, done);
    true
}

fn decode_image(bytes: &[u8]) -> Option<ImagePixels> {
    if looks_like_svg(bytes) {
        return decode_svg(bytes);
    }
    // An animated GIF keeps its frames (`ImageSequence::load_gif`, C#'s
    // `ImageIO.LoadImageData(Stream, ImageSequence)`); a one-frame GIF
    // comes back as a plain still.
    if ImageSequence::is_gif(bytes) {
        if let Some(sequence) = ImageSequence::load_gif(bytes) {
            return super::animated_image::pixels_of_sequence(sequence);
        }
    }

    let image = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    Some(ImagePixels {
        data: Arc::new(image.into_raw()),
        width,
        height,
        sequence: None,
    })
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let prefix_len = bytes.len().min(256);
    let prefix = std::str::from_utf8(&bytes[..prefix_len]).unwrap_or("");
    let trimmed = prefix.trim_start();
    trimmed.starts_with("<svg") || trimmed.starts_with("<?xml")
}

fn decode_svg(bytes: &[u8]) -> Option<ImagePixels> {
    let fb = crate::svg::render_svg_to_framebuffer(bytes).ok()?;
    let width = fb.width();
    let height = fb.height();
    let mut pixels = fb.pixels_flipped();
    unpremultiply_rgba_inplace(&mut pixels);
    Some(ImagePixels {
        data: Arc::new(pixels),
        width,
        height,
        sequence: None,
    })
}

#[cfg(test)]
mod tests {
    use super::decode_image;
    use crate::image_sequence::tests::gif_bytes;

    #[test]
    fn rust_only_fetched_animated_gif_keeps_its_frames() {
        let bytes = gif_bytes(3, 2, &[([255, 0, 0, 255], 5), ([0, 255, 0, 255], 5)]);
        let image = decode_image(&bytes).expect("decodes");
        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!(&image.data[..4], &[255, 0, 0, 255]);
        assert_eq!(image.sequence.expect("animated").num_frames(), 2);
    }

    #[test]
    fn rust_only_fetched_single_frame_gif_is_a_still() {
        let bytes = gif_bytes(3, 2, &[([0, 0, 255, 255], 5)]);
        let image = decode_image(&bytes).expect("decodes");
        assert!(image.sequence.is_none());
        assert_eq!(&image.data[..4], &[0, 0, 255, 255]);
    }
}
