//! Software `GfxCtx` tests for `DrawCtx::draw_image_rgba_arc_sampled`: a
//! picture the user zooms into draws its real pixels as hard-edged squares
//! with `Nearest`, `Auto` keeps the software backend's nearest blit, `Smooth`
//! blends, and a zoomed picture far larger than its clip samples only what is
//! visible. The wgpu backend's counterpart is
//! `agg-gui-wgpu/src/image_sampling_readback_tests.rs`.

use super::*;
use crate::draw_ctx::{DrawCtx, ImageSampling};
use std::sync::Arc;

const SIZE: u32 = 64;
const IMG: u32 = 512;
/// Magnification with no device pixel centre on an image-pixel edge (with
/// `DST_X` an integer, `10 x + 405 = 64 k` has no solution: odd ≠ even).
const ZOOM: f64 = 6.4;
const DST_X: f64 = -40.0;
const DST_Y: f64 = -300.0;
const CLIP: (f64, f64, f64, f64) = (8.0, 8.0, 48.0, 48.0);
const GREEN: [u8; 4] = [0, 255, 0, 255];

/// 512×512 opaque vertical one-pixel stripes: even columns black, odd white.
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

/// Draw the whole 512 px picture zoomed past a 48×48 clip on a green target.
fn draw_zoomed(sampling: ImageSampling, zoom: f64) -> Framebuffer {
    let img = stripes(IMG);
    let mut fb = Framebuffer::new(SIZE, SIZE);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::rgba(0.0, 1.0, 0.0, 1.0));
    ctx.clip_rect(CLIP.0, CLIP.1, CLIP.2, CLIP.3);
    let side = IMG as f64 * zoom;
    ctx.draw_image_rgba_arc_sampled(&img, IMG, IMG, DST_X, DST_Y, side, side, sampling);
    drop(ctx);
    fb
}

/// Every visible pixel is exactly its image pixel's colour, so colour changes
/// only where an image-pixel edge crosses the row; outside the clip the
/// target is untouched.
fn assert_hard_edged(fb: &Framebuffer, mode: &str) {
    for y in [10, 30, 53] {
        for x in 8..56 {
            assert_eq!(sample(fb, x, y), expected(x), "{mode}: pixel ({x}, {y})");
        }
    }
    for (x, y) in [(4, 30), (60, 30), (30, 4), (30, 60)] {
        assert_eq!(
            sample(fb, x, y),
            GREEN,
            "{mode}: outside the clip ({x}, {y})"
        );
    }
}

#[test]
fn nearest_magnifies_a_large_image_with_hard_edged_pixels() {
    let fb = draw_zoomed(ImageSampling::Nearest, ZOOM);
    assert_hard_edged(&fb, "Nearest");
}

#[test]
fn auto_keeps_the_software_nearest_blit() {
    let fb = draw_zoomed(ImageSampling::Auto, ZOOM);
    assert_hard_edged(&fb, "Auto");
    // `draw_image_rgba_arc` draws exactly what `Auto` does.
    let img = stripes(IMG);
    let mut plain = Framebuffer::new(SIZE, SIZE);
    let mut ctx = GfxCtx::new(&mut plain);
    ctx.clear(Color::rgba(0.0, 1.0, 0.0, 1.0));
    ctx.clip_rect(CLIP.0, CLIP.1, CLIP.2, CLIP.3);
    let side = IMG as f64 * ZOOM;
    ctx.draw_image_rgba_arc(&img, IMG, IMG, DST_X, DST_Y, side, side);
    drop(ctx);
    assert_eq!(plain.pixels(), fb.pixels());
}

#[test]
fn smooth_blends_neighbouring_pixels() {
    let fb = draw_zoomed(ImageSampling::Smooth, ZOOM);
    let blended = (8..56)
        .map(|x| sample(&fb, x, 30))
        .filter(|p| (40..=215).contains(&p[0]))
        .count();
    assert!(blended > 0, "Smooth must blend across stripe edges");
    assert_eq!(sample(&fb, 4, 30), GREEN, "Smooth honours the clip");
}

/// Zoomed 400× the picture covers 204 800 px a side, a footprint of about
/// 168 GB; only the 48×48 visible window may be sampled.
#[test]
fn a_hugely_zoomed_picture_samples_only_the_visible_window() {
    let zoom = 400.0;
    let fb = draw_zoomed(ImageSampling::Nearest, zoom);
    for x in 8..56 {
        let col = ((x as f64 + 0.5 - DST_X) / zoom).floor() as i64;
        let c = if col % 2 == 0 { 0 } else { 255 };
        assert_eq!(sample(&fb, x, 30), [c, c, c, 255], "pixel ({x}, 30)");
    }
    assert_eq!(sample(&fb, 60, 30), GREEN);
}

/// Under a rotated CTM the quad blit honours the sampling mode too: a quarter
/// turn keeps every visible pixel one image pixel's colour with `Nearest`.
#[test]
fn nearest_holds_under_a_rotated_transform() {
    let img = stripes(IMG);
    let mut fb = Framebuffer::new(SIZE, SIZE);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::rgba(0.0, 1.0, 0.0, 1.0));
    ctx.clip_rect(CLIP.0, CLIP.1, CLIP.2, CLIP.3);
    ctx.translate(SIZE as f64, 0.0);
    ctx.rotate(0.3);
    let side = IMG as f64 * ZOOM;
    ctx.draw_image_rgba_arc_sampled(
        &img,
        IMG,
        IMG,
        -side / 2.0,
        -side / 2.0,
        side,
        side,
        ImageSampling::Nearest,
    );
    drop(ctx);
    for x in 8..56 {
        let p = sample(&fb, x, 30);
        assert!(
            p == [0, 0, 0, 255] || p == [255, 255, 255, 255],
            "rotated Nearest pixel ({x}, 30) must be a stripe colour; got {p:?}"
        );
    }
    assert_eq!(sample(&fb, 60, 30), GREEN);
}
