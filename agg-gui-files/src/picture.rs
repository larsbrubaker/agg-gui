//! A [`Picture`]: the pixels an item or folder hands a browser for its tile (`item`'s
//! `PictureFuture`). Straight-alpha RGBA, eight bits a channel, top row first, the layout agg-gui
//! draws icons in, so a browser shows it without converting; an app whose own pictures are laid
//! out another way converts once, where it makes them. The pixels are shared (`Arc`), so a picture
//! is cheap to clone into a cache and a row.

use std::sync::Arc;

/// A picture: `width` × `height` pixels of straight-alpha RGBA, top row first, `rgba` holding
/// exactly `width * height * 4` bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

impl Picture {
    /// The picture `width` × `height` with these pixels, or `None` when `rgba` is not exactly
    /// `width * height * 4` bytes (or that size does not fit in memory at all).
    pub fn new(width: u32, height: u32, rgba: impl Into<Arc<[u8]>>) -> Option<Self> {
        let rgba = rgba.into();
        let expected = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        (rgba.len() == expected).then_some(Self {
            width,
            height,
            rgba,
        })
    }

    /// The pixel at column `x` of row `y` (row 0 at the top) as `[r, g, b, a]`, or `None` outside
    /// the picture (or for a picture whose bytes are short of its size).
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        // Checked, so a hand-built picture whose size its bytes cannot hold answers `None`
        // rather than overflowing where `usize` is 32 bits.
        let at = (y as usize)
            .checked_mul(self.width as usize)?
            .checked_add(x as usize)?
            .checked_mul(4)?;
        let bytes = self.rgba.get(at..at.checked_add(4)?)?;
        Some([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
}
