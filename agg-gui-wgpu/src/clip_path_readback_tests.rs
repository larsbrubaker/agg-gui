//! Headless GPU readback tests for `DrawCtx::clip_path` (see `layer_mask.rs`).
//!
//! Each test renders through the real [`WgpuGfxCtx`] deferred-command pipeline
//! onto an offscreen texture, reads the pixels back, and asserts on them.  They
//! mirror the software-backend contracts in
//! `agg-gui/src/tests/clip_path_software.rs`.
//!
//! Like `layer_text_readback_tests`, they are skipped (pass trivially) when no
//! GPU adapter is available.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;

use crate::layer_text_readback_tests::{px, try_device, Target};
use crate::WgpuGfxCtx;

const SIZE: u32 = 64;

fn red() -> Color {
    Color::rgba(1.0, 0.0, 0.0, 1.0)
}
fn blue() -> Color {
    Color::rgba(0.0, 0.0, 1.0, 1.0)
}
fn green() -> Color {
    Color::rgba(0.0, 1.0, 0.0, 1.0)
}

fn is_blue(p: [u8; 4]) -> bool {
    p[2] > 200 && p[0] < 60 && p[1] < 60
}
fn is_red(p: [u8; 4]) -> bool {
    p[0] > 200 && p[1] < 60 && p[2] < 60
}

/// Sample in Y-up coordinates (the readback is top-row-first).
fn at(data: &[u8], x: u32, y_up: u32) -> [u8; 4] {
    px(data, SIZE, x, SIZE - 1 - y_up)
}

/// Fill the whole target with a colour in the current clip.
fn fill_all(ctx: &mut WgpuGfxCtx, color: Color) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.rect(-200.0, -200.0, 600.0, 600.0);
    ctx.fill();
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
    assert!(ctx.supports_clip_path());
    ctx.clear(red());
    paint(&mut ctx);
    ctx.flush_to_surface(&target.view);
    Some(target.read())
}

