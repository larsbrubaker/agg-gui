//! Smooth resampling of an SVG `<image>` raster to its device-pixel size, for
//! `render_tree.rs`'s `render_image`.
//!
//! resvg draws a raster with bicubic filtering for `image-rendering`
//! `optimizeQuality`/`high-quality` (the default), bilinear for `smooth`, and
//! nearest-neighbour for `optimizeSpeed`, `crisp-edges` and `pixelated`.
//! `DrawCtx::draw_image_rgba` samples nearest-neighbour (exact for the 1:1
//! blits widgets do), so a smoothed image is resampled here, bilinearly on
//! premultiplied colour with edge texels clamped, to exactly the device size
//! it is then blitted 1:1 at.

/// Largest resampled image, in pixels; beyond it the image is drawn unsmoothed
/// rather than allocating an outsized scratch buffer.
const MAX_RESAMPLED_PIXELS: u64 = 4096 * 4096;

/// Whether `mode` asks for a smoothed image.
pub(super) fn is_smooth(mode: usvg::ImageRendering) -> bool {
    matches!(
        mode,
        usvg::ImageRendering::OptimizeQuality
            | usvg::ImageRendering::Smooth
            | usvg::ImageRendering::HighQuality
    )
}

/// `data` (straight RGBA, `img_w` x `img_h`, top row first) resampled to
/// `out_w` x `out_h` with bilinear filtering; `None` when either size is empty
/// or the result would exceed [`MAX_RESAMPLED_PIXELS`].
pub(super) fn resample_bilinear(
    data: &[u8],
    img_w: u32,
    img_h: u32,
    out_w: u32,
    out_h: u32,
) -> Option<Vec<u8>> {
    if img_w == 0 || img_h == 0 || out_w == 0 || out_h == 0 {
        return None;
    }
    if u64::from(out_w) * u64::from(out_h) > MAX_RESAMPLED_PIXELS
        || data.len() < (img_w as usize) * (img_h as usize) * 4
    {
        return None;
    }

    // Premultiplied texels, so transparent neighbours do not bleed colour.
    let premultiplied: Vec<[f32; 4]> = data
        .chunks_exact(4)
        .take((img_w * img_h) as usize)
        .map(|p| {
            let a = p[3] as f32 / 255.0;
            [
                p[0] as f32 * a,
                p[1] as f32 * a,
                p[2] as f32 * a,
                p[3] as f32,
            ]
        })
        .collect();
    let texel = |x: i64, y: i64| -> [f32; 4] {
        let x = x.clamp(0, img_w as i64 - 1) as usize;
        let y = y.clamp(0, img_h as i64 - 1) as usize;
        premultiplied[y * img_w as usize + x]
    };

    let mut out = vec![0u8; (out_w as usize) * (out_h as usize) * 4];
    let scale_x = img_w as f64 / out_w as f64;
    let scale_y = img_h as f64 / out_h as f64;
    for dy in 0..out_h {
        // The output pixel's centre in source texel space, texel centres at +0.5.
        let sy = (dy as f64 + 0.5) * scale_y - 0.5;
        let y0 = sy.floor();
        let fy = (sy - y0) as f32;
        for dx in 0..out_w {
            let sx = (dx as f64 + 0.5) * scale_x - 0.5;
            let x0 = sx.floor();
            let fx = (sx - x0) as f32;
            let (x0, y0) = (x0 as i64, y0 as i64);
            let (a, b) = (texel(x0, y0), texel(x0 + 1, y0));
            let (c, d) = (texel(x0, y0 + 1), texel(x0 + 1, y0 + 1));
            let mut px = [0f32; 4];
            for i in 0..4 {
                let top = a[i] + (b[i] - a[i]) * fx;
                let bottom = c[i] + (d[i] - c[i]) * fx;
                px[i] = top + (bottom - top) * fy;
            }
            let o = ((dy * out_w + dx) * 4) as usize;
            let alpha = px[3].clamp(0.0, 255.0);
            out[o + 3] = alpha.round() as u8;
            if alpha > 0.0 {
                let k = 255.0 / alpha;
                for i in 0..3 {
                    out[o + i] = (px[i] * k).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    Some(out)
}
