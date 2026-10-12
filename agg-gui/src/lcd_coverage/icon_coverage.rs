//! The LCD subpixel coverage of a vector-drawn icon, kept beside its ordinary
//! single-alpha raster so a 1:1 draw can composite it per channel the way
//! text is.  Port of agg-sharp's `LcdCoverageSidecar`
//! (`agg/LcdCoverage/LcdCoverageSidecar.cs`).
//!
//! [`crate::IconImage`] rasterises an SVG once per physical size and blits the
//! raster ever after, so the per-channel coverage the LCD pipeline would have
//! produced is gone by the time the icon is drawn.  This keeps it:
//! `channel_alpha` is the same picture rasterised through the LCD filter, and
//! `source_alpha` is the raster's own alpha before any pixel filter ran.
//!
//! **Coverage only, never colour.**  Colour is read from the (filtered) raster
//! when the planes are built ([`IconLcdCoverage::build_planes`]), so
//! colour-only edits — a dark-theme lightness inversion, a theme tint, a fade —
//! still show.  Rows are **top row first** in every plane here, matching the
//! icon raster and the `DrawCtx::draw_lcd_backbuffer_arc` input.

use crate::svg::render_svg_tree_to_framebuffer_at_size;

/// Per-channel coverage plus the load-time alpha of one icon raster.
#[derive(Clone, Debug)]
pub struct IconLcdCoverage {
    width: u32,
    height: u32,
    /// R, G, B coverage, 3 bytes per pixel, top row first.
    channel_alpha: Vec<u8>,
    /// The raster's alpha before its pixel filter, 1 byte per pixel, top row first.
    source_alpha: Vec<u8>,
}

/// The two planes `DrawCtx::draw_lcd_backbuffer_arc` takes: premultiplied
/// per-channel colour and per-channel alpha, 3 bytes per pixel, top row first.
#[derive(Clone, Debug)]
pub struct IconLcdPlanes {
    pub color: Vec<u8>,
    pub alpha: Vec<u8>,
}

impl IconLcdCoverage {
    /// Rasterise `tree` through the LCD filter at `width × height` and record
    /// `source_rgba`'s alpha beside it.  `source_rgba` must be the same tree
    /// rendered at that size (straight or premultiplied — only alpha is read),
    /// top row first, before any pixel filter.
    ///
    /// As in C#, rather than fill through the LCD pipeline path by path, this
    /// does the pipeline's two steps to the whole picture: the tree is rendered
    /// 3× wide — exactly the horizontal supersample the LCD mask builder
    /// rasterises a path into — and that render's alpha is run through the same
    /// 5-tap filter, with the Primary Weight and Gamma current now.  Every SVG
    /// feature the renderer supports therefore gets subpixel coverage.
    pub fn from_svg(
        tree: &usvg::Tree,
        source_rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Option<Self> {
        let w = width as usize;
        let h = height as usize;
        if width == 0 || height == 0 || source_rgba.len() < w * h * 4 {
            return None;
        }
        let gray_w = width.checked_mul(3)?;
        let supersampled = render_svg_tree_to_framebuffer_at_size(tree, gray_w, height).ok()?;
        // Framebuffer rows are bottom first; the coverage is top first.
        let px = supersampled.pixels();
        let gw = gray_w as usize;
        let mut gray = vec![0u8; gw * h];
        for row in 0..h {
            let fb_row = h - 1 - row;
            for x in 0..gw {
                gray[row * gw + x] = px[(fb_row * gw + x) * 4 + 3];
            }
        }
        let channel_alpha = super::filter::apply_5_tap_filter(&gray, gray_w, width, height);
        let source_alpha = (0..w * h).map(|i| source_rgba[i * 4 + 3]).collect();
        Some(Self {
            width,
            height,
            channel_alpha,
            source_alpha,
        })
    }

    /// True when this coverage was made for a `width × height` raster.
    pub fn fits(&self, width: u32, height: u32) -> bool {
        self.width == width && self.height == height
    }

    /// The planes that paint `rgba` (straight alpha, top row first, the
    /// raster as it looks now) with this coverage, or `None` when `rgba` is
    /// the wrong size or has gained ink since the coverage was made.
    ///
    /// Per channel `A' = A_c * (a_now / a_orig)`, so a fade applied after load
    /// fades the subpixel coverage with it, and the colour plane is `C * A'`.
    ///
    /// **Fringe pixels**: the filter spreads coverage about a subpixel past
    /// where the gray raster stops, so some pixels have channel coverage but no
    /// raster alpha, and so no colour or alpha ratio of their own.  They borrow
    /// both from the nearest horizontal neighbour that has load-time alpha.
    ///
    /// **`None` when the raster gained ink** — any pixel whose alpha is now
    /// above its alpha at load (a badge a pixel filter drew in, a background
    /// it filled).  The coverage knows nothing about that ink; compositing
    /// through it would drop the ink wherever the coverage is zero, so the
    /// caller takes the plain blit, which shows the raster as it is.  Lower
    /// alpha is a fade and is kept.
    pub fn build_planes(&self, rgba: &[u8], width: u32, height: u32) -> Option<IconLcdPlanes> {
        let w = self.width as usize;
        let h = self.height as usize;
        if !self.fits(width, height) || rgba.len() < w * h * 4 {
            return None;
        }
        if (0..w * h).any(|i| rgba[i * 4 + 3] > self.source_alpha[i]) {
            return None;
        }
        let mut color = vec![0u8; w * h * 3];
        let mut alpha = vec![0u8; w * h * 3];
        for y in 0..h {
            let row_start = y * w;
            for x in 0..w {
                let ci = (row_start + x) * 3;
                let cov = &self.channel_alpha[ci..ci + 3];
                if cov[0] == 0 && cov[1] == 0 && cov[2] == 0 {
                    continue;
                }
                let Some(source_x) = self.nearest_colored_column(row_start, x) else {
                    continue;
                };
                let pi = (row_start + source_x) * 4;
                let ratio =
                    (rgba[pi + 3] as f32 / self.source_alpha[row_start + source_x] as f32).min(1.0);
                for c in 0..3 {
                    let channel_alpha = (cov[c] as f32 / 255.0) * ratio;
                    alpha[ci + c] = to_byte(channel_alpha);
                    color[ci + c] = to_byte((rgba[pi + c] as f32 / 255.0) * channel_alpha);
                }
            }
        }
        Some(IconLcdPlanes { color, alpha })
    }

    /// `x` when the raster had alpha there at load, else the nearer of its
    /// left and right neighbours (two columns out at most — the filter's
    /// reach) that did; left first at equal distance, as in C#.
    fn nearest_colored_column(&self, row_start: usize, x: usize) -> Option<usize> {
        let w = self.width as usize;
        for distance in 0..=2usize {
            if x >= distance && self.source_alpha[row_start + x - distance] > 0 {
                return Some(x - distance);
            }
            if x + distance < w && self.source_alpha[row_start + x + distance] > 0 {
                return Some(x + distance);
            }
        }
        None
    }
}

/// Quantise 0..1 to a byte, rounding half up then clamping, as `LcdBuffer` does.
fn to_byte(value: f32) -> u8 {
    (value * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}
