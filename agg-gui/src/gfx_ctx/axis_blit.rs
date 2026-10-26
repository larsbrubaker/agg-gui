//! Software image blit under an axis-aligned CTM, for `GfxCtx`'s image draws
//! (`draw_impl.rs`), plus the pieces it shares with the rotated blit
//! (`rotated_blit.rs`): the visible device window a blit may write and the
//! per-pixel sampler that honours [`ImageSampling`].
//!
//! Only device pixels inside the active clip and the target are sampled, so a
//! picture drawn zoomed far past its frame costs what is visible, not what the
//! destination rect covers.

use agg_rust::trans_affine::TransAffine;

use crate::draw_ctx::ImageSampling;
use crate::framebuffer::Framebuffer;

/// The half-open device-pixel window `x0..x1 × y0..y1` (Y-up) a blit may
/// write: the target's bounds narrowed by the active clip, rounded outward
/// the way `composite_framebuffers` scissors.
#[derive(Clone, Copy, Debug)]
pub(super) struct BlitWindow {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}

impl BlitWindow {
    pub(super) fn new(target_w: u32, target_h: u32, clip: Option<(f64, f64, f64, f64)>) -> Self {
        let mut w = BlitWindow {
            x0: 0,
            y0: 0,
            x1: target_w as i64,
            y1: target_h as i64,
        };
        if let Some((cx, cy, cw, ch)) = clip {
            // `as i64` saturates on an infinite or huge clip; the window then
            // stays the target's bounds on that side.
            w.x0 = w.x0.max(cx.floor() as i64);
            w.y0 = w.y0.max(cy.floor() as i64);
            w.x1 = w.x1.min((cx + cw).ceil() as i64);
            w.y1 = w.y1.min((cy + ch).ceil() as i64);
        }
        w
    }

    /// `lo..hi` narrowed to the window's columns, or `None` when empty.
    pub(super) fn cols(&self, lo: f64, hi: f64) -> Option<(i64, i64)> {
        span(lo, hi, self.x0, self.x1)
    }

    pub(super) fn rows(&self, lo: f64, hi: f64) -> Option<(i64, i64)> {
        span(lo, hi, self.y0, self.y1)
    }
}

fn span(lo: f64, hi: f64, w0: i64, w1: i64) -> Option<(i64, i64)> {
    if !lo.is_finite() || !hi.is_finite() {
        return None;
    }
    // Clamped to the window first, so the casts below stay in range.
    let a = lo.max(w0 as f64).min(w1 as f64) as i64;
    let b = hi.max(w0 as f64).min(w1 as f64) as i64;
    (a < b).then_some((a, b))
}

/// The premultiplied RGBA colour of a straight-alpha, top-row-first image at
/// image position `(u, v)` in pixels (`u` from the left edge, `v` from the
/// top edge; pixel `i` spans `i..i+1`).
///
/// `Nearest` (and the software backend's `Auto`) takes the one pixel the
/// position falls in. `Smooth` blends the four pixels around it bilinearly,
/// weighting each by its alpha so a transparent pixel adds no colour.
pub(super) fn sample_premultiplied(
    data: &[u8],
    img_w: u32,
    img_h: u32,
    u: f64,
    v: f64,
    sampling: ImageSampling,
) -> [u8; 4] {
    let texel = |x: u32, y: u32| -> [u8; 4] {
        let i = ((y as usize) * (img_w as usize) + x as usize) * 4;
        match data.get(i..i + 4) {
            Some(p) => [p[0], p[1], p[2], p[3]],
            None => [0; 4],
        }
    };
    match sampling {
        ImageSampling::Auto | ImageSampling::Nearest => {
            let sx = u.floor().clamp(0.0, (img_w - 1) as f64) as u32;
            let sy = v.floor().clamp(0.0, (img_h - 1) as f64) as u32;
            let p = texel(sx, sy);
            let a = p[3] as u32;
            if a == 255 {
                return p;
            }
            // Premultiply: (c * a + 127) / 255 (round-half-up).
            let pm = |c: u8| (((c as u32) * a + 127) / 255) as u8;
            [pm(p[0]), pm(p[1]), pm(p[2]), a as u8]
        }
        ImageSampling::Smooth => {
            // Texel centres sit at `i + 0.5`; clamping to the outer centres
            // keeps the edge pixels' colour out to the image's edge.
            let fu = (u - 0.5).clamp(0.0, (img_w - 1) as f64);
            let fv = (v - 0.5).clamp(0.0, (img_h - 1) as f64);
            let (x0, y0) = (fu.floor() as u32, fv.floor() as u32);
            let (x1, y1) = ((x0 + 1).min(img_w - 1), (y0 + 1).min(img_h - 1));
            let (tx, ty) = (fu - x0 as f64, fv - y0 as f64);
            let mut acc = [0.0f64; 4];
            for (x, y, wgt) in [
                (x0, y0, (1.0 - tx) * (1.0 - ty)),
                (x1, y0, tx * (1.0 - ty)),
                (x0, y1, (1.0 - tx) * ty),
                (x1, y1, tx * ty),
            ] {
                let p = texel(x, y);
                let a = p[3] as f64 / 255.0;
                for c in 0..3 {
                    acc[c] += wgt * p[c] as f64 * a;
                }
                acc[3] += wgt * p[3] as f64;
            }
            acc.map(|c| c.round().clamp(0.0, 255.0) as u8)
        }
    }
}

