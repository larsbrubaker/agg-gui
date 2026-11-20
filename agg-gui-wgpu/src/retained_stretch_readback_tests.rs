//! Headless GPU readback test for the stretched composite of a retained
//! layer (`layers.rs`, `DrawCtx::composite_retained_layer_scaled`): a layer
//! rastered at one scale and composited under twice that scale covers the
//! area its content now covers, from the same place, instead of being
//! refused as the plain composite refuses a layer of another size. A node
//! editor's cards are drawn this way while a zoom moves.
//!
//! Skipped (passes trivially) when no GPU adapter is available.

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::framebuffer::Framebuffer;

use crate::headless::{HeadlessFrame, HeadlessGpu};

const W: u32 = 80;
const H: u32 = 80;
const KEY: u64 = 41;

/// The layer: a red 20 x 20 square, placed at (10, 10).
fn red() -> Color {
    Color::from_rgba8(255, 0, 0, 255)
}

/// The pixel at `(x, y)` (y up), as RGBA8.
fn px(fb: &Framebuffer, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

fn is_red(p: [u8; 4]) -> bool {
    p[0] > 230 && p[1] < 25 && p[2] < 25
}

fn is_white(p: [u8; 4]) -> bool {
    p[0] > 230 && p[1] > 230 && p[2] > 230
}

#[test]
fn a_retained_layer_composited_stretched_covers_its_content_at_the_new_scale() {
    let gpu = match HeadlessGpu::shared() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: {e}");
            return;
        }
    };
    let mut frame = HeadlessFrame::new(gpu, W, H);
    // Rastered at scale 1.
    frame.render(Color::white(), |ctx| {
        ctx.translate(10.0, 10.0);
        ctx.push_retained_layer_with_alpha(KEY, 20.0, 20.0, 1.0);
        ctx.set_fill_color(red());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 20.0, 20.0);
        ctx.fill();
        ctx.pop_layer();
    });
    // Drawn again under twice the scale: the plain composite refuses the
    // layer (its texture is the old size), the stretched one draws it.
    let mut plain = true;
    let mut stretched = false;
    frame.render(Color::white(), |ctx| {
        ctx.translate(10.0, 10.0);
        ctx.scale(2.0, 2.0);
        plain = ctx.composite_retained_layer(KEY, 20.0, 20.0, 1.0);
        stretched = ctx.composite_retained_layer_scaled(KEY, 20.0, 20.0, 1.0);
    });
    assert!(
        !plain,
        "a layer of another scale is not composited as it is"
    );
    assert!(stretched, "it is composited stretched");
    let fb = frame.read_framebuffer().expect("the frame reads back");
    for (x, y) in [(12, 12), (30, 30), (47, 47), (12, 47), (47, 12)] {
        assert!(
            is_red(px(&fb, x, y)),
            "({x}, {y}) lies in the square at twice the size: {:?}",
            px(&fb, x, y)
        );
    }
    for (x, y) in [(7, 7), (53, 53), (53, 30), (30, 53)] {
        assert!(
            is_white(px(&fb, x, y)),
            "({x}, {y}) lies outside it: {:?}",
            px(&fb, x, y)
        );
    }
}
