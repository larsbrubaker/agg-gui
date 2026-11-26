//! RGBA framebuffer — the rendering target for the entire widget tree.
//!
//! # Memory layout
//!
//! Pixels are stored in **bottom-up row order** (Y-up): row 0 is at the start
//! of the `Vec<u8>` and corresponds to the bottom edge of the image (Y = 0).
//! Row `height - 1` is at the end and corresponds to the top edge.
//!
//! This matches OpenGL's texture layout: `glTexImage2D` treats the first byte
//! as the bottom-left pixel, so the buffer can be uploaded directly without
//! any Y-flip at the GL boundary.
//!
//! For display targets that use a top-down layout (e.g. HTML Canvas
//! `putImageData`), use [`Framebuffer::pixels_flipped`] to obtain a copy with
//! rows reversed.

/// RGBA framebuffer with bottom-up (Y-up) row ordering.
pub struct Framebuffer {
    pixels: Vec<u8>, // RGBA8, row 0 = bottom (Y = 0)
    width: u32,
    height: u32,
}

/// Byte length of a `width x height` RGBA8 buffer, or `None` when no
/// framebuffer of that size can be drawn into.
///
/// See [`rgba8_byte_len_within`]; the limit here is `isize::MAX`, the longest
/// allocation Rust allows on the target.
pub(crate) fn rgba8_byte_len(width: u32, height: u32) -> Option<usize> {
    rgba8_byte_len_within(width, height, isize::MAX as usize)
}

/// Byte length of a `width x height` RGBA8 buffer, or `None` when it is more
/// than `max_len` bytes or one row does not fit the renderer's stride.
///
/// The product is computed in `usize` with `checked_mul`, so it never wraps.
/// `max_len` is the longest allocation the target allows (`isize::MAX` in
/// production); a parameter so the limit a 32-bit target meets can be tested
/// on a 64-bit one. A length over it would reach `vec!` and panic there with
/// a generic "capacity overflow" instead of a message naming the size.
///
/// The row limit applies whatever the height, zero included: the result is
/// `None` when one row (`width * 4` bytes) does not fit `i32`. Every draw site
/// attaches the buffer with an `i32` row stride, and a wrapped stride would
/// read as a bottom-up buffer, so every `Framebuffer` keeps
/// `width * 4 <= i32::MAX`, which [`Framebuffer::pixels_flipped`] and every
/// stride computation rely on.
pub(crate) fn rgba8_byte_len_within(width: u32, height: u32, max_len: usize) -> Option<usize> {
    let row_bytes = (width as usize).checked_mul(4)?;
    if row_bytes > i32::MAX as usize {
        return None;
    }
    let len = row_bytes.checked_mul(height as usize)?;
    (len <= max_len).then_some(len)
}

/// [`rgba8_byte_len`], panicking with a message that names the size when it
/// is `None`. Shared by [`Framebuffer::new`] and [`Framebuffer::resize`].
fn rgba8_byte_len_or_panic(width: u32, height: u32) -> usize {
    match rgba8_byte_len(width, height) {
        Some(len) => len,
        None => panic!(
            "framebuffer of {width} x {height} pixels is too large: its RGBA8 bytes do not fit \
             an allocation (more than isize::MAX), or one row does not fit the renderer's i32 \
             stride"
        ),
    }
}

impl Framebuffer {
    /// Create a new zeroed framebuffer.
    ///
    /// # Panics
    ///
    /// Panics, naming the size, when `width x height` RGBA8 bytes do not fit
    /// an allocation (more than `isize::MAX`), or one row (`width * 4` bytes)
    /// does not fit the renderer's `i32` stride, whatever the height.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            pixels: vec![0u8; rgba8_byte_len_or_panic(width, height)],
            width,
            height,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Raw RGBA8 pixels in bottom-up row order. Row 0 = Y=0 = bottom of image.
    /// Upload directly to OpenGL without modification.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Mutable access to the raw pixel data.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// Consume the framebuffer and return ownership of the underlying
    /// `Vec<u8>`.  Used by widget backbuffer caching to hand the bitmap
    /// off to an `Arc` without copying.
    pub fn into_pixels(self) -> Vec<u8> {
        self.pixels
    }

