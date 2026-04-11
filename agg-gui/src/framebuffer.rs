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

impl Framebuffer {
    /// Create a new zeroed framebuffer.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            pixels: vec![0u8; (width * height * 4) as usize],
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

    /// Resize the framebuffer (pixels are zeroed on resize).
    pub fn resize(&mut self, width: u32, height: u32) {
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
            self.pixels = vec![0u8; (width * height * 4) as usize];
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
