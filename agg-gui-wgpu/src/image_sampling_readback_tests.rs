//! Headless GPU readback tests for `DrawCtx::draw_image_rgba_arc_sampled` on
//! the wgpu backend (`image_blit.rs`): a picture the user zooms into draws its
//! real pixels as hard-edged squares with `Nearest` at any image size, `Auto`
//! keeps the size rule (small images hard-edged, 256 px and up smooth),
//! `Smooth` blends, the zoomed quad is scissored by the clip, and switching
//! modes reuses the one cached texture, mip chain included.  The software
//! backend's counterpart is `agg-gui/src/tests/image_sampling.rs`.
//!
//! Like the other readback tests, these skip (pass trivially) when no GPU
//! adapter is available.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::{DrawCtx, ImageSampling};

use crate::layer_text_readback_tests::{px, try_device, Target};
use crate::WgpuGfxCtx;

const SIZE: u32 = 64;
/// Magnification with no device pixel centre on an image-pixel edge (with
/// `DST_X` an integer, `10 x + 405 = 64 k` has no solution: odd ≠ even).
const ZOOM: f64 = 6.4;
const DST_X: f64 = -40.0;
const DST_Y: f64 = -300.0;
const CLIP: (f64, f64, f64, f64) = (8.0, 8.0, 48.0, 48.0);
const GREEN: [u8; 4] = [0, 255, 0, 255];

/// `n`×`n` opaque vertical one-pixel stripes: even columns black, odd white.
fn stripes(n: u32) -> Arc<Vec<u8>> {
    let mut data = Vec::with_capacity((n * n * 4) as usize);
    for _y in 0..n {
        for x in 0..n {
            let c = if x % 2 == 0 { 0 } else { 255 };
            data.extend_from_slice(&[c, c, c, 255]);
        }
    }
    Arc::new(data)
}

/// The stripe colour device column `x` shows when every device pixel takes
/// the image pixel under its centre.
fn expected(x: u32) -> [u8; 4] {
    let col = ((x as f64 + 0.5 - DST_X) / ZOOM).floor() as i64;
    let c = if col % 2 == 0 { 0 } else { 255 };
    [c, c, c, 255]
}

/// Sample in Y-up coordinates (the readback is top-row-first).
fn at(data: &[u8], x: u32, y_up: u32) -> [u8; 4] {
    px(data, SIZE, x, SIZE - 1 - y_up)
}

/// Draw `img` (`n` px a side) zoomed past a 48×48 clip on a green target,
/// once per mode in `modes` (the last one shows), and return the read-back
/// pixels with the context, whose texture cache the caller may inspect.
fn draw_zoomed(
    img: &Arc<Vec<u8>>,
    n: u32,
    modes: &[ImageSampling],
) -> Option<(Vec<u8>, WgpuGfxCtx)> {
    let (device, queue) = try_device()?;
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), SIZE, SIZE);
    let mut ctx = WgpuGfxCtx::new(
        device,
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        SIZE as f32,
        SIZE as f32,
    );
    ctx.reset(SIZE as f32, SIZE as f32);
    ctx.clear(Color::rgba(0.0, 1.0, 0.0, 1.0));
    ctx.clip_rect(CLIP.0, CLIP.1, CLIP.2, CLIP.3);
    let side = n as f64 * ZOOM;
    for &mode in modes {
        ctx.draw_image_rgba_arc_sampled(img, n, n, DST_X, DST_Y, side, side, mode);
    }
    ctx.flush_to_surface(&target.view);
    Some((target.read(), ctx))
}

/// Every visible pixel is exactly its image pixel's colour, so colour changes
/// only where an image-pixel edge crosses the row; outside the clip the
/// target is untouched.
fn assert_hard_edged(data: &[u8], mode: &str) {
    for y in [10, 30, 53] {
        for x in 8..56 {
            assert_eq!(at(data, x, y), expected(x), "{mode}: pixel ({x}, {y})");
        }
    }
    for (x, y) in [(4, 30), (60, 30), (30, 4), (30, 60)] {
        assert_eq!(at(data, x, y), GREEN, "{mode}: outside the clip ({x}, {y})");
    }
}

/// How many visible pixels on a row blend black and white.
fn blended_count(data: &[u8]) -> usize {
    (8..56)
        .map(|x| at(data, x, 30))
        .filter(|p| (40..=215).contains(&p[0]))
        .count()
}

#[test]
fn nearest_magnifies_a_large_image_with_hard_edged_pixels() {
    let img = stripes(512);
    let Some((data, _)) = draw_zoomed(&img, 512, &[ImageSampling::Nearest]) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_hard_edged(&data, "Nearest 512");
}

#[test]
fn auto_draws_a_large_image_smoothly() {
    let img = stripes(512);
    let Some((data, _)) = draw_zoomed(&img, 512, &[ImageSampling::Auto]) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert!(
        blended_count(&data) > 0,
        "Auto keeps linear+mipmap sampling for images of 256 px and up"
    );
    assert_eq!(at(&data, 4, 30), GREEN, "Auto honours the clip");
}

#[test]
fn auto_draws_a_small_image_with_hard_edged_pixels() {
    let img = stripes(128);
    let Some((data, _)) = draw_zoomed(&img, 128, &[ImageSampling::Auto]) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_hard_edged(&data, "Auto 128");
}

#[test]
fn smooth_blends_a_small_image() {
    let img = stripes(128);
    let Some((data, _)) = draw_zoomed(&img, 128, &[ImageSampling::Smooth]) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert!(blended_count(&data) > 0, "Smooth blends at any image size");
}

/// Drawing one picture with every mode uploads it once, and a first draw
/// with `Nearest` still builds the mip chain a later `Smooth` / `Auto`
/// minification samples.
#[test]
fn switching_modes_reuses_one_mipmapped_texture() {
    let img = stripes(512);
    let modes = [
        ImageSampling::Nearest,
        ImageSampling::Smooth,
        ImageSampling::Auto,
        ImageSampling::Nearest,
    ];
    let Some((data, ctx)) = draw_zoomed(&img, 512, &modes) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_hard_edged(&data, "last draw Nearest");
    assert_eq!(ctx.arc_texture_cache.len(), 1, "one texture for every mode");
    let entry = ctx
        .arc_texture_cache
        .values()
        .next()
        .expect("cached texture");
    assert_eq!(
        entry.texture.mip_level_count(),
        10,
        "a 512 px image carries its full mip chain whatever the first mode"
    );
}
