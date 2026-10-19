//! Headless GPU readback: images and layers larger than the device's texture
//! limit draw instead of failing validation. An image wider than the limit
//! is uploaded shrunk to fit and still covers its quad (each of the slice,
//! `Arc` and four-corner blit paths); a layer wider than the limit holds the
//! content within the limit. LCD text planes have their own tests
//! (`lcd_tiles_readback_tests`).
//!
//! The headless device keeps wgpu's panicking error handler, so a texture
//! request over the limit fails these tests loudly. Skipped (passes
//! trivially) when no GPU adapter is available.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;

use crate::image_blit::fit_within;
use crate::layer_text_readback_tests::{px, try_device, Target};
use crate::WgpuGfxCtx;

const W: u32 = 64;
const H: u32 = 16;

fn ctx_for(device: &Arc<wgpu::Device>, queue: &Arc<wgpu::Queue>) -> WgpuGfxCtx {
    let mut ctx = WgpuGfxCtx::new(
        Arc::clone(device),
        Arc::clone(queue),
        wgpu::TextureFormat::Rgba8Unorm,
        W as f32,
        H as f32,
    );
    ctx.reset(W as f32, H as f32);
    ctx.clear(Color::white());
    ctx
}

#[test]
fn fit_within_keeps_the_aspect_and_leaves_fitting_sizes_alone() {
    assert_eq!(fit_within(100, 50, 16384), None);
    assert_eq!(fit_within(16384, 16384, 16384), None);
    assert_eq!(fit_within(32768, 100, 16384), Some((16384, 50)));
    assert_eq!(fit_within(10, 40000, 16384), Some((4, 16384)));
    assert_eq!(fit_within(100000, 1, 16384), Some((16384, 1)));
}

/// An image one pixel wider than the device allows: red on its left half,
/// blue on its right.
fn wide_image(limit: u32) -> (Arc<Vec<u8>>, u32, u32) {
    let (w, h) = (limit + 1, 2u32);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..h {
        for x in 0..w {
            let px = if x < w / 2 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            };
            data.extend_from_slice(&px);
        }
    }
    (Arc::new(data), w, h)
}

/// Draw the wide image over the whole frame through `draw` and check its two
/// halves landed on the frame's two halves.
fn assert_wide_image_draws(draw: impl Fn(&mut WgpuGfxCtx, &Arc<Vec<u8>>, u32, u32)) {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), W, H);
    let mut ctx = ctx_for(&device, &queue);
    let (data, w, h) = wide_image(device.limits().max_texture_dimension_2d);
    draw(&mut ctx, &data, w, h);
    ctx.flush_to_surface(&target.view);
    let frame = target.read();
    let left = px(&frame, W, 8, H / 2);
    let right = px(&frame, W, W - 8, H / 2);
    assert!(left[0] > 240 && left[2] < 15, "left half is red: {left:?}");
    assert!(
        right[2] > 240 && right[0] < 15,
        "right half is blue: {right:?}"
    );
}

#[test]
fn an_image_wider_than_the_texture_limit_draws_from_a_slice() {
    assert_wide_image_draws(|ctx, data, w, h| {
        ctx.draw_image_rgba(data.as_slice(), w, h, 0.0, 0.0, W as f64, H as f64);
    });
}

#[test]
fn an_image_wider_than_the_texture_limit_draws_from_an_arc() {
    assert_wide_image_draws(|ctx, data, w, h| {
        ctx.draw_image_rgba_arc(data, w, h, 0.0, 0.0, W as f64, H as f64);
    });
}

#[test]
fn an_image_wider_than_the_texture_limit_draws_through_its_corners() {
    assert_wide_image_draws(|ctx, data, w, h| {
        let (fw, fh) = (W as f64, H as f64);
        ctx.draw_image_rgba_corners(data, w, h, [(0.0, 0.0), (fw, 0.0), (fw, fh), (0.0, fh)]);
    });
}

#[test]
fn a_layer_wider_than_the_texture_limit_draws_its_content() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), W, H);
    let mut ctx = ctx_for(&device, &queue);
    let limit = device.limits().max_texture_dimension_2d as f64;
    ctx.push_layer_with_alpha(limit + 1000.0, H as f64, 1.0);
    ctx.set_fill_color(Color::rgba(1.0, 0.0, 0.0, 1.0));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, 32.0, H as f64);
    ctx.fill();
    ctx.pop_layer();
    ctx.flush_to_surface(&target.view);
    let frame = target.read();
    let inside = px(&frame, W, 8, H / 2);
    let outside = px(&frame, W, 48, H / 2);
    assert!(
        inside[0] > 240 && inside[1] < 15,
        "the layer's red: {inside:?}"
    );
    assert!(outside[1] > 240, "white past the red: {outside:?}");
}
