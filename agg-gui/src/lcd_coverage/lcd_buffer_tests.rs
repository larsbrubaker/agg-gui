//! Port of agg-sharp's `LcdBufferTests` (Tests/Agg.Tests/Agg/LcdBufferTests.cs).
//!
//! Covers [`LcdBuffer`]: the two-plane render target that lets per-channel
//! coverage survive a widget backbuffer, plus the ways back out of it
//! (collapse to single alpha, composite onto an image, plane flip).  Same
//! test names (snake_case) and hand-computed expected bytes as the C# class.
//!
//! Mapping of the C# API onto agg-gui:
//! - `ToImageBufferCollapsed` (Y-up BGRA image) is
//!   [`LcdBuffer::to_rgba8_top_down_collapsed`] (top-row-first RGBA), so the
//!   positional checks read the flipped row.
//! - `CompositeOnto(ImageBuffer, x, y, fade)` is the software [`GfxCtx`]'s
//!   `draw_lcd_backbuffer_arc` onto a premultiplied [`Framebuffer`], with
//!   the fade as `global_alpha`; its planes are top-row-first.
//! - `FlipPlane` is the private `flip_plane`.

use super::*;
use crate::framebuffer::Framebuffer;
use agg_rust::basics::PATH_FLAGS_NONE;

use super::lcd_composite_tests::{c8, fractional_triangle};

fn offset(buf: &LcdBuffer, x: u32, y: u32) -> usize {
    ((y * buf.width() + x) * 3) as usize
}

fn set_pixel(buf: &mut LcdBuffer, x: u32, y: u32, color: [u8; 3], alpha: [u8; 3]) {
    let o = offset(buf, x, y);
    buf.color_plane_mut()[o..o + 3].copy_from_slice(&color);
    buf.alpha_plane_mut()[o..o + 3].copy_from_slice(&alpha);
}

fn assert_pixel(buf: &LcdBuffer, x: u32, y: u32, color: [u8; 3], alpha: [u8; 3]) {
    let o = offset(buf, x, y);
    assert_eq!(&buf.color_plane()[o..o + 3], &color, "color at ({x}, {y})");
    assert_eq!(&buf.alpha_plane()[o..o + 3], &alpha, "alpha at ({x}, {y})");
}

fn mask1(data: [u8; 3]) -> LcdMask {
    LcdMask {
        data: data.to_vec(),
        width: 1,
        height: 1,
    }
}

fn full_coverage_mask(w: u32, h: u32) -> LcdMask {
    LcdMask {
        data: vec![255; (w * h * 3) as usize],
        width: w,
        height: h,
    }
}

fn rectangle(left: f64, bottom: f64, right: f64, top: f64) -> PathStorage {
    let mut p = PathStorage::new();
    p.move_to(left, bottom);
    p.line_to(right, bottom);
    p.line_to(right, top);
    p.line_to(left, top);
    p.close_polygon(PATH_FLAGS_NONE);
    p
}

fn white() -> Color {
    c8(255, 255, 255, 255)
}

/// Both planes start transparent: unpainted regions mean "no paint landed".
#[test]
fn fresh_buffer_is_transparent_in_both_planes() {
    let buf = LcdBuffer::new(8, 4);
    assert_eq!(buf.color_plane().len(), 8 * 4 * 3);
    assert_eq!(buf.alpha_plane().len(), 8 * 4 * 3);
    assert!(
        buf.color_plane().iter().all(|&b| b == 0),
        "a fresh color plane must be zero"
    );
    assert!(
        buf.alpha_plane().iter().all(|&b| b == 0),
        "a fresh alpha plane must be zero, which is what 'transparent' means here"
    );
}

/// A pathological size clamps to 1x1 rather than allocating.
#[test]
fn pathological_size_clamps_instead_of_allocating() {
    // 100k x 100k x 3 bytes is 30 GB per plane.
    let buf = LcdBuffer::new(100_000, 100_000);
    assert_eq!(buf.width(), 1);
    assert_eq!(buf.height(), 1);
    assert_eq!(buf.color_plane().len(), 3);
}

/// Clear writes premultiplied colour and the same alpha on every channel.
#[test]
fn clear_writes_premultiplied_color_and_the_same_alpha_on_every_channel() {
    let mut opaque = LcdBuffer::new(4, 3);
    opaque.clear(c8(255, 128, 64, 255));
    for y in 0..3 {
        for x in 0..4 {
            assert_pixel(&opaque, x, y, [255, 128, 64], [255, 255, 255]);
        }
    }
    // Alpha 137/255: 137, 68.8 -> 69, 34.4 -> 34; every alpha 137.
    let mut faded = LcdBuffer::new(2, 2);
    faded.clear(c8(255, 128, 64, 137));
    assert_pixel(&faded, 1, 1, [137, 69, 34], [137, 137, 137]);
}

