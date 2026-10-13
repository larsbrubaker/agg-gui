//! Software blit compositing for `GfxCtx`.
//!
//! `composite_framebuffers` blends one framebuffer onto another (popped
//! layers in `layers.rs`, scaled images in `draw_impl.rs`); `blit_origin`
//! places and culls the per-pixel LCD blits in `draw_impl.rs`. Both cull
//! blits that miss the target before any integer pixel arithmetic, so a
//! widget drawn at an enormous offset is skipped instead of overflowing.
//! `opaque_under` answers `takes_lcd_image_over` and gates
//! `draw_lcd_backbuffer_arc`: whether an LCD composite lands only on opaque
//! pixels; `collapsed_lcd_framebuffer` is the single-alpha form it falls back
//! to everywhere else.

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

/// The part of `footprint` (left, bottom, right, top; half-open, Y-up device
/// pixels) that lands on `fb` and that `clip` leaves, or `None` when none of
/// it does.  The clip is rounded to pixels as `draw_lcd_backbuffer_arc` does.
fn landing_footprint(
    fb: &Framebuffer,
    footprint: (i32, i32, i32, i32),
    clip: Option<(f64, f64, f64, f64)>,
) -> Option<(i32, i32, i32, i32)> {
    let (fw, fh) = (fb.width() as i32, fb.height() as i32);
    let (mut x1, mut y1, mut x2, mut y2) = footprint;
    x1 = x1.max(0);
    y1 = y1.max(0);
    x2 = x2.min(fw);
    y2 = y2.min(fh);
    if let Some((cx, cy, cw, ch)) = clip {
        x1 = x1.max(cx.floor() as i32);
        y1 = y1.max(cy.floor() as i32);
        x2 = x2.min((cx + cw).ceil() as i32);
        y2 = y2.min((cy + ch).ceil() as i32);
    }
    (x1 < x2 && y1 < y2).then_some((x1, y1, x2, y2))
}

/// True when some of `footprint` lands on `fb` through `clip` (see
/// [`landing_footprint`]): a composite that does not draws nothing on either
/// path, so the caller skips it before building anything.
pub(super) fn footprint_lands(
    fb: &Framebuffer,
    footprint: (i32, i32, i32, i32),
    clip: Option<(f64, f64, f64, f64)>,
) -> bool {
    landing_footprint(fb, footprint, clip).is_some()
}

/// True when every pixel of `fb` inside `footprint` that `clip` leaves has
/// alpha 255; false when any does not, or when nothing of the footprint lands
/// (the fallback draws nothing either).  Port of agg-sharp's
/// `ImageGraphics2D.IsOpaqueUnder`, scanning only the clipped pixels as its
/// `CompositeLcdBuffer` does.
pub(super) fn opaque_under(
    fb: &Framebuffer,
    footprint: (i32, i32, i32, i32),
    clip: Option<(f64, f64, f64, f64)>,
) -> bool {
    let Some((x1, y1, x2, y2)) = landing_footprint(fb, footprint, clip) else {
        return false;
    };
    let fw = fb.width() as i32;
    let px = fb.pixels();
    (y1..y2).all(|y| (x1..x2).all(|x| px[((y * fw + x) * 4 + 3) as usize] == 255))
}

/// The collapsed, single-alpha form of top-row-first LCD planes as a
/// premultiplied Y-up [`Framebuffer`] (what a destination that cannot hold
/// three channel alphas takes): [`collapse_lcd_planes`] premultiplied the
/// way `draw_image_rgba` does.  `None` when the planes are too short.
pub(super) fn collapsed_lcd_framebuffer(
    color: &[u8],
    alpha: &[u8],
    w: u32,
    h: u32,
) -> Option<Framebuffer> {
    let rgba = crate::draw_ctx::defaults::collapse_lcd_planes(color, alpha, w, h)?;
    let mut out = Framebuffer::new(w, h);
    let row = w as usize * 4;
    for (y, dst_row) in out.pixels_mut().chunks_exact_mut(row).enumerate() {
        let src_row = &rgba[(h as usize - 1 - y) * row..(h as usize - y) * row];
        for (d, s) in dst_row.chunks_exact_mut(4).zip(src_row.chunks_exact(4)) {
            let a = s[3] as u32;
            d[3] = s[3];
            for k in 0..3 {
                d[k] = if a == 255 {
                    s[k]
                } else {
                    (((s[k] as u32) * a + 127) / 255) as u8
                };
            }
        }
    }
    Some(out)
}

