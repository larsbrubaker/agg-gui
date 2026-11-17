//! Tests for bind group reuse (`bind_group_cache.rs`): a context drawing the
//! same frame over and over makes no new bind groups once the first frames
//! have made theirs, and lets go of the ones for textures it stops drawing.
//!
//! Why this matters: in Firefox a bind group is freed only when JavaScript's
//! garbage collector collects it, and each one takes descriptors from a
//! fixed per-device heap. A UI that made a few hundred bind groups every
//! frame ran that heap out within a minute or two of redraws and lost the
//! device ("Out of memory"). Like the other headless GPU tests these skip
//! (pass trivially) when no adapter is available.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::{DrawCtx, GradientSpread, GradientStop, LinearGradientPaint};
use agg_rust::trans_affine::TransAffine;

use crate::headless::{HeadlessFrame, HeadlessGpu};
use crate::{BindGroupCounts, WgpuGfxCtx};

/// The shared device, or `None` (the test skips) when there is no adapter.
fn gpu() -> Option<&'static HeadlessGpu> {
    match HeadlessGpu::shared() {
        Ok(gpu) => Some(gpu),
        Err(e) => {
            eprintln!("SKIP: {e}");
            None
        }
    }
}

/// The pixel buffers a UI keeps from frame to frame (an icon, a glyph mask,
/// a label's LCD backbuffer), so the context's texture caches find them.
struct Content {
    image: Arc<Vec<u8>>,
    lcd_mask: Arc<Vec<u8>>,
    lcb_color: Arc<Vec<u8>>,
    lcb_alpha: Arc<Vec<u8>>,
}

const SIDE: u32 = 8;

impl Content {
    fn new() -> Self {
        let rgb = (SIDE * SIDE * 3) as usize;
        Self {
            image: Arc::new(vec![200u8; (SIDE * SIDE * 4) as usize]),
            lcd_mask: Arc::new(vec![180u8; rgb]),
            lcb_color: Arc::new(vec![90u8; rgb]),
            lcb_alpha: Arc::new(vec![255u8; rgb]),
        }
    }
}

fn fill_rect(ctx: &mut WgpuGfxCtx, x: f64, y: f64, w: f64, h: f64) {
    ctx.begin_path();
    ctx.rect(x, y, w, h);
    ctx.fill();
}

/// One frame of a UI: a solid fill, a gradient fill, an image, LCD text
/// (a glyph mask and a label backbuffer), a transient layer, a retained
/// layer and a path-clip layer — every uniform layout and every group-1
/// texture binding the frame path has.
fn paint(ctx: &mut WgpuGfxCtx, content: &Content, with_image: bool) {
    ctx.set_fill_color(Color::rgba(0.2, 0.4, 0.8, 1.0));
    fill_rect(ctx, 2.0, 2.0, 20.0, 12.0);

    ctx.set_fill_linear_gradient(LinearGradientPaint {
        x1: 0.0,
        y1: 0.0,
        x2: 60.0,
        y2: 0.0,
        transform: TransAffine::new(),
        spread: GradientSpread::Pad,
        stops: vec![
            GradientStop {
                offset: 0.0,
                color: Color::rgba(1.0, 0.0, 0.0, 1.0),
            },
            GradientStop {
                offset: 1.0,
                color: Color::rgba(0.0, 0.0, 1.0, 1.0),
            },
        ],
    });
    fill_rect(ctx, 0.0, 20.0, 60.0, 10.0);
    ctx.set_fill_color(Color::rgba(0.9, 0.5, 0.1, 1.0));

    if with_image {
        ctx.draw_image_rgba_arc(&content.image, SIDE, SIDE, 70.0, 2.0, 16.0, 16.0);
    }
    ctx.draw_lcd_mask_arc(
        &content.lcd_mask,
        SIDE,
        SIDE,
        Color::rgba(0.0, 0.0, 0.0, 1.0),
        90.0,
        2.0,
    );
    ctx.draw_lcd_backbuffer_arc(
        &content.lcb_color,
        &content.lcb_alpha,
        1,
        SIDE,
        SIDE,
        100.0,
        2.0,
        SIDE as f64,
        SIDE as f64,
    );

    ctx.push_layer(40.0, 30.0);
    fill_rect(ctx, 1.0, 1.0, 10.0, 10.0);
    ctx.pop_layer();

    ctx.push_retained_layer_with_alpha(7, 40.0, 30.0, 0.8);
    fill_rect(ctx, 4.0, 4.0, 8.0, 8.0);
    ctx.pop_layer();

    ctx.save();
    ctx.begin_path();
    ctx.rect(10.0, 40.0, 30.0, 20.0);
    ctx.clip_path();
    fill_rect(ctx, 0.0, 35.0, 50.0, 30.0);
    ctx.restore();
}

