//! Async remote image loading for `MarkdownView`.
//!
//! The markdown widget keeps rendering lightweight placeholders while this
//! module fetches and decodes HTTP(S) images. Fetching goes through
//! [`crate::http_fetch::fetch_bytes`] (pure-Rust TLS on native, the browser's
//! fetch on wasm), which calls the completion callback when bytes arrive.
//! That callback may run on a worker thread, so the wakeup it sends goes to
//! the queue of the UI thread that started the load
//! ([`crate::ui_thread::current_queue`], captured up front), not to whatever
//! queue the worker itself would post to.

use std::sync::{Arc, Mutex};

use crate::framebuffer::unpremultiply_rgba_inplace;

use super::{ImagePixels, ImageState};

pub(super) fn load_remote_image(url: String, state: Arc<Mutex<ImageState>>) {
    // Captured on the calling (painting) UI thread: the native completion runs
    // on an unbound worker, whose own `current_queue()` is the process's main
    // queue — the wrong thread for an App on a UI thread bound to a queue of
    // its own. Unbound callers capture the main queue, as before.
    let ui_queue = crate::ui_thread::current_queue();
    crate::http_fetch::fetch_bytes(url, move |result| {
        let next = match result {
            Ok(bytes) => decode_image(&bytes)
                .map(|image| ImageState::Ready { image, seen: false })
                .unwrap_or(ImageState::Failed),
            Err(_) => ImageState::Failed,
        };

        if let Ok(mut state) = state.lock() {
            *state = next;
        }
        // Bump the async-state epoch so retained backbuffers
        // (Window FBOs, in-process bitmap caches) re-rasterise on
        // the next frame.  Without this, the freshly-decoded image's
        // dimensions land in the markdown's next layout but the
        // parent Window's cached FBO keeps compositing the previous
        // frame's placeholder rendering — the SVG-badge "wrong
        // scale until first re-draw" bug.
        ui_queue.signal_async_state_change();
    });
}

fn decode_image(bytes: &[u8]) -> Option<ImagePixels> {
    if looks_like_svg(bytes) {
        return decode_svg(bytes);
    }

    let image = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    Some(ImagePixels {
        data: Arc::new(image.into_raw()),
        width,
        height,
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
    })
}