/// Per-channel premultiplied src-over of top-row-first LCD planes (`color`
/// premultiplied, `alpha` per channel, 3 bytes per pixel, `w` x `h`) onto
/// `fb`, their bottom-left at pixel `(sx, sy)`, `ga` scaling colour and alpha
/// alike, `clip` bounding the writes.  agg-sharp's `LcdBuffer.CompositeOnto`:
/// the ungated primitive.  `GfxCtx::draw_lcd_backbuffer_arc` calls it only
/// onto opaque pixels, since the destination's single alpha takes the
/// strongest channel's and a pixel that is not opaque cannot hold three
/// channel coverages.
#[allow(clippy::too_many_arguments)]
pub(crate) fn composite_lcd_planes_per_channel(
    fb: &mut Framebuffer,
    color: &[u8],
    alpha: &[u8],
    w: u32,
    h: u32,
    sx: i32,
    sy: i32,
    ga: f32,
    clip: Option<(f64, f64, f64, f64)>,
) {
    let (w_u, h_u) = (w as usize, h as usize);
    if color.len() < w_u * h_u * 3 || alpha.len() < w_u * h_u * 3 {
        return;
    }
    let fw = fb.width() as i32;
    let fh = fb.height() as i32;
    let (cx1, cy1, cx2, cy2) = match clip {
        Some((cx, cy, cw, ch)) => (
            cx.floor() as i32,
            cy.floor() as i32,
            (cx + cw).ceil() as i32,
            (cy + ch).ceil() as i32,
        ),
        None => (0, 0, fw, fh),
    };
    let fw_u = fw as usize;
    let pixels = fb.pixels_mut();

    for src_y in 0..h_u {
        // Top-row-first src → Y-up dst: src row 0 (visually top)
        // lands at dst_y + h - 1 (the visually-top dst row).
        let dy = sy + (h_u - 1 - src_y) as i32;
        if dy < 0 || dy >= fh || dy < cy1 || dy >= cy2 {
            continue;
        }
        let dy_u = dy as usize;
        for src_x in 0..w_u {
            let dx = sx + src_x as i32;
            if dx < 0 || dx >= fw || dx < cx1 || dx >= cx2 {
                continue;
            }
            let ci = (src_y * w_u + src_x) * 3;

            let sa_r = (alpha[ci] as f32 / 255.0) * ga;
            let sa_g = (alpha[ci + 1] as f32 / 255.0) * ga;
            let sa_b = (alpha[ci + 2] as f32 / 255.0) * ga;
            if sa_r == 0.0 && sa_g == 0.0 && sa_b == 0.0 {
                continue;
            }

            let sc_r = (color[ci] as f32 / 255.0) * ga;
            let sc_g = (color[ci + 1] as f32 / 255.0) * ga;
            let sc_b = (color[ci + 2] as f32 / 255.0) * ga;

            let di = (dy_u * fw_u + dx as usize) * 4;
            // Framebuffer holds premultiplied RGBA.  Per-channel
            // src-over is `dst = src + dst * (1 - src_a)` since src
            // is already premultiplied.  Alpha composites via
            // max-channel-alpha so the destination picks up full
            // opacity wherever any subpixel was painted — matches
            // "this pixel was drawn on" for subsequent SrcOver blits.
            let dc_r = pixels[di] as f32 / 255.0;
            let dc_g = pixels[di + 1] as f32 / 255.0;
            let dc_b = pixels[di + 2] as f32 / 255.0;
            let da = pixels[di + 3] as f32 / 255.0;

            let rc_r = sc_r + dc_r * (1.0 - sa_r);
            let rc_g = sc_g + dc_g * (1.0 - sa_g);
            let rc_b = sc_b + dc_b * (1.0 - sa_b);
            let src_a_max = sa_r.max(sa_g).max(sa_b);
            let ra = src_a_max + da * (1.0 - src_a_max);

            pixels[di] = (rc_r * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            pixels[di + 1] = (rc_g * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            pixels[di + 2] = (rc_b * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            pixels[di + 3] = (ra * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        }
    }
}