/// The per-channel composite on one pixel over existing paint, both planes.
#[test]
fn composite_mask_blends_each_channel_into_both_planes() {
    let mut buf = LcdBuffer::new(1, 1);
    set_pixel(&mut buf, 0, 0, [100, 30, 200], [150, 60, 255]);
    buf.composite_mask(&mask1([200, 100, 0]), c8(255, 128, 64, 137), 0, 0, None);
    // R: color 165.31 -> 165, alpha 194.24 -> 194; G: 50.65 -> 51, 101.08 -> 101;
    // B: coverage 0 keeps both planes.
    assert_pixel(&buf, 0, 0, [165, 51, 200], [194, 101, 255]);
}

/// Two composites accumulate per channel, strictly independently.
#[test]
fn repeated_composites_accumulate_per_channel() {
    let mut buf = LcdBuffer::new(1, 1);
    buf.composite_mask(&mask1([255, 128, 0]), white(), 0, 0, None);
    assert_pixel(&buf, 0, 0, [255, 128, 0], [255, 128, 0]);
    buf.composite_mask(&mask1([0, 128, 255]), white(), 0, 0, None);
    // G: 0.501961 + 0.501961 * 0.498039 = 0.751957 -> 192.
    assert_pixel(&buf, 0, 0, [255, 192, 255], [255, 192, 255]);
}

/// The composite clip is half open and intersected with the buffer.
#[test]
fn composite_mask_honours_a_half_open_clip_rect() {
    let mask = full_coverage_mask(4, 4);

    let mut clipped = LcdBuffer::new(4, 4);
    clipped.composite_mask(&mask, white(), 0, 0, Some((1, 1, 3, 3)));
    for y in 0..4 {
        for x in 0..4 {
            let inside = (1..3).contains(&x) && (1..3).contains(&y);
            let e = if inside { 255 } else { 0 };
            assert_pixel(&clipped, x, y, [e; 3], [e; 3]);
        }
    }

    // Clip wider than the buffer: intersected, not trusted.
    let mut wide = LcdBuffer::new(2, 2);
    wide.composite_mask(&mask, white(), 0, 0, Some((-10, -10, 100, 100)));
    assert!(wide.alpha_plane().iter().all(|&b| b == 255));

    // Empty clip paints nothing.
    let mut empty = LcdBuffer::new(2, 2);
    empty.composite_mask(&mask, white(), 0, 0, Some((1, 1, 1, 3)));
    assert!(empty.alpha_plane().iter().all(|&b| b == 0));

    // Mask hanging off the bottom-left corner: only its upper-right 2x2 lands.
    let mut overhang = LcdBuffer::new(4, 4);
    overhang.composite_mask(&mask, white(), -2, -2, None);
    for y in 0..4 {
        for x in 0..4 {
            let e = if x < 2 && y < 2 { 255 } else { 0 };
            assert_pixel(&overhang, x, y, [e; 3], [e; 3]);
        }
    }
}

/// Black ink on a transparent buffer leaves the colour plane at zero.
#[test]
fn black_fill_on_a_transparent_buffer_records_alpha_only() {
    let mut buf = LcdBuffer::new(20, 20);
    let mut rect = rectangle(5.25, 5.5, 15.75, 15.5);
    buf.fill_path(
        &mut rect,
        c8(0, 0, 0, 255),
        &identity_xform(),
        None,
        FillRule::NonZero,
    );
    assert!(
        buf.color_plane().iter().all(|&b| b == 0),
        "premultiplied black is zero, so the color plane must stay zero"
    );
    assert!(
        buf.alpha_plane().iter().any(|&b| b > 0),
        "the alpha plane must record coverage where the path was filled"
    );
    assert_pixel(&buf, 10, 10, [0, 0, 0], [255, 255, 255]);
    assert_pixel(&buf, 0, 0, [0, 0, 0], [0, 0, 0]);
    assert_pixel(&buf, 19, 19, [0, 0, 0], [0, 0, 0]);

    let edge = offset(&buf, 5, 10);
    let a = buf.alpha_plane();
    assert!(
        a[edge] != a[edge + 1] || a[edge + 1] != a[edge + 2],
        "a fractional edge must produce different coverage per channel"
    );
}

