//! Headless GPU readback tests for the image blit paths in `image_blit.rs`.
//!
//! Covers the sampler policy the two Arc-keyed blits use:
//! - `draw_image_rgba_corners` renders an arbitrarily oriented quad, so it must
//!   always sample with LINEAR filtering (nearest produces jagged, shimmering
//!   edges on rotated / scaled sprites).
//! - `draw_image_rgba_arc` keeps its crisp NEAREST behaviour for the 1:1
//!   pixel-aligned `Label` backbuffer blits.
//!
//! Like `clip_path_readback_tests`, these skip (pass trivially) when no GPU
//! adapter is available.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;

use crate::layer_text_readback_tests::{px, try_device, Target};
use crate::WgpuGfxCtx;

const SIZE: u32 = 64;

/// 2×2 RGBA image: left column opaque red, right column opaque blue.
fn red_blue_2x2() -> Arc<Vec<u8>> {
    let mut data = Vec::with_capacity(2 * 2 * 4);
    for _row in 0..2 {
        data.extend_from_slice(&[255, 0, 0, 255]);
        data.extend_from_slice(&[0, 0, 255, 255]);
    }
    Arc::new(data)
}

/// Sample in Y-up coordinates (the readback is top-row-first).
fn at(data: &[u8], x: u32, y_up: u32) -> [u8; 4] {
    px(data, SIZE, x, SIZE - 1 - y_up)
}

/// Run `paint` on a fresh context and return the read-back pixels.
fn render(paint: impl FnOnce(&mut WgpuGfxCtx)) -> Option<Vec<u8>> {
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
    paint(&mut ctx);
    ctx.flush_to_surface(&target.view);
    Some(target.read())
}

/// A corner-quad blit stretches a tiny image over a large area; the seam
/// between the two source texels must be a blend, not a hard nearest-sampled
/// step.  Regression guard for the `nearest: !should_use_mipmaps(..)` policy
/// that used to point-sample every image below `MIPMAP_MIN_DIM`.
#[test]
fn corner_blit_uses_linear_filtering() {
    let img = red_blue_2x2();
    let Some(data) = render(|ctx| {
        // Cover the whole 64×64 target: BL, BR, TR, TL in Y-up local coords.
        ctx.draw_image_rgba_corners(
            &img,
            2,
            2,
            [(0.0, 0.0), (64.0, 0.0), (64.0, 64.0), (0.0, 64.0)],
        );
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };

    let centre = at(&data, 32, 32);
    assert!(
        (60..=200).contains(&centre[0]) && (60..=200).contains(&centre[2]),
        "horizontal centre must blend red and blue (linear sampling); got {centre:?}"
    );
    // Sanity: the far edges still read as the source columns.
    let left = at(&data, 1, 32);
    let right = at(&data, 62, 32);
    assert!(
        left[0] > 200 && left[2] < 60,
        "left edge must be red; got {left:?}"
    );
    assert!(
        right[2] > 200 && right[0] < 60,
        "right edge must be blue; got {right:?}"
    );
}

/// `draw_image_rgba_arc` keeps nearest sampling: a small image blitted 1:1 on
/// integer pixel boundaries must come back exactly, with no blend at the
/// column seam.
#[test]
fn arc_blit_is_exact_at_1_to_1() {
    let img = red_blue_2x2();
    let Some(data) = render(|ctx| {
        ctx.draw_image_rgba_arc(&img, 2, 2, 16.0, 16.0, 2.0, 2.0);
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };

    assert_eq!(
        at(&data, 16, 16),
        [255, 0, 0, 255],
        "left source column must blit exactly"
    );
    assert_eq!(
        at(&data, 17, 16),
        [0, 0, 255, 255],
        "right source column must blit exactly"
    );
}