/// The visible part of an axis-aligned blit: a premultiplied framebuffer and
/// the device pixel of its bottom-left corner, or `None` when no visible
/// pixel is covered.
///
/// The destination rect maps through the CTM to its device footprint; the
/// footprint is rounded to whole pixels starting at the first pixel whose
/// centre lies inside it (the half-open rule the GPU rasterizer uses), and
/// each pixel samples the image at its centre's position within that
/// footprint, mirrored when the CTM flips an axis.
// The blit's arguments mirror `DrawCtx::draw_image_rgba_arc_sampled`.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_axis_aligned(
    t: &TransAffine,
    data: &[u8],
    img_w: u32,
    img_h: u32,
    dst_x: f64,
    dst_y: f64,
    dst_w: f64,
    dst_h: f64,
    sampling: ImageSampling,
    window: BlitWindow,
) -> Option<(Framebuffer, i32, i32)> {
    let x_a = dst_x * t.sx + t.tx;
    let x_b = (dst_x + dst_w) * t.sx + t.tx;
    let y_a = dst_y * t.sy + t.ty;
    let y_b = (dst_y + dst_h) * t.sy + t.ty;
    let (flip_x, flip_y) = (x_b < x_a, y_b < y_a);
    let (left, right) = (x_a.min(x_b), x_a.max(x_b));
    let (bottom, top) = (y_a.min(y_b), y_a.max(y_b));
    let out_w = (right - left).round();
    let out_h = (top - bottom).round();
    if !(out_w >= 1.0 && out_h >= 1.0) {
        return None;
    }
    let screen_x = (left - 0.5).ceil();
    let screen_y = (bottom - 0.5).ceil();
    let (c0, c1) = window.cols(screen_x, screen_x + out_w)?;
    let (r0, r1) = window.rows(screen_y, screen_y + out_h)?;
    // The window lies inside the target, so these sizes and origins fit.
    let (vis_w, vis_h) = ((c1 - c0) as u32, (r1 - r0) as u32);
    let mut out = Framebuffer::new(vis_w, vis_h);
    let px = out.pixels_mut();
    for ly in 0..vis_h {
        // Sample at the destination pixel centre (exact at 1:1).
        let fy = ((r0 - screen_y as i64) as f64 + ly as f64 + 0.5) / out_h;
        let fy = if flip_y { 1.0 - fy } else { fy };
        // Image is top-row-first; Y-up dst means we flip v.
        let v = (1.0 - fy) * img_h as f64;
        for lx in 0..vis_w {
            let fx = ((c0 - screen_x as i64) as f64 + lx as f64 + 0.5) / out_w;
            let fx = if flip_x { 1.0 - fx } else { fx };
            let u = fx * img_w as f64;
            let di = ((ly * vis_w + lx) * 4) as usize;
            let p = sample_premultiplied(data, img_w, img_h, u, v, sampling);
            px[di..di + 4].copy_from_slice(&p);
        }
    }
    Some((out, c0 as i32, r0 as i32))
}