/// `fill_path` is exactly `build_bounded_mask` + `composite_mask` at the
/// reported origin, byte for byte in both planes.
#[test]
fn fill_path_is_the_bounded_mask_composited_at_its_reported_origin() {
    let mut transform = TransAffine::new_rotation(0.3);
    transform.multiply(&TransAffine::new_translation(6.25, 4.5));
    let ink = c8(220, 40, 90, 200);

    let mut via_fill_path = LcdBuffer::new(32, 32);
    via_fill_path.clear(white());
    let mut tri = fractional_triangle();
    via_fill_path.fill_path(&mut tri, ink, &transform, None, FillRule::NonZero);

    let mut via_mask = LcdBuffer::new(32, 32);
    via_mask.clear(white());
    let mut tri = fractional_triangle();
    let (mask, ox, oy) = build_bounded_mask(32, 32, &mut tri, &transform, None, FillRule::NonZero)
        .expect("the triangle lands on the buffer");
    via_mask.composite_mask(&mask, ink, ox, oy, None);

    assert!(via_mask.alpha_plane().iter().any(|&b| b > 0));
    assert_eq!(via_mask.color_plane(), via_fill_path.color_plane());
    assert_eq!(via_mask.alpha_plane(), via_fill_path.alpha_plane());
}

/// A fractional clip becomes `[floor(left), ceil(right)) x [floor(bottom),
/// ceil(top))`.  agg-gui's clip is `(x, y, w, h)`, so C#'s
/// `RectangleDouble(5.75, 5.75, 9.25, 9.25)` is `(5.75, 5.75, 3.5, 3.5)`.
#[test]
fn fill_path_clips_to_every_pixel_the_clip_rect_touches() {
    let mut buf = LcdBuffer::new(20, 20);
    let mut rect = rectangle(-5.0, -5.0, 25.0, 25.0);
    buf.fill_path(
        &mut rect,
        white(),
        &identity_xform(),
        Some((5.75, 5.75, 9.25 - 5.75, 9.25 - 5.75)),
        FillRule::NonZero,
    );
    for y in 0..20 {
        for x in 0..20 {
            let o = offset(&buf, x, y);
            let inside = (5..10).contains(&x) && (5..10).contains(&y);
            if inside {
                assert!(buf.alpha_plane()[o] > 0, "({x}, {y}) is inside the clip");
            } else {
                assert_eq!(buf.alpha_plane()[o], 0, "({x}, {y}) is outside the clip");
            }
        }
    }
    assert_pixel(&buf, 7, 7, [255, 255, 255], [255, 255, 255]);
}

/// The collapse uses the Rec.709-weighted mean of the channel alphas over
/// the ink's own straight colour.
#[test]
fn collapse_weights_alpha_by_rec709_over_the_inks_own_colour() {
    let mut light = LcdBuffer::new(1, 1);
    light.composite_mask(&mask1([200, 60, 150]), white(), 0, 0, None);
    assert_pixel(&light, 0, 0, [200, 60, 150], [200, 60, 150]);

    // weighted 96.262 -> alpha 96; the ink is white, so every channel's
    // colour is c_i / a_i = 1. (Channel 1's alpha, 60, is under the 64 below
    // which its colour blends toward the strongest channel's, which is the
    // same white.)
    let straight = collapse_lcd_pixel([200, 60, 150], [200, 60, 150]);
    assert_eq!(straight, [255, 255, 255, 96], "straight-alpha collapse");

    let mut dark = LcdBuffer::new(1, 1);
    dark.composite_mask(&mask1([200, 60, 150]), c8(0, 0, 0, 255), 0, 0, None);
    assert_pixel(&dark, 0, 0, [0, 0, 0], [200, 60, 150]);
    assert_eq!(
        collapse_lcd_pixel([0, 0, 0], [200, 60, 150]),
        [0, 0, 0, 96],
        "dark collapse"
    );
    assert_eq!(
        collapse_lcd_pixel([0, 0, 0], [0, 0, 0]),
        [0, 0, 0, 0],
        "empty collapse"
    );
}

/// The whole-buffer flatten lands each collapsed pixel at its position, in
/// R, G, B, A order.  The output is top-row-first, so Y-up `(x, y)` reads
/// row `h - 1 - y`.
#[test]
fn collapsed_image_keeps_position_and_channel_order() {
    let mut buf = LcdBuffer::new(2, 2);
    buf.composite_mask(&mask1([200, 60, 150]), white(), 1, 0, None);
    let img = buf.to_rgba8_top_down_collapsed();
    assert_eq!(img.len(), 2 * 2 * 4);
    let read = |x: usize, y_up: usize| {
        let i = ((1 - y_up) * 2 + x) * 4;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    };
    assert_eq!(read(1, 0), [255, 255, 255, 96], "collapsed (1, 0)");
    assert_eq!(read(0, 0), [0, 0, 0, 0], "collapsed (0, 0)");
    assert_eq!(read(1, 1), [0, 0, 0, 0], "collapsed (1, 1)");
}

