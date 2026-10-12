//! Port of agg-sharp `Agg.GoldenImages/GpuClearTests.cs`: a clip-respecting
//! clear (`DrawCtx::clear_clip`, C#'s `Graphics2D.Clear`) fills exactly the
//! clip rect whatever the transform, and `DrawCtx::clear_rect` (C#'s
//! `Clear(RectangleDouble, Color)`) takes its rect in destination pixels,
//! intersected with the clip — on the GPU and in software alike.

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_rust::trans_affine::TransAffine;

use super::harness::{gpu, software};

/// `WebGpuOffscreenCapture`'s default size.
const W: u32 = 512;
const H: u32 = 384;

/// C#'s `Clip`: `RectangleDouble(100, 80, 300, 240)` (left, bottom, right, top).
const CLIP: [f64; 4] = [100.0, 80.0, 300.0, 240.0];

/// AggSharpDemo's frame for a y-down demo: y flipped over the demo's height,
/// then placed on the page (`AggDemoView.DrawDirect`). Clear there left the
/// frame unfilled.
fn y_flipped() -> TransAffine {
    let mut t = TransAffine::new_scaling(1.0, -1.0);
    t.multiply(&TransAffine::new_translation(0.0, 160.0));
    t.multiply(&TransAffine::new_translation(100.0, 80.0));
    t
}

#[test]
fn clear_fills_the_clip_rect_under_a_y_flip() {
    assert_fills_exactly(
        y_flipped(),
        &|ctx| ctx.clear_clip(Color::rgb(1.0, 0.0, 0.0)),
        CLIP,
    );
}

#[test]
fn clear_fills_the_clip_rect_under_a_scale() {
    let mut t = TransAffine::new_scaling(2.0, 2.0);
    t.multiply(&TransAffine::new_translation(30.0, 20.0));
    assert_fills_exactly(t, &|ctx| ctx.clear_clip(Color::rgb(1.0, 0.0, 0.0)), CLIP);
}

#[test]
fn clear_rect_is_in_destination_pixels_under_a_y_flip() {
    // C#'s RectangleDouble(150, 50, 250, 200).
    assert_fills_exactly(
        y_flipped(),
        &|ctx| ctx.clear_rect(150.0, 50.0, 100.0, 150.0, Color::rgb(1.0, 0.0, 0.0)),
        [150.0, 80.0, 250.0, 200.0],
    );
}

/// Draws `clear` under `transform` with `CLIP` set, on the GPU and in
/// software, and checks each is red exactly inside `expected`.
fn assert_fills_exactly(
    transform: TransAffine,
    clear: &dyn Fn(&mut dyn DrawCtx),
    expected: [f64; 4],
) {
    let draw = |ctx: &mut dyn DrawCtx| {
        ctx.clip_rect(CLIP[0], CLIP[1], CLIP[2] - CLIP[0], CLIP[3] - CLIP[1]);
        ctx.set_transform(transform);
        clear(ctx);
    };
    let soft = software(W, H, Color::black(), &draw);
    assert_eq!(red_bounds(&soft), expected, "software");
    let Some(gpu_image) = gpu(W, H, Color::black(), &draw) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_eq!(red_bounds(&gpu_image), expected, "GPU");
}

/// The bounds (left, bottom, right, top, y up) of the red pixels of a
/// top-row-first readback, or all zeros when there are none.
fn red_bounds(data: &[u8]) -> [f64; 4] {
    let (mut left, mut bottom, mut right, mut top) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for row in 0..H as i64 {
        let y = H as i64 - 1 - row;
        for x in 0..W as i64 {
            let i = ((row * W as i64 + x) * 4) as usize;
            if data[i] > 200 && data[i + 1] < 60 && data[i + 2] < 60 {
                left = left.min(x);
                bottom = bottom.min(y);
                right = right.max(x + 1);
                top = top.max(y + 1);
            }
        }
    }
    if left == i64::MAX {
        [0.0; 4]
    } else {
        [left as f64, bottom as f64, right as f64, top as f64]
    }
}
