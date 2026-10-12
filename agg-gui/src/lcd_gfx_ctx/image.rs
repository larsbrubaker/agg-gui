//! Image blitting helpers for `LcdGfxCtx`.
//!
//! Images carry color data, not coverage, so they composite into the LCD
//! buffer with the source alpha applied equally to all three subpixel channels.
//! Two-plane LCD images (a nested `LcdCoverage` backbuffer, an SVG icon's
//! subpixel composite) carry coverage per channel, and
//! [`composite_lcd_planes`] merges them plane into plane.

use agg_rust::trans_affine::TransAffine;

use crate::lcd_coverage::{rect_to_pixel_clip, LcdBuffer};

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_image_rgba(
    buffer: &mut LcdBuffer,
    data: &[u8],
    img_w: u32,
    img_h: u32,
    dst_x: f64,
    dst_y: f64,
    dst_w: f64,
    dst_h: f64,
    transform: &TransAffine,
    global_alpha: f32,
    clip: Option<(f64, f64, f64, f64)>,
) {
    if img_w == 0 || img_h == 0 || dst_w <= 0.0 || dst_h <= 0.0 {
        return;
    }
    if data.len() < (img_w as usize) * (img_h as usize) * 4 {
        return;
    }

    let ox = (dst_x * transform.sx + dst_y * transform.shx + transform.tx).round() as i32;
    let oy = (dst_x * transform.shy + dst_y * transform.sy + transform.ty).round() as i32;
    let scaled_w = ((dst_w * transform.sx).abs()).round() as i32;
    let scaled_h = ((dst_h * transform.sy).abs()).round() as i32;
    if scaled_w <= 0 || scaled_h <= 0 {
        return;
    }

    let buf_w = buffer.width() as i32;
    let buf_h = buffer.height() as i32;
    let (cx1, cy1, cx2, cy2) = match clip.map(rect_to_pixel_clip) {
        Some((x1, y1, x2, y2)) => (x1.max(0), y1.max(0), x2.min(buf_w), y2.min(buf_h)),
        None => (0, 0, buf_w, buf_h),
    };
    if cx1 >= cx2 || cy1 >= cy2 {
        return;
    }

    let buf_w_u = buf_w as usize;
    let img_w_u = img_w as usize;
    let alpha = global_alpha.clamp(0.0, 1.0);
    let (color_plane, alpha_plane) = buffer.planes_mut();

    for ly in 0..scaled_h {
        let dy = oy + ly;
        if dy < cy1 || dy >= cy2 {
            continue;
        }
        let frac_y = (ly as f64 + 0.5) / (scaled_h as f64);
        let sy_visual = ((frac_y * img_h as f64) as u32).min(img_h - 1);
        let sy_storage = (img_h - 1 - sy_visual) as usize;

        for lx in 0..scaled_w {
            let dx = ox + lx;
            if dx < cx1 || dx >= cx2 {
                continue;
            }
            let frac_x = (lx as f64 + 0.5) / (scaled_w as f64);
            let sx_storage = ((frac_x * img_w as f64) as u32).min(img_w - 1) as usize;
            let si = (sy_storage * img_w_u + sx_storage) * 4;
            let sa = (data[si + 3] as f32 / 255.0) * alpha;
            if sa <= 0.0 {
                continue;
            }

            let sr = (data[si] as f32 / 255.0) * sa;
            let sg = (data[si + 1] as f32 / 255.0) * sa;
            let sb = (data[si + 2] as f32 / 255.0) * sa;
            let di = ((dy as usize) * buf_w_u + (dx as usize)) * 3;

            let bc_r = color_plane[di] as f32 / 255.0;
            let bc_g = color_plane[di + 1] as f32 / 255.0;
            let bc_b = color_plane[di + 2] as f32 / 255.0;
            let ba_r = alpha_plane[di] as f32 / 255.0;
            let ba_g = alpha_plane[di + 1] as f32 / 255.0;
            let ba_b = alpha_plane[di + 2] as f32 / 255.0;

            color_plane[di] = ((sr + bc_r * (1.0 - sa)) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            color_plane[di + 1] = ((sg + bc_g * (1.0 - sa)) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            color_plane[di + 2] = ((sb + bc_b * (1.0 - sa)) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            alpha_plane[di] = ((sa + ba_r * (1.0 - sa)) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            alpha_plane[di + 1] = ((sa + ba_g * (1.0 - sa)) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            alpha_plane[di + 2] = ((sa + ba_b * (1.0 - sa)) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        }
    }
}

/// Composite two top-row-first LCD planes (`color` premultiplied, `alpha`
/// per channel, 3 bytes per pixel, `w × h`) into `buffer` 1:1 with their
/// bottom-left at device `(dev_x, dev_y)` (rounded onto the pixel grid),
/// per channel: `dst.c := src.c + dst.c × (1 − src.a_c)` on both planes —
/// agg-sharp's `LcdBufferGraphics2D.CompositeLcdBuffer` (`LcdBuffer.
/// CompositeBuffer`), so a nested LCD backbuffer or an LCD icon keeps every
/// subpixel alpha.  `global_alpha` scales colour and alpha alike (as
/// `GfxCtx::draw_lcd_backbuffer_arc`), and `clip` bounds the writes.
#[allow(clippy::too_many_arguments)]
pub(super) fn composite_lcd_planes(
    buffer: &mut LcdBuffer,
    color: &[u8],
    alpha: &[u8],
    w: u32,
    h: u32,
    dev_x: f64,
    dev_y: f64,
    global_alpha: f32,
    clip: Option<(f64, f64, f64, f64)>,
) {
    let (w_u, h_u) = (w as usize, h as usize);
    if w == 0 || h == 0 || color.len() < w_u * h_u * 3 || alpha.len() < w_u * h_u * 3 {
        return;
    }
    let buf_w = buffer.width() as i32;
    let buf_h = buffer.height() as i32;
    let (ox, oy) = (dev_x.round(), dev_y.round());
    // Cull in f64 first so an enormous offset cannot overflow `origin + row`.
    if !ox.is_finite()
        || !oy.is_finite()
        || ox >= buf_w as f64
        || oy >= buf_h as f64
        || ox + w as f64 <= 0.0
        || oy + h as f64 <= 0.0
    {
        return;
    }
    let (ox, oy) = (ox as i32, oy as i32);
    let (cx1, cy1, cx2, cy2) = match clip.map(rect_to_pixel_clip) {
        Some((x1, y1, x2, y2)) => (x1.max(0), y1.max(0), x2.min(buf_w), y2.min(buf_h)),
        None => (0, 0, buf_w, buf_h),
    };
    let ga = global_alpha.clamp(0.0, 1.0);
    let buf_w_u = buf_w as usize;
    let (color_plane, alpha_plane) = buffer.planes_mut();
    for src_y in 0..h_u {
        // Top-row-first source → Y-up buffer: source row 0 is the top row.
        let dy = oy + (h_u - 1 - src_y) as i32;
        if dy < cy1 || dy >= cy2 {
            continue;
        }
        for src_x in 0..w_u {
            let dx = ox + src_x as i32;
            if dx < cx1 || dx >= cx2 {
                continue;
            }
            let si = (src_y * w_u + src_x) * 3;
            if alpha[si] == 0 && alpha[si + 1] == 0 && alpha[si + 2] == 0 {
                continue;
            }
            let di = (dy as usize * buf_w_u + dx as usize) * 3;
            for ch in 0..3 {
                let sa = alpha[si + ch] as f32 / 255.0 * ga;
                let sc = color[si + ch] as f32 / 255.0 * ga;
                let bc = color_plane[di + ch] as f32 / 255.0;
                let ba = alpha_plane[di + ch] as f32 / 255.0;
                color_plane[di + ch] =
                    ((sc + bc * (1.0 - sa)) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                alpha_plane[di + ch] =
                    ((sa + ba * (1.0 - sa)) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }
}