/// Composite an LCD buffer onto a premultiplied framebuffer with the
/// per-channel primitive (C# `LcdBuffer.CompositeOnto`), `fade` as global
/// alpha.  The primitive, not `GfxCtx::draw_lcd_backbuffer_arc`: that one is
/// C#'s `CompositeLcdBuffer` and refuses destinations that are not opaque
/// (`lcd_backbuffer_opacity_gate`), which these tests deliberately use.
fn composite_onto(src: &LcdBuffer, fb: &mut Framebuffer, fade: f64) {
    let color = src.color_plane_flipped();
    let alpha = src.alpha_plane_flipped();
    let (w, h) = (src.width(), src.height());
    crate::gfx_ctx::composite_lcd_planes_per_channel(
        fb,
        &color,
        &alpha,
        w,
        h,
        0,
        0,
        fade.clamp(0.0, 1.0) as f32,
        None,
    );
}

fn premultiplied_image(w: u32, h: u32, fill: [u8; 4]) -> Framebuffer {
    let mut fb = Framebuffer::new(w, h);
    for px in fb.pixels_mut().chunks_exact_mut(4) {
        px.copy_from_slice(&fill);
    }
    fb
}

/// Per channel `dest = source + dest * (1 - source_alpha)`; the single
/// destination alpha takes `max` over the three source alphas.
#[test]
fn composite_onto_image_blends_per_channel_and_takes_max_alpha() {
    let mut source = LcdBuffer::new(2, 1);
    set_pixel(&mut source, 0, 0, [200, 60, 150], [200, 60, 150]);
    let mut dst = premultiplied_image(2, 1, [100, 30, 210, 150]);
    composite_onto(&source, &mut dst, 1.0);
    assert_eq!(&dst.pixels()[0..4], &[222, 83, 236, 232], "blended pixel");
    assert_eq!(&dst.pixels()[4..8], &[100, 30, 210, 150], "unpainted pixel");

    let mut opaque = LcdBuffer::new(1, 1);
    set_pixel(&mut opaque, 0, 0, [255; 3], [255; 3]);
    let mut faded = premultiplied_image(1, 1, [0, 0, 0, 0]);
    composite_onto(&opaque, &mut faded, 0.5);
    assert_eq!(
        &faded.pixels()[0..4],
        &[128, 128, 128, 128],
        "half-faded pixel"
    );
}

/// Buffer onto buffer keeps the per-channel alpha.
#[test]
fn composite_buffer_blends_both_planes_per_channel() {
    let mut source = LcdBuffer::new(2, 1);
    set_pixel(&mut source, 0, 0, [200, 60, 150], [200, 60, 150]);
    let mut dst = LcdBuffer::new(2, 1);
    set_pixel(&mut dst, 0, 0, [100, 30, 200], [150, 60, 255]);
    set_pixel(&mut dst, 1, 0, [10, 20, 30], [40, 50, 60]);
    dst.composite_buffer(&source, 0, 0, None);
    assert_pixel(&dst, 0, 0, [222, 83, 232], [232, 106, 255]);
    assert_pixel(&dst, 1, 0, [10, 20, 30], [40, 50, 60]);
}

/// `flip_plane` reverses row order and is its own inverse.
#[test]
fn flip_plane_reverses_row_order_and_round_trips() {
    let mut plane = vec![0u8; 2 * 3 * 3];
    for y in 0..3 {
        for i in 0..6 {
            plane[y * 6 + i] = (y * 10 + i) as u8;
        }
    }
    let flipped = flip_plane(&plane, 2, 3);
    let expected = [
        20u8, 21, 22, 23, 24, 25, 10, 11, 12, 13, 14, 15, 0, 1, 2, 3, 4, 5,
    ];
    assert_eq!(flipped, expected, "flipped plane");
    assert_eq!(flip_plane(&flipped, 2, 3), plane, "round-tripped plane");

    let mut buf = LcdBuffer::new(2, 3);
    set_pixel(&mut buf, 0, 0, [1, 2, 3], [4, 5, 6]);
    assert_eq!(
        flip_plane(buf.color_plane(), 2, 3),
        buf.color_plane_flipped()
    );
    assert_eq!(
        flip_plane(buf.alpha_plane(), 2, 3),
        buf.alpha_plane_flipped()
    );
    assert_eq!(buf.color_plane_flipped()[2 * 6], 1);
    assert_eq!(buf.alpha_plane_flipped()[2 * 6], 4);
}