/// The bind groups made between two counts, by kind.
fn made_since(before: BindGroupCounts, after: BindGroupCounts) -> (u64, u64, u64) {
    (
        after.uniform - before.uniform,
        after.texture - before.texture,
        after.one_frame - before.one_frame,
    )
}

#[test]
fn steady_frames_make_no_bind_groups_but_their_one_frame_ones() {
    let Some(gpu) = gpu() else { return };
    let mut frame = HeadlessFrame::new(gpu, 128, 96);
    let content = Content::new();
    // The first frame makes the bind groups; the second may still make a
    // few (the retained layer composites from its cache from then on).
    for _ in 0..2 {
        frame.render(Color::white(), |ctx| paint(ctx, &content, true));
    }
    let warm = frame.ctx().bind_group_counts();
    for i in 1..=5u64 {
        frame.render(Color::white(), |ctx| paint(ctx, &content, true));
        let counts = frame.ctx().bind_group_counts();
        // The gradient's ramp texture is made for each frame (its colours
        // come from the stops the frame paints), so its group 1 is the one
        // bind group a steady frame makes: exactly one per frame.
        assert_eq!(
            made_since(warm, counts),
            (0, 0, i),
            "frame {i}: (uniform, texture, one-frame) bind groups made since warm-up: {counts:?} after {warm:?}"
        );
        assert_eq!(
            counts.cached, warm.cached,
            "frame {i}: the same bind groups are kept"
        );
    }
    assert!(
        warm.cached > 0,
        "the frames' bind groups are kept: {warm:?}"
    );
}

#[test]
fn a_texture_no_longer_drawn_lets_go_of_its_bind_group() {
    let Some(gpu) = gpu() else { return };
    let mut frame = HeadlessFrame::new(gpu, 128, 96);
    let content = Content::new();
    for _ in 0..2 {
        frame.render(Color::white(), |ctx| paint(ctx, &content, true));
    }
    let with_image = frame.ctx().bind_group_counts();
    // The image goes away: its group-1 bind group is not kept for it.
    frame.render(Color::white(), |ctx| paint(ctx, &content, false));
    let without = frame.ctx().bind_group_counts();
    assert_eq!(
        without.cached + 1,
        with_image.cached,
        "the image's bind group is let go once a frame does not draw it: {without:?} after {with_image:?}"
    );
    // Drawing it again makes one bind group for it, kept again.
    frame.render(Color::white(), |ctx| paint(ctx, &content, true));
    let again = frame.ctx().bind_group_counts();
    assert_eq!(
        made_since(without, again).1,
        1,
        "{again:?} after {without:?}"
    );
    assert_eq!(again.cached, with_image.cached, "{again:?}");
}

/// An LCD mask drawn from a slice gets a texture for its frame only, let go
/// in that frame: the bind group over it goes with it at the frame's end
/// rather than keeping the texture alive past its destruction.
#[test]
fn a_texture_let_go_in_its_frame_takes_its_bind_group_with_it() {
    let Some(gpu) = gpu() else { return };
    let mut frame = HeadlessFrame::new(gpu, 128, 96);
    let content = Content::new();
    for _ in 0..2 {
        frame.render(Color::white(), |ctx| paint(ctx, &content, true));
    }
    let steady = frame.ctx().bind_group_counts();
    let mask = vec![120u8; (SIDE * SIDE * 3) as usize];
    for i in 1..=3u64 {
        frame.render(Color::white(), |ctx| {
            paint(ctx, &content, true);
            ctx.draw_lcd_mask(&mask, SIDE, SIDE, Color::black(), 40.0, 60.0);
        });
        let counts = frame.ctx().bind_group_counts();
        assert_eq!(
            made_since(steady, counts).1,
            i,
            "frame {i}: each frame's mask texture is new, so is its bind group: {counts:?}"
        );
        assert_eq!(
            counts.cached, steady.cached,
            "frame {i}: the one-frame mask's bind group is not kept: {counts:?} after {steady:?}"
        );
    }
}