    /// Resize the framebuffer (pixels are zeroed on resize).
    ///
    /// # Panics
    ///
    /// Panics, naming the size, when `width x height` RGBA8 bytes do not fit
    /// an allocation (more than `isize::MAX`), or one row (`width * 4` bytes)
    /// does not fit the renderer's `i32` stride, whatever the height. The
    /// framebuffer is left unchanged when it panics.
    pub fn resize(&mut self, width: u32, height: u32) {
        if self.width != width || self.height != height {
            let len = rgba8_byte_len_or_panic(width, height);
            self.width = width;
            self.height = height;
            self.pixels = vec![0u8; len];
        }
    }

    /// Return a copy of the pixels with rows reversed (top-down / Y-down order).
    ///
    /// Use this for HTML Canvas `putImageData`, which expects the first row to
    /// be the top of the image.
    pub fn pixels_flipped(&self) -> Vec<u8> {
        let row_bytes = (self.width * 4) as usize;
        let mut flipped = vec![0u8; self.pixels.len()];
        for y in 0..self.height as usize {
            let src = (self.height as usize - 1 - y) * row_bytes;
            let dst = y * row_bytes;
            flipped[dst..dst + row_bytes].copy_from_slice(&self.pixels[src..src + row_bytes]);
        }
        flipped
    }
}

// ---------------------------------------------------------------------------
// Alpha conversion helpers
// ---------------------------------------------------------------------------
//
// The AGG rasterizer writes **premultiplied** RGBA into its framebuffer.  The
// `DrawCtx::draw_image_rgba` API, in contrast, takes **straight-alpha** input
// (PNG/markdown images, screenshots, etc.).  These helpers convert between
// the two representations at the boundary — Label un-premultiplies its AGG
// backbuffer before handing it to `draw_image_rgba`, and the software
// compositor premultiplies straight input before writing it back into an AGG
// framebuffer for blending.

/// Convert an RGBA8 buffer from **premultiplied** to **straight** alpha
/// in place.  For each pixel, divides the colour channels by the alpha.
/// Zero-alpha pixels are zeroed out (there is no colour information to recover).
pub fn unpremultiply_rgba_inplace(data: &mut [u8]) {
    for px in data.chunks_exact_mut(4) {
        let a = px[3];
        if a == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        } else if a < 255 {
            let af = a as u32;
            // Round-half-up division: (c * 255 + a/2) / a.
            px[0] = (((px[0] as u32) * 255 + af / 2) / af).min(255) as u8;
            px[1] = (((px[1] as u32) * 255 + af / 2) / af).min(255) as u8;
            px[2] = (((px[2] as u32) * 255 + af / 2) / af).min(255) as u8;
        }
    }
}

/// Convert an RGBA8 buffer from **straight** to **premultiplied** alpha
/// in place.  Each colour channel is multiplied by the alpha.
pub fn premultiply_rgba_inplace(data: &mut [u8]) {
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a < 255 {
            // Round-half-up: (c * a + 127) / 255.
            px[0] = (((px[0] as u32) * a + 127) / 255) as u8;
            px[1] = (((px[1] as u32) * a + 127) / 255) as u8;
            px[2] = (((px[2] as u32) * a + 127) / 255) as u8;
        }
    }
}

#[cfg(test)]
mod alpha_tests {
    use super::*;

    /// Round-trip premul → unpremul on a half-alpha white pixel must recover
    /// the original colour channels (within 1/255 rounding error).
    #[test]
    fn test_premul_roundtrip_half_alpha_white() {
        // Straight white at 50 % opacity: (255, 255, 255, 128).
        let mut px = [255, 255, 255, 128];
        premultiply_rgba_inplace(&mut px);
        // Premultiplied: (128, 128, 128, 128).
        assert_eq!(px, [128, 128, 128, 128]);
        unpremultiply_rgba_inplace(&mut px);
        // Round-tripped: should recover (255, 255, 255, 128) exactly.
        assert_eq!(px, [255, 255, 255, 128]);
    }

    /// Fully-opaque pixels are unchanged in both directions.
    #[test]
    fn test_premul_opaque_unchanged() {
        let mut px = [60, 120, 240, 255];
        premultiply_rgba_inplace(&mut px);
        assert_eq!(px, [60, 120, 240, 255]);
        unpremultiply_rgba_inplace(&mut px);
        assert_eq!(px, [60, 120, 240, 255]);
    }

    /// Fully-transparent pixels zero out their colour when un-premultiplied.
    #[test]
    fn test_unpremul_zero_alpha_zeros_colour() {
        let mut px = [10, 20, 30, 0];
        unpremultiply_rgba_inplace(&mut px);
        assert_eq!(px, [0, 0, 0, 0]);
    }
}
