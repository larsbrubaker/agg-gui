//! Headless GPU readback: LCD text and LCD planes larger than the device's
//! texture limit draw (`lcd_tiles.rs`). A single line a few thousand
//! characters long rasterises to a coverage mask wider than any GPU allows;
//! it is uploaded as tiles, and the part on screen is what a short line with
//! the same start draws. Splitting at an artificially small limit draws
//! exactly what one texture draws, for the mask, the one-shot slice and the
//! two-plane backbuffer paths.
//!
//! The headless device keeps wgpu's panicking error handler, so a texture
//! request over the limit fails these tests loudly. Skipped (passes
//! trivially) when no GPU adapter is available, like the sibling
//! `layer_text_readback_tests`.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::text::Font;

use crate::layer_text_readback_tests::{try_device, Target};
use crate::WgpuGfxCtx;

const TEST_FONT: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
const W: u32 = 256;
const H: u32 = 40;

fn ctx_for(device: &Arc<wgpu::Device>, queue: &Arc<wgpu::Queue>) -> WgpuGfxCtx {
    let mut ctx = WgpuGfxCtx::new(
        Arc::clone(device),
        Arc::clone(queue),
        wgpu::TextureFormat::Rgba8Unorm,
        W as f32,
        H as f32,
    );
    ctx.reset(W as f32, H as f32);
    ctx
}

/// The frame after drawing black LCD `text` on white, with textures split at
/// `max_dim` when given.
fn lcd_text_frame(text: &str, max_dim: Option<u32>) -> Option<Vec<u8>> {
    let (device, queue) = try_device()?;
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), W, H);
    let mut ctx = ctx_for(&device, &queue);
    if let Some(max_dim) = max_dim {
        ctx.max_texture_dim = max_dim;
    }
    ctx.set_lcd_mode(true);
    ctx.clear(Color::white());
    ctx.set_font(Arc::new(Font::from_slice(TEST_FONT).unwrap()));
    ctx.set_font_size(18.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 1.0));
    ctx.fill_text(text, 4.0, 12.0);
    ctx.flush_to_surface(&target.view);
    Some(target.read())
}

fn dark_pixels(frame: &[u8]) -> usize {
    frame
        .chunks_exact(4)
        .filter(|p| (p[0] as u32 + p[1] as u32 + p[2] as u32) < 300)
        .count()
}

#[test]
fn a_text_line_wider_than_the_texture_limit_draws_its_visible_part() {
    let Some((device, _)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let limit = device.limits().max_texture_dimension_2d;
    let long = "W".repeat(4000);
    let font = Font::from_slice(TEST_FONT).unwrap();
    let width = agg_gui::text::measure_text_metrics(&font, &long, 18.0).width;
    assert!(
        width > limit as f64,
        "the line ({width} px) must be wider than the device allows ({limit})"
    );

    let long_frame = lcd_text_frame(&long, None).expect("device");
    // Sixty W's run well past the 256 px frame, so the visible part is the
    // same text.
    let short_frame = lcd_text_frame(&"W".repeat(60), None).expect("device");
    assert!(dark_pixels(&long_frame) > 500, "the line is drawn");
    assert_eq!(
        long_frame, short_frame,
        "the visible part of the long line is what the short line draws"
    );
}

#[test]
fn lcd_text_split_into_tiles_draws_what_one_texture_draws() {
    let text = "Tiles of a text mask meet without a seam";
    let Some(whole) = lcd_text_frame(text, None) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    // 7 texels a side splits the mask into many tiles across and down.
    let tiled = lcd_text_frame(text, Some(7)).expect("device");
    assert!(dark_pixels(&whole) > 200, "the text is drawn");
    assert_eq!(tiled, whole);
}

/// A plane whose texels differ everywhere, so a tile drawn out of place or
/// a missing seam column shows.
fn patterned_plane(w: u32, h: u32) -> Vec<u8> {
    let mut plane = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            plane.extend_from_slice(&[(x * 7) as u8, (y * 11) as u8, ((x + y) * 5) as u8]);
        }
    }
    plane
}

/// The frame after drawing a patterned `w` x `h` plane through `draw`, with
/// textures split at `max_dim` when given.
fn plane_frame(
    max_dim: Option<u32>,
    draw: impl Fn(&mut WgpuGfxCtx, &Arc<Vec<u8>>, u32, u32),
) -> Option<Vec<u8>> {
    let (device, queue) = try_device()?;
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), W, H);
    let mut ctx = ctx_for(&device, &queue);
    if let Some(max_dim) = max_dim {
        ctx.max_texture_dim = max_dim;
    }
    ctx.clear(Color::white());
    let (w, h) = (90u32, 30u32);
    let plane = Arc::new(patterned_plane(w, h));
    draw(&mut ctx, &plane, w, h);
    ctx.flush_to_surface(&target.view);
    Some(target.read())
}

#[test]
fn a_one_shot_mask_split_into_tiles_draws_what_one_texture_draws() {
    let draw = |ctx: &mut WgpuGfxCtx, plane: &Arc<Vec<u8>>, w: u32, h: u32| {
        ctx.draw_lcd_mask(
            plane.as_slice(),
            w,
            h,
            Color::rgba(0.1, 0.2, 0.6, 1.0),
            5.0,
            4.0,
        );
    };
    let Some(whole) = plane_frame(None, draw) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let tiled = plane_frame(Some(16), draw).expect("device");
    assert_ne!(dark_pixels(&whole), 0, "the mask is drawn");
    assert_eq!(tiled, whole);
}

#[test]
fn a_backbuffer_split_into_tiles_draws_what_one_texture_draws() {
    let draw = |ctx: &mut WgpuGfxCtx, plane: &Arc<Vec<u8>>, w: u32, h: u32| {
        let alpha = Arc::new(vec![255u8; (w * h * 3) as usize]);
        ctx.draw_lcd_backbuffer_arc_impl(plane, &alpha, 7, w, h, 5.0, 4.0, w as f64, h as f64);
    };
    let Some(whole) = plane_frame(None, draw) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let tiled = plane_frame(Some(16), draw).expect("device");
    assert_ne!(dark_pixels(&whole), 0, "the planes are drawn");
    assert_eq!(tiled, whole);
}
