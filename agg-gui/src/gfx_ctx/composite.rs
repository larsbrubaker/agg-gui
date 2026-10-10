//! Software blit compositing for `GfxCtx`.
//!
//! `composite_framebuffers` blends one framebuffer onto another (popped
//! layers in `layers.rs`, scaled images in `draw_impl.rs`); `blit_origin`
//! places and culls the per-pixel LCD blits in `draw_impl.rs`. Both cull
//! blits that miss the target before any integer pixel arithmetic, so a
//! widget drawn at an enormous offset is skipped instead of overflowing.

use super::*;
// ---------------------------------------------------------------------------
// SrcOver layer compositing
// ---------------------------------------------------------------------------

/// Composite `src` onto `dst` using SrcOver alpha blending.
///
/// AGG writes **premultiplied** RGBA into framebuffers.  The premultiplied
/// SrcOver formula is:
///
/// ```text
/// out_channel = src_premul + dst_premul × (1 − src_alpha_norm)
/// ```
///
/// This applies identically to all four channels (R, G, B, A), which makes
/// the implementation straightforward and avoids the division step needed for
/// straight-alpha compositing.
///
/// `dest_x` / `dest_y` are the Y-up pixel coordinates in `dst` where the
/// bottom-left corner of `src` lands.  Out-of-bounds pixels are silently clipped.
/// Pixel origin for a `w`×`h` blit whose bottom-left lands at device
/// `(x, y)` in an `fw`×`fh` target, or `None` when the blit misses the target
/// entirely (or the position is not finite).
///
/// Culling in `f64` before converting keeps the per-row `origin + row`
/// arithmetic in range: a widget drawn at an enormous offset (scrolled far
/// away, or measured at a `ScrollView`'s `f64::MAX / 2` height) saturates
/// `as i32` to `i32::MAX`, and adding a row index to that overflowed.
pub(super) fn blit_origin(x: f64, y: f64, w: u32, h: u32, fw: i32, fh: i32) -> Option<(i32, i32)> {
    let (x, y) = (x.round(), y.round());
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    if x >= fw as f64 || y >= fh as f64 || x + w as f64 <= 0.0 || y + h as f64 <= 0.0 {
        return None;
    }
    Some((x as i32, y as i32))
}

pub(super) fn composite_framebuffers(
    dst: &mut Framebuffer,
    src: &Framebuffer,
    dest_x: i32,
    dest_y: i32,
    alpha: f64,
    clip: Option<(f64, f64, f64, f64)>,
) {
    let src_w = src.width() as i32;
    let src_h = src.height() as i32;
    let dst_w = dst.width() as i32;
    let dst_h = dst.height() as i32;
    // Cull a composite that misses the target: a layer pushed at an enormous
    // offset arrives with a saturated `i32` origin, and `dest + row` would
    // overflow. Past this check both sums stay within `i32`.
    let (dx64, dy64) = (dest_x as i64, dest_y as i64);
    if dx64 >= dst_w as i64
        || dy64 >= dst_h as i64
        || dx64 + src_w as i64 <= 0
        || dy64 + src_h as i64 <= 0
    {
        return;
    }

    // Destination scissor bounds in Y-up pixel space (half-open).  `clip` is a
    // screen-space rect in the same coordinates as `dest_x/dest_y`; a composite
    // (e.g. a popped layer) must not paint outside the scissor that was active
    // when the layer was pushed.
    let (cx1, cy1, cx2, cy2) = match clip {
        Some((cx, cy, cw, ch)) => (
            cx.floor() as i32,
            cy.floor() as i32,
            (cx + cw).ceil() as i32,
            (cy + ch).ceil() as i32,
        ),
        None => (0, 0, dst_w, dst_h),
    };

    let src_px = src.pixels();
    let dst_px = dst.pixels_mut();

    for sy in 0..src_h {
        let dy = dest_y + sy;
        if dy < 0 || dy >= dst_h || dy < cy1 || dy >= cy2 {
            continue;
        }
        for sx in 0..src_w {
            let dx = dest_x + sx;
            if dx < 0 || dx >= dst_w || dx < cx1 || dx >= cx2 {
                continue;
            }
            let si = ((sy * src_w + sx) * 4) as usize;
            let di = ((dy * dst_w + dx) * 4) as usize;
            let layer_alpha = alpha.clamp(0.0, 1.0) as f32;
            let sa = (src_px[si + 3] as f32 / 255.0) * layer_alpha;
            if sa < 1e-4 {
                continue;
            } // fully transparent source — skip
            let inv_sa = 1.0 - sa;
            // Premultiplied SrcOver — same formula for all four channels.
            for k in 0..4 {
                let s = src_px[si + k] as f32 * layer_alpha;
                let d = dst_px[di + k] as f32;
                dst_px[di + k] = (s + d * inv_sa).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}
