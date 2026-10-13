//! The outline text path puts a hinted baseline on a whole device pixel row,
//! the row the coverage-mask path and the software outline fill use.
//!
//! * `outline_baseline` (the positioning function, no GPU): under a pure
//!   translate and scale the baseline lands on the nearest device row at 1x,
//!   1.5x and 2x; under a rotation it never moves toward a device row; with
//!   hinting off it is untouched.
//! * Headless GPU readback: "HHHH" drawn through the mask path (`fill_text`)
//!   and the outline path (`fill_text_outline`) at 1x, 1.5x and 2x has its
//!   flat bottom edge on the same device row, fully inked.  The wgpu backend
//!   always has a mask composite, so the outline path is driven directly.
//!
//! The readback tests pass trivially when no GPU adapter is available.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::text::Font;
use agg_gui::TransAffine;

use crate::layer_text_readback_tests::{px, try_device, Target};
use crate::text_render::outline_baseline;
use crate::WgpuGfxCtx;

const TEST_FONT: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");

/// A Y-down device transform at `scale`, as the app's surface applies: the
/// baseline `y` is `ty - y * scale` device rows from the top.
fn y_flip_scale(scale: f64, height: f64) -> TransAffine {
    let mut t = TransAffine::new();
    t.sx = scale;
    t.sy = -scale;
    t.ty = height;
    t
}

fn device_row(t: &TransAffine, y: f64) -> f64 {
    y * t.sy + t.ty
}

#[test]
fn outline_baseline_lands_on_a_device_row_at_1x_1_5x_and_2x() {
    for &scale in &[1.0, 1.5, 2.0] {
        for &flip in &[false, true] {
            let t = if flip {
                y_flip_scale(scale, 48.0)
            } else {
                let mut t = TransAffine::new();
                t.sx = scale;
                t.sy = scale;
                t.ty = 0.25;
                t
            };
            for &y in &[3.5, 10.0, 10.3, 10.5, 10.7, 17.25] {
                let snapped = outline_baseline(&t, y, true);
                let row = device_row(&t, snapped);
                assert!(
                    (row - row.round()).abs() < 1e-9,
                    "scale {scale} flip {flip}: baseline {y} snapped to {snapped}, \
                     device row {row} is not whole"
                );
                // Within half a device row of where it started.
                assert!(
                    (row - device_row(&t, y)).abs() <= 0.5 + 1e-9,
                    "scale {scale} flip {flip}: baseline {y} moved to device row {row}"
                );
            }
        }
    }
}

#[test]
fn outline_baseline_at_1_5x_is_a_device_row_not_a_logical_one() {
    // 10.5 units at 1.5x is device row 15.75: row 16, which is 10.6667 units,
    // not the logical unit 11 (device row 16.5, half a pixel soft).
    let mut t = TransAffine::new();
    t.sx = 1.5;
    t.sy = 1.5;
    let snapped = outline_baseline(&t, 10.5, true);
    assert!((snapped - 16.0 / 1.5).abs() < 1e-9, "snapped {snapped}");
}

#[test]
fn outline_baseline_does_not_snap_a_rotated_text_to_device_rows() {
    // Rotated 30 degrees at 1.5x: no device row exists along the glyph's own
    // Y, so the snap is the logical unit one, never a device row.
    let angle = 30f64.to_radians();
    let (c, s) = (angle.cos() * 1.5, angle.sin() * 1.5);
    let mut t = TransAffine::new();
    t.sx = c;
    t.shy = s;
    t.shx = -s;
    t.sy = c;
    for &y in &[3.5, 10.3, 10.5, 17.25] {
        let snapped = outline_baseline(&t, y, true);
        assert_eq!(snapped, (y + 0.5).floor(), "rotated baseline {y}");
        // And a skew alone, with a pure Y scale that would otherwise snap.
        let mut skew = TransAffine::new();
        skew.sx = 1.5;
        skew.sy = 1.5;
        skew.shx = 0.4;
        assert_eq!(
            outline_baseline(&skew, y, true),
            (y + 0.5).floor(),
            "skewed baseline {y}"
        );
    }
}

#[test]
fn outline_baseline_is_untouched_with_hinting_off() {
    let mut t = TransAffine::new();
    t.sx = 1.5;
    t.sy = 1.5;
    assert_eq!(outline_baseline(&t, 10.3, false), 10.3);
}

#[derive(Clone, Copy)]
enum TextPath {
    Mask,
    Outline,
}

/// The device row (from the top) of the flat bottom edge of the "HHHH" ink,
/// and how dark that row is (0 = untouched white, 255 = black).
fn baseline_row(path: TextPath, scale: f64) -> Option<(u32, u8)> {
    let (device, queue) = try_device()?;
    let (w, h) = (96u32, 48u32);
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), w, h);
    let mut ctx = WgpuGfxCtx::new(
        Arc::clone(&device),
        Arc::clone(&queue),
        wgpu::TextureFormat::Rgba8Unorm,
        w as f32,
        h as f32,
    );
    ctx.reset(w as f32, h as f32);
    ctx.set_lcd_mode(false);
    ctx.clear(Color::white());
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    ctx.set_font(Arc::clone(&font));
    ctx.set_font_size(12.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 1.0));
    ctx.scale(scale, scale);
    match path {
        TextPath::Mask => ctx.fill_text("HHHH", 4.0, 10.5),
        TextPath::Outline => ctx.fill_text_outline(&font, "HHHH", 4.0, 10.5),
    }
    ctx.flush_to_surface(&target.view);

    let data = target.read();
    let darkness = |y: u32| (0..w).map(|x| 255 - px(&data, w, x, y)[1]).max().unwrap();
    let last = (0..h).rev().find(|&y| darkness(y) > 16)?;
    Some((last, darkness(last)))
}

#[test]
fn mask_and_outline_text_share_a_baseline_row_at_1x_1_5x_and_2x() {
    agg_gui::font_settings::set_hinting_enabled(true);
    for &scale in &[1.0, 1.5, 2.0] {
        let Some(mask) = baseline_row(TextPath::Mask, scale) else {
            eprintln!("SKIP: no GPU adapter");
            return;
        };
        let outline = baseline_row(TextPath::Outline, scale).expect("outline text drew no ink");
        assert_eq!(
            mask.0, outline.0,
            "at {scale}x the mask path's baseline row is {} and the outline path's {}",
            mask.0, outline.0
        );
        assert!(
            outline.1 >= 200,
            "at {scale}x the outline path's bottom row is only {}/255 dark: \
             the baseline sits between device rows",
            outline.1
        );
    }
}
