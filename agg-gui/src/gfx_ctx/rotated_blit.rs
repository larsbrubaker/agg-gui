//! Software image blit under a rotated or skewed CTM, for
//! `GfxCtx::draw_image_rgba` (`draw_impl.rs`): the destination rect becomes
//! a transformed quad, sampled nearest-neighbour at each device pixel centre
//! inside it, so the result lands where the wgpu backend's textured quad
//! does (agg-sharp `Graphics2D.Render(image, x, y, angle, sx, sy)`). The
//! axis-aligned case stays in `draw_impl.rs`, which maps it exactly.

use agg_rust::trans_affine::TransAffine;

use crate::framebuffer::Framebuffer;

/// The device-space footprint of a rotated blit: an RGBA (premultiplied)
/// framebuffer covering the quad's bounding box, and the device pixel of
/// its bottom-left corner. `None` when the quad covers no pixel centre or
/// the CTM is singular.
// The blit's arguments mirror `DrawCtx::draw_image_rgba`.
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
    let x0 = min_x.floor() as i32;
    let y0 = min_y.floor() as i32;
    let out_w = (max_x.ceil() as i32 - x0).max(0) as u32;
    let out_h = (max_y.ceil() as i32 - y0).max(0) as u32;
    if out_w == 0 || out_h == 0 {
        return None;
    }
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
            let sx = ((u * img_w as f64) as u32).min(img_w - 1);
            let sy = (((1.0 - v) * img_h as f64) as u32).min(img_h - 1);
            let si = ((sy * img_w + sx) * 4) as usize;
            let di = ((dy * out_w + dx) * 4) as usize;
            if si + 3 >= data.len() {
                continue;
            }
            let a = data[si + 3] as u32;
            // Premultiply, as the axis-aligned path does: (c * a + 127) / 255.
            for c in 0..3 {
                px[di + c] = (((data[si + c] as u32) * a + 127) / 255) as u8;
            }
            px[di + 3] = a as u8;
        }
    }
    Some((out, x0, y0))
}
