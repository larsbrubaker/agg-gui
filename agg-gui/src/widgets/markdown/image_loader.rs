//! Async remote image loading for `MarkdownView`.
//!
//! The markdown widget keeps rendering lightweight placeholders while this
//! module fetches and decodes HTTP(S) images. Native and WASM are both handled
//! by `ehttp`, which calls the completion callback when bytes are available.

use std::sync::{Arc, Mutex};

use crate::framebuffer::unpremultiply_rgba_inplace;

use super::{ImagePixels, ImageState};

pub(super) fn load_remote_image(url: String, state: Arc<Mutex<ImageState>>) {
    ehttp::fetch(ehttp::Request::get(url), move |result| {
        let next = match result {
            Ok(response) if response.ok => decode_image(&response.bytes)
                .map(|image| ImageState::Ready { image, seen: false })
                .unwrap_or(ImageState::Failed),
            _ => ImageState::Failed,
        };

        if let Ok(mut state) = state.lock() {
            *state = next;
        }
        crate::animation::request_draw();
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