#[test]
fn clip_path_masks_fill_to_the_circle() {
    let Some(data) = render(|ctx| {
        ctx.save();
        ctx.begin_path();
        ctx.circle(32.0, 32.0, 16.0);
        ctx.clip_path();
        fill_all(ctx, blue());
        ctx.restore();
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };

    assert!(is_blue(at(&data, 32, 32)), "centre must be blue");
    assert!(is_red(at(&data, 1, 1)), "corner must stay red");
    assert!(
        is_red(at(&data, 52, 32)),
        "just past the circle edge must be red; got {:?}",
        at(&data, 52, 32)
    );
}

#[test]
fn clip_is_released_after_restore() {
    let Some(data) = render(|ctx| {
        ctx.save();
        ctx.begin_path();
        ctx.circle(32.0, 32.0, 16.0);
        ctx.clip_path();
        fill_all(ctx, blue());
        ctx.restore();
        fill_all(ctx, green());
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };

    let corner = at(&data, 1, 1);
    assert!(
        corner[1] > 200 && corner[0] < 60,
        "corner must be green after restore; got {corner:?}"
    );
}

#[test]
fn clip_path_respects_translate_and_rotate() {
    let Some(data) = render(|ctx| {
        ctx.save();
        ctx.translate(32.0, 32.0);
        ctx.rotate(std::f64::consts::FRAC_PI_4);
        // 20×20 square centred on the origin, rotated 45° → a diamond.
        ctx.begin_path();
        ctx.rect(-10.0, -10.0, 20.0, 20.0);
        ctx.clip_path();
        fill_all(ctx, blue());
        ctx.restore();
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };

    assert!(is_blue(at(&data, 32, 32)), "centre must be blue");
    assert!(
        is_blue(at(&data, 32, 42)),
        "on-axis point inside the diamond must be blue; got {:?}",
        at(&data, 32, 42)
    );
    assert!(
        is_red(at(&data, 44, 44)),
        "the unrotated square's corner direction must stay red; got {:?}",
        at(&data, 44, 44)
    );
}

#[test]
fn nested_clip_paths_intersect() {
    let Some(data) = render(|ctx| {
        ctx.save();
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 32.0, 64.0); // left half
        ctx.clip_path();
        ctx.save();
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 64.0, 32.0); // bottom half
        ctx.clip_path();
        fill_all(ctx, blue());
        ctx.restore();
        ctx.restore();
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };

    assert!(is_blue(at(&data, 16, 16)), "bottom-left must be blue");
    assert!(is_red(at(&data, 48, 16)), "bottom-right must be red");
    assert!(is_red(at(&data, 16, 48)), "top-left must be red");
    assert!(is_red(at(&data, 48, 48)), "top-right must be red");
}

#[test]
fn empty_path_clip_draws_nothing() {
    let Some(data) = render(|ctx| {
        ctx.save();
        ctx.begin_path();
        ctx.clip_path();
        fill_all(ctx, blue());
        ctx.restore();
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };

    assert!(
        is_red(at(&data, 32, 32)),
        "an empty clip must draw nothing; got {:?}",
        at(&data, 32, 32)
    );
}

fn is_green(p: [u8; 4]) -> bool {
    p[1] > 200 && p[0] < 60 && p[2] < 60
}

/// `clip_path` keeps the current path (and the clip layer's pop puts it back),
/// so the canvas-port idiom `begin_path(); <outline>; fill(); save();
/// clip_path(); <interior>; restore(); stroke();` strokes the outline.
#[test]
fn clip_path_keeps_the_current_path_for_a_later_stroke() {
    let Some(data) = render(|ctx| {
        ctx.set_fill_color(red());
        ctx.begin_path();
        ctx.circle(32.0, 32.0, 16.0);
        ctx.fill();
        ctx.save();
        ctx.clip_path();
        fill_all(ctx, blue());
        ctx.restore();
        ctx.set_stroke_color(green());
        ctx.set_line_width(6.0);
        ctx.stroke();
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };

    assert!(
        is_green(at(&data, 48, 32)),
        "stroke must follow the clipping circle; got {:?}",
        at(&data, 48, 32)
    );
    assert!(
        is_blue(at(&data, 32, 32)),
        "centre must stay blue; got {:?}",
        at(&data, 32, 32)
    );
}

/// A balanced `save()`/`restore()` pair inside a clip layer must not pop the
/// clip early.
#[test]
fn balanced_save_restore_inside_clip_does_not_pop_it() {
    let Some(data) = render(|ctx| {
        ctx.save();
        ctx.begin_path();
        ctx.circle(32.0, 32.0, 16.0);
        ctx.clip_path();
        ctx.save();
        ctx.set_fill_color(green());
        ctx.restore();
        fill_all(ctx, blue());
        ctx.restore();
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };

    assert!(is_blue(at(&data, 32, 32)), "centre must be blue");
    assert!(
        is_red(at(&data, 1, 1)),
        "the clip must still be active after the inner restore; got {:?}",
        at(&data, 1, 1)
    );
}

#[test]
fn clip_path_inside_a_compositing_layer() {
    let Some(data) = render(|ctx| {
        ctx.push_layer(SIZE as f64, SIZE as f64);
        ctx.save();
        ctx.begin_path();
        ctx.circle(32.0, 32.0, 16.0);
        ctx.clip_path();
        fill_all(ctx, blue());
        ctx.restore();
        ctx.pop_layer();
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };

    assert!(is_blue(at(&data, 32, 32)), "centre must be blue");
    assert!(is_red(at(&data, 1, 1)), "corner must stay red");
}

#[test]
fn compositing_layer_inside_a_clip_path() {
    let Some(data) = render(|ctx| {
        ctx.save();
        ctx.begin_path();
        ctx.circle(32.0, 32.0, 16.0);
        ctx.clip_path();
        ctx.push_layer(SIZE as f64, SIZE as f64);
        fill_all(ctx, blue());
        ctx.pop_layer();
        ctx.restore();
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };

    assert!(is_blue(at(&data, 32, 32)), "centre must be blue");
    assert!(is_red(at(&data, 1, 1)), "corner must stay red");
}

/// The clip edge is anti-aliased: some pixel straddling the circle boundary
/// is strictly between the clipped fill and the background.
#[test]
fn clip_path_edge_is_anti_aliased() {
    let Some(data) = render(|ctx| {
        ctx.save();
        ctx.begin_path();
        ctx.circle(32.0, 32.0, 16.0);
        ctx.clip_path();
        fill_all(ctx, blue());
        ctx.restore();
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };

    let row: Vec<[u8; 4]> = (44..54).map(|x| at(&data, x, 32)).collect();
    assert!(
        row.iter()
            .any(|p| p[0] > 0 && p[0] < 255 && p[2] > 0 && p[2] < 255),
        "no anti-aliased pixel on the clip edge: {row:?}"
    );
}
