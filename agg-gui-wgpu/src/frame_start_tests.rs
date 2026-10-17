//! Tests for the frame's leading clear (`frame_start.rs`): which clear, if
//! any, heads the command list after each way of starting a frame. The
//! flush clears the target only when the first command is a clear, so this
//! list decides whether a frame starts from the background or from the
//! target's old pixels. Pixel-level checks through the shells' frame bodies
//! are in agg-gui-shell (`frame_clear_tests.rs`) and agg-gui-web-shell
//! (`frame_clear_tests.rs`).
//!
//! Skipped (pass trivially) when no GPU adapter is available.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;

use crate::layer_text_readback_tests::{try_device, Target};
use crate::{DrawCommand, WgpuGfxCtx};

const SIZE: u32 = 64;

fn ctx_and_target() -> Option<(WgpuGfxCtx, Target)> {
    let (device, queue) = try_device()?;
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), SIZE, SIZE);
    let ctx = WgpuGfxCtx::new(
        device,
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        SIZE as f32,
        SIZE as f32,
    );
    Some((ctx, target))
}

/// The clear colours queued, in order, and whether anything else is queued.
fn queued_clears(ctx: &WgpuGfxCtx) -> (Vec<Color>, bool) {
    let mut clears = Vec::new();
    let mut other = false;
    for c in &ctx.commands {
        match c {
            DrawCommand::Clear(color) => clears.push(*color),
            _ => other = true,
        }
    }
    (clears, other)
}

/// The shells begin the frame, then their `default_paint` resets the context:
/// the frame must still start with the theme's background.
#[test]
fn a_reset_inside_the_frame_keeps_the_theme_clear() {
    let Some((mut ctx, target)) = ctx_and_target() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    ctx.begin_frame(target.view.clone());
    ctx.set_fill_color(Color::rgb(1.0, 0.0, 0.0));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, 8.0, 8.0);
    ctx.fill();
    ctx.reset(SIZE as f32, SIZE as f32);
    assert_eq!(
        queued_clears(&ctx),
        (vec![agg_gui::current_visuals().bg_color], false),
        "the reset keeps exactly the frame's clear and drops the drawing"
    );
    ctx.end_frame();
}

/// Resetting before the frame begins (the other order shells use) queues
/// nothing, so `begin_frame` leaves one clear at the head of the list.
#[test]
fn a_reset_before_the_frame_queues_one_clear() {
    let Some((mut ctx, target)) = ctx_and_target() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let bg = Color::rgb(0.0, 0.0, 1.0);
    ctx.reset(SIZE as f32, SIZE as f32);
    assert_eq!(queued_clears(&ctx), (vec![], false));
    ctx.begin_frame_cleared(target.view.clone(), bg);
    assert_eq!(queued_clears(&ctx), (vec![bg], false));
    ctx.end_frame();
}

/// Once the frame is flushed its clear is spent: the next reset (before the
/// next frame begins) must not queue it again.
#[test]
fn a_flushed_frame_leaves_no_clear_for_the_next_reset() {
    let Some((mut ctx, target)) = ctx_and_target() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    ctx.begin_frame_cleared(target.view.clone(), Color::rgb(0.0, 0.0, 1.0));
    ctx.end_frame();
    ctx.reset(SIZE as f32, SIZE as f32);
    assert_eq!(queued_clears(&ctx), (vec![], false));
}

/// An overlay frame draws over a finished frame: no clear, before or after a
/// reset, so the pass loads the frame's pixels.
#[test]
fn an_overlay_frame_queues_no_clear() {
    let Some((mut ctx, target)) = ctx_and_target() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    ctx.begin_overlay_frame(target.view.clone());
    assert_eq!(queued_clears(&ctx), (vec![], false));
    ctx.reset(SIZE as f32, SIZE as f32);
    assert_eq!(queued_clears(&ctx), (vec![], false));
    ctx.end_frame();
}

/// A frame begun and never flushed leaves its clear for a reset; an overlay
/// frame begun after that still must not clear the target.
#[test]
fn an_overlay_frame_drops_a_stale_clear() {
    let Some((mut ctx, target)) = ctx_and_target() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    ctx.begin_frame_cleared(target.view.clone(), Color::rgb(0.0, 0.0, 1.0));
    ctx.reset(SIZE as f32, SIZE as f32);
    ctx.begin_overlay_frame(target.view.clone());
    assert_eq!(queued_clears(&ctx), (vec![], false));
    ctx.end_frame();
}

/// A paint that starts by clearing gets its own colour: before anything is
/// drawn, `clear` replaces the frame's clear (only a leading clear is
/// honoured, so appending it would silently lose it).
#[test]
fn clearing_before_drawing_replaces_the_frame_clear() {
    let Some((mut ctx, target)) = ctx_and_target() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let own = Color::rgb(0.0, 1.0, 0.0);
    ctx.begin_frame_cleared(target.view.clone(), Color::rgb(0.0, 0.0, 1.0));
    ctx.reset(SIZE as f32, SIZE as f32);
    ctx.clear(own);
    assert_eq!(queued_clears(&ctx), (vec![own], false));
    ctx.end_frame();
    let data = target.read();
    assert_eq!(
        crate::layer_text_readback_tests::px(&data, SIZE, SIZE / 2, SIZE / 2),
        [0, 255, 0, 255],
        "the frame is cleared to the paint's colour"
    );
}
