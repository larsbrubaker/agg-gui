//! `CompOp::Src` (copy) compositing of an [`LcdMask`] into an [`LcdBuffer`]:
//! the fill behind `LcdGfxCtx::fill` when the blend mode is `Src`, which is
//! what `DrawCtx::clear_clip` and a fractional-edged `clear_rect` use.
//!
//! [`LcdBuffer::composite_mask_with_color`] (in `lcd_coverage.rs`) is the
//! source-over twin.  Src differs in what coverage scales: source-over
//! weights the source by `alpha × coverage`, so a translucent colour
//! *blends* over what is there even where the mask fully covers; Src
//! interpolates between the destination and the source by coverage alone,
//! AGG's `comp_op_rgba_src` with a cover, so a fully covered subpixel column
//! takes the source's premultiplied colour and alpha outright:
//!
//! ```text
//! k_c          = mask.c / 255
//! buf.color_c := src.c * src.a * k_c + buf.color_c * (1 - k_c)
//! buf.alpha_c :=         src.a * k_c + buf.alpha_c * (1 - k_c)
//! ```

use super::{LcdBuffer, LcdMask};
use crate::color::Color;

impl LcdBuffer {
    /// Composite `mask` at `(dst_x, dst_y)` (its bottom-left in this
    /// buffer's Y-up pixels) in `CompOp::Src` terms, with the source colour
    /// of each destination pixel from `color_at(dx, dy)`.  `clip` (half-open
    /// integer pixels) suppresses writes outside it.  A source with zero
    /// alpha still writes: copying transparency is what a clear does.
    pub fn composite_mask_src_with_color<F>(
        &mut self,
        mask: &LcdMask,
        dst_x: i32,
        dst_y: i32,
        clip: Option<(i32, i32, i32, i32)>,
        mut color_at: F,
    ) where
        F: FnMut(i32, i32) -> Color,
    {
        if mask.width == 0 || mask.height == 0 {
            return;
        }
        let dst_w_i = self.width as i32;
        let dst_h_i = self.height as i32;
        let dst_w_u = self.width as usize;
        let mw = mask.width as i32;
        let mh = mask.height as i32;
        let (cx1, cy1, cx2, cy2) = match clip {
            Some((x1, y1, x2, y2)) => (x1.max(0), y1.max(0), x2.min(dst_w_i), y2.min(dst_h_i)),
            None => (0, 0, dst_w_i, dst_h_i),
        };
        if cx1 >= cx2 || cy1 >= cy2 {
            return;
        }
        for my in 0..mh {
            let dy = dst_y + my;
            if dy < cy1 || dy >= cy2 {
                continue;
            }
            for mx in 0..mw {
                let dx = dst_x + mx;
                if dx < cx1 || dx >= cx2 {
                    continue;
                }
                let mi = ((my * mw + mx) * 3) as usize;
                let k = [
                    mask.data[mi] as f32 / 255.0,
                    mask.data[mi + 1] as f32 / 255.0,
                    mask.data[mi + 2] as f32 / 255.0,
                ];
                if k == [0.0; 3] {
                    continue;
                }
                let src = color_at(dx, dy);
                let sa = src.a.clamp(0.0, 1.0);
                let premul = [
                    src.r.clamp(0.0, 1.0) * sa,
                    src.g.clamp(0.0, 1.0) * sa,
                    src.b.clamp(0.0, 1.0) * sa,
                ];
                let di = (dy as usize * dst_w_u + dx as usize) * 3;
                for ch in 0..3 {
                    let bc = self.color[di + ch] as f32 / 255.0;
                    let ba = self.alpha[di + ch] as f32 / 255.0;
                    let rc = premul[ch] * k[ch] + bc * (1.0 - k[ch]);
                    let ra = sa * k[ch] + ba * (1.0 - k[ch]);
                    self.color[di + ch] = (rc * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                    self.alpha[di + ch] = (ra * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}
