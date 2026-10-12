//! Whole-pixel plane operations on an [`LcdBuffer`]: the solid rectangle
//! fill behind `LcdGfxCtx::clear_rect` and the weighted layer blend behind
//! `LcdGfxCtx::pop_layer`.
//!
//! Neither runs the LCD pipeline (3× supersample, 5-tap filter): a clear
//! and a layer composite move whole pixels, so every subpixel column of a
//! pixel gets the same treatment and whatever subpixel structure the
//! planes already hold is kept. This is agg-sharp's split too: its
//! `Graphics2D.Clear` is a flat fill of the clip rectangle, and its rounded
//! panel lands in an LCD buffer scaled per pixel by
//! `RoundedClipCoverage`, never through the coverage filter.

use super::LcdBuffer;
use crate::color::Color;

impl LcdBuffer {
    /// Replace every pixel of the half-open integer rectangle
    /// `(x1, y1, x2, y2)` (Y-up, clamped to the buffer) with `color`, in
    /// `CompOp::Src` (copy) terms: the premultiplied colour and the alpha
    /// are written as [`LcdBuffer::clear`] writes them, whatever was there.
    pub fn fill_rect_src(&mut self, rect: (i32, i32, i32, i32), color: Color) {
        let x1 = rect.0.max(0) as usize;
        let y1 = rect.1.max(0) as usize;
        let x2 = rect.2.clamp(0, self.width as i32) as usize;
        let y2 = rect.3.clamp(0, self.height as i32) as usize;
        if x1 >= x2 || y1 >= y2 {
            return;
        }
        let a = color.a.clamp(0.0, 1.0);
        let c = [
            ((color.r.clamp(0.0, 1.0) * a) * 255.0 + 0.5) as u8,
            ((color.g.clamp(0.0, 1.0) * a) * 255.0 + 0.5) as u8,
            ((color.b.clamp(0.0, 1.0) * a) * 255.0 + 0.5) as u8,
        ];
        let a_byte = (a * 255.0 + 0.5) as u8;
        let row = self.width as usize * 3;
        for y in y1..y2 {
            let span = y * row + x1 * 3..y * row + x2 * 3;
            for px in self.color[span.clone()].chunks_exact_mut(3) {
                px.copy_from_slice(&c);
            }
            self.alpha[span].fill(a_byte);
        }
    }

    /// Blend `src` onto this buffer at `(dst_x, dst_y)` as
    /// `dst := dst + (src - dst) * weight(sx, sy)` on both planes, for
    /// every source pixel `(sx, sy)` whose destination lies inside `clip`
    /// (half-open integer pixels, `None` for the whole buffer).
    ///
    /// This is the composite for a layer that was **seeded with its
    /// parent's pixels** (`LcdGfxCtx::push_layer`): the layer already holds
    /// "content over parent", so a weight of 1 is a plain copy and a weight
    /// `w` gives exactly `(w × content) over parent` — for every subpixel
    /// column independently, so per-channel coverage survives. Both planes
    /// are premultiplied, which is what makes the linear blend exact.
    pub fn blend_seeded_layer<F>(
        &mut self,
        src: &LcdBuffer,
        dst_x: i32,
        dst_y: i32,
        clip: Option<(i32, i32, i32, i32)>,
        weight: F,
    ) where
        F: Fn(u32, u32) -> f32,
    {
        let dst_w = self.width as i32;
        let dst_h = self.height as i32;
        let (cx1, cy1, cx2, cy2) = match clip {
            Some((x1, y1, x2, y2)) => (x1.max(0), y1.max(0), x2.min(dst_w), y2.min(dst_h)),
            None => (0, 0, dst_w, dst_h),
        };
        // Source columns and rows that land inside the clip.
        let sx1 = (cx1 - dst_x).max(0);
        let sy1 = (cy1 - dst_y).max(0);
        let sx2 = (cx2 - dst_x).min(src.width as i32);
        let sy2 = (cy2 - dst_y).min(src.height as i32);
        if sx1 >= sx2 || sy1 >= sy2 {
            return;
        }
        let dst_row = self.width as usize * 3;
        let src_row = src.width as usize * 3;
        for sy in sy1..sy2 {
            let dy = (sy + dst_y) as usize;
            for sx in sx1..sx2 {
                let w = weight(sx as u32, sy as u32).clamp(0.0, 1.0);
                if w <= 0.0 {
                    continue;
                }
                let si = sy as usize * src_row + sx as usize * 3;
                let di = dy * dst_row + (sx + dst_x) as usize * 3;
                if w >= 1.0 {
                    self.color[di..di + 3].copy_from_slice(&src.color[si..si + 3]);
                    self.alpha[di..di + 3].copy_from_slice(&src.alpha[si..si + 3]);
                    continue;
                }
                for k in 0..3 {
                    self.color[di + k] = lerp_byte(self.color[di + k], src.color[si + k], w);
                    self.alpha[di + k] = lerp_byte(self.alpha[di + k], src.alpha[si + k], w);
                }
            }
        }
    }
}

#[inline]
fn lerp_byte(dst: u8, src: u8, w: f32) -> u8 {
    let d = dst as f32;
    (d + (src as f32 - d) * w + 0.5).clamp(0.0, 255.0) as u8
}
