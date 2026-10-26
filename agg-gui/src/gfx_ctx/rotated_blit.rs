//! Software image blit under a rotated or skewed CTM, for `GfxCtx`'s image
//! draws (`draw_impl.rs`): the destination rect becomes a transformed quad,
//! sampled at each visible device pixel centre inside it with the caller's
//! [`ImageSampling`], so the result lands where the wgpu backend's textured
//! quad does. The axis-aligned case lives in `axis_blit.rs`, which maps it
//! exactly and supplies the shared window and sampler.

use agg_rust::trans_affine::TransAffine;

use super::axis_blit::{sample_premultiplied, BlitWindow};
use crate::draw_ctx::ImageSampling;
use crate::framebuffer::Framebuffer;

/// The visible device-space footprint of a rotated blit: an RGBA
/// (premultiplied) framebuffer covering the quad's bounding box narrowed to
/// `window`, and the device pixel of its bottom-left corner. `None` when no
/// visible pixel is covered or the CTM is singular.
// The blit's arguments mirror `DrawCtx::draw_image_rgba_arc_sampled`.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_rotated(
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
    if dst_w <= 0.0 || dst_h <= 0.0 || t.determinant().abs() < 1e-12 {
        return None;
    }
    let corners = [
        (dst_x, dst_y),
        (dst_x + dst_w, dst_y),
        (dst_x, dst_y + dst_h),
        (dst_x + dst_w, dst_y + dst_h),
    ];
    let (mut min_x, mut min_y) = (f64::MAX, f64::MAX);
    let (mut max_x, mut max_y) = (f64::MIN, f64::MIN);
    for (cx, cy) in corners {
        let (mut x, mut y) = (cx, cy);
        t.transform(&mut x, &mut y);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    let (x0, x1) = window.cols(min_x.floor(), max_x.ceil())?;
    let (y0, y1) = window.rows(min_y.floor(), max_y.ceil())?;
    // The window lies inside the target, so these sizes and origins fit.
    let (x0, y0) = (x0 as i32, y0 as i32);
    let out_w = (x1 as i32 - x0) as u32;
    let out_h = (y1 as i32 - y0) as u32;
    let mut out = Framebuffer::new(out_w, out_h);
    let px = out.pixels_mut();
    for dy in 0..out_h {
        for dx in 0..out_w {
            // The device pixel centre, back in the blit's local space.
            let mut x = (x0 + dx as i32) as f64 + 0.5;
            let mut y = (y0 + dy as i32) as f64 + 0.5;
            t.inverse_transform(&mut x, &mut y);
            let u = (x - dst_x) / dst_w;
            let v = (y - dst_y) / dst_h;
            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                continue;
            }
            // The image is top-row-first; local space is Y-up.
            let p = sample_premultiplied(
                data,
                img_w,
                img_h,
                u * img_w as f64,
                (1.0 - v) * img_h as f64,
                sampling,
            );
            let di = ((dy * out_w + dx) * 4) as usize;
            px[di..di + 4].copy_from_slice(&p);
        }
    }
    Some((out, x0, y0))
}
