//! Port of agg-sharp's `LcdMaskBuilderTests` (Tests/Agg.Tests/Agg/LcdMaskBuilderTests.cs).
//!
//! Covers [`LcdMaskBuilder`] and [`build_bounded_mask`]: the first stage of
//! the LCD pipeline, which rasterizes vector paths into a 3×-horizontally-
//! supersampled gray coverage buffer and hands it to the filter.  Same test
//! names (snake_case), values and tolerances as the C# class.
//!
//! Mapping of the C#-only machinery onto agg-gui:
//! - `BlenderGrayExact.Lerp/Multiply` (C#'s copy of the reference's rounding
//!   lerp) is agg-rust's [`Gray8::lerp`] / [`Gray8::multiply`], which
//!   `PixfmtGray8` blends with.
//! - `SaturatingMath.Floor/Ceiling` is Rust's saturating `f64 as i32` (NaN
//!   gives 0), reached here through [`rect_to_pixel_clip`]; `MultiplyBy3` is
//!   `i32::saturating_mul(3)`, as the builder's clip uses.
//! - C#'s finalizers don't consume the builder; agg-gui's do, so the
//!   two-finalizer tests finalize a clone of the same raster.
//! - `VertexSourceApplyTransform` becomes the transform applied to the
//!   rectangle's vertices by hand.

use super::lcd_composite_tests::fractional_triangle;
use super::*;
use agg_rust::basics::PATH_FLAGS_NONE;
use agg_rust::color::Gray8;

fn rectangle(left: f64, bottom: f64, right: f64, top: f64) -> PathStorage {
    let mut p = PathStorage::new();
    p.move_to(left, bottom);
    p.line_to(right, bottom);
    p.line_to(right, top);
    p.line_to(left, top);
    p.close_polygon(PATH_FLAGS_NONE);
    p
}

fn shallow_wedge() -> PathStorage {
    let mut p = PathStorage::new();
    p.move_to(-40.0, 2.1);
    p.line_to(40.0, 5.3);
    p.line_to(40.0, 5.9);
    p.line_to(-40.0, 2.7);
    p.close_polygon(PATH_FLAGS_NONE);
    p
}

fn add(builder: &mut LcdMaskBuilder, transform: &TransAffine, mut path: PathStorage) {
    builder.with_paths(transform, |add| add(&mut path));
}

fn id() -> TransAffine {
    identity_xform()
}

fn bounded(
    w: u32,
    h: u32,
    mut path: PathStorage,
    transform: &TransAffine,
    clip: Option<(f64, f64, f64, f64)>,
) -> Option<(LcdMask, i32, i32)> {
    build_bounded_mask(w, h, &mut path, transform, clip, FillRule::NonZero)
}

fn assert_gray_row(expected: &[u8], gray: &[u8], gray_w: u32, row: usize) {
    assert_eq!(expected.len(), gray_w as usize);
    let start = row * gray_w as usize;
    for (sub, &want) in expected.iter().enumerate() {
        assert_eq!(
            gray[start + sub],
            want,
            "gray row {row} subpixel {sub} (pixel {})",
            sub / 3
        );
    }
}

fn assert_builder_row(expected: &[u8], b: &LcdMaskBuilder, row: usize) {
    assert_gray_row(expected, b.gray_buffer(), b.gray_width(), row);
}

fn assert_mask_bytes(expected: &[u8], mask: &LcdMask) {
    assert_eq!(mask.data.len(), expected.len());
    for (i, (&got, &want)) in mask.data.iter().zip(expected).enumerate() {
        assert_eq!(
            got,
            want,
            "mask byte {i} (pixel {}, channel {})",
            i / 3,
            ["R", "G", "B"][i % 3]
        );
    }
}

/// Only X is supersampled: x 1..3 becomes subpixels 3..8 on one gray row.
#[test]
fn integer_rectangle_fills_whole_subpixel_columns_on_one_row() {
    let mut b = LcdMaskBuilder::new(4, 3);
    add(&mut b, &id(), rectangle(1.0, 1.0, 3.0, 2.0));
    assert_eq!(b.gray_width(), 12);
    assert_eq!(b.gray_height(), 3);
    assert_builder_row(&[0; 12], &b, 0);
    assert_builder_row(&[0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 0], &b, 1);
    assert_builder_row(&[0; 12], &b, 2);
}

/// The same rectangle through the filter: a per-channel edge ramp.
#[test]
fn integer_rectangle_filters_to_a_per_channel_edge_ramp() {
    let mut b = LcdMaskBuilder::new(4, 3);
    add(&mut b, &id(), rectangle(1.0, 1.0, 3.0, 2.0));
    let mask = b.finalize();
    assert_eq!(mask.width, 4);
    assert_eq!(mask.height, 3);
    let mut expected = vec![0u8; 12];
    expected.extend_from_slice(&[0, 28, 85, 170, 226, 255, 255, 226, 170, 85, 28, 0]);
    expected.extend_from_slice(&[0; 12]);
    assert_mask_bytes(&expected, &mask);
}

/// A half-covered subpixel blends white at cover 128 to exactly 128.
#[test]
fn half_covered_subpixel_rounds_to_exactly_half() {
    let mut b = LcdMaskBuilder::new(4, 1);
    add(&mut b, &id(), rectangle(1.0, 0.0, 1.5, 1.0));
    assert_builder_row(&[0, 0, 0, 255, 128, 0, 0, 0, 0, 0, 0, 0], &b, 0);
}

/// The rounding lerp reaches both endpoints in both directions.
#[test]
fn exact_blender_lerp_reaches_both_endpoints_and_rounds_half_up() {
    assert_eq!(Gray8::lerp(0, 255, 0), 0);
    assert_eq!(Gray8::lerp(0, 255, 128), 128);
    assert_eq!(Gray8::lerp(0, 255, 255), 255);
    // Downward: without the "- (p > q)" term this misses 0.
    assert_eq!(Gray8::lerp(255, 0, 255), 0);
    assert_eq!(Gray8::lerp(255, 0, 128), 127);
    assert_eq!(Gray8::lerp(255, 0, 0), 255);
    for cover in 0..=255u8 {
        assert_eq!(Gray8::multiply(255, cover), cover);
    }
}

/// A second path adds coverage into the same gray buffer.
#[test]
fn second_path_accumulates_into_the_same_gray_buffer() {
    let mut b = LcdMaskBuilder::new(4, 1);
    add(&mut b, &id(), rectangle(0.0, 0.0, 2.0, 1.0));
    let after_first = b.gray_buffer().to_vec();
    add(&mut b, &id(), rectangle(1.0, 0.0, 3.0, 1.0));
    assert_gray_row(
        &[255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0],
        &after_first,
        12,
        0,
    );
    assert_builder_row(
        &[255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0],
        &b,
        0,
    );
}

/// The clip maps onto the 3×-wide grid with an inclusive right edge
/// `ceil(right) * 3 - 1`.  agg-gui's clip is `(x, y, w, h)`, so C#'s
/// `RectangleDouble(1, 0, 2.5, 1)` is `(1, 0, 1.5, 1)`.
#[test]
fn clip_rect_drops_coverage_outside_it_on_subpixel_boundaries() {
    let mut b = LcdMaskBuilder::new(4, 1).with_clip(Some((1.0, 0.0, 1.5, 1.0)));
    add(&mut b, &id(), rectangle(0.0, 0.0, 4.0, 1.0));
    assert_builder_row(&[0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 0], &b, 0);
    assert_eq!(b.gray_buffer()[8], 255);
    assert_eq!(b.gray_buffer()[9], 0);
}

/// Geometry entirely outside the clip contributes nothing.
#[test]
fn geometry_fully_outside_the_clip_produces_no_coverage() {
    let mut b = LcdMaskBuilder::new(6, 2).with_clip(Some((0.0, 0.0, 2.0, 2.0)));
    add(&mut b, &id(), rectangle(3.0, 0.0, 5.0, 2.0));
    assert!(b.gray_buffer().iter().all(|&v| v == 0));
    assert!(b.finalize().data.iter().all(|&v| v == 0));
}

/// The same path at two integer offsets gives byte-identical masks.
#[test]
fn bounded_mask_is_identical_at_different_integer_offsets() {
    let (at_origin, ox, oy) = bounded(64, 64, fractional_triangle(), &id(), None).expect("origin");
    let (shifted, sx, sy) = bounded(
        64,
        64,
        fractional_triangle(),
        &TransAffine::new_translation(7.0, 5.0),
        None,
    )
    .expect("shifted");
    assert_eq!(sx, ox + 7);
    assert_eq!(sy, oy + 5);
    assert_eq!(ox, 8);
    assert_eq!(oy, 8);
    assert_eq!(shifted.width, at_origin.width);
    assert_eq!(shifted.height, at_origin.height);
    assert!(at_origin.data.iter().any(|&v| v > 0));
    assert_mask_bytes(&at_origin.data, &shifted);
}

/// A rotated transform rasterizes exactly as the rotation baked into the
/// path's vertices.
#[test]
fn rotated_transform_matches_the_same_rotation_baked_into_the_path() {
    let mut rotation = TransAffine::new_translation(-8.0, -6.0);
    rotation.multiply(&TransAffine::new_rotation(30.0_f64.to_radians()));
    rotation.multiply(&TransAffine::new_translation(8.0, 6.0));
    assert!(rotation.shx.abs() > 0.4);
    assert!((rotation.shy - -rotation.shx).abs() <= 1e-12);

    let (l, bt, r, t) = (4.25, 3.5, 11.75, 8.5);
    let mut via_transform = LcdMaskBuilder::new(16, 12);
    add(&mut via_transform, &rotation, rectangle(l, bt, r, t));

    let mut baked = PathStorage::new();
    for (i, (mut x, mut y)) in [(l, bt), (r, bt), (r, t), (l, t)].into_iter().enumerate() {
        rotation.transform(&mut x, &mut y);
        if i == 0 {
            baked.move_to(x, y);
        } else {
            baked.line_to(x, y);
        }
    }
    baked.close_polygon(PATH_FLAGS_NONE);
    let mut via_baked = LcdMaskBuilder::new(16, 12);
    add(&mut via_baked, &id(), baked);

    let rotated = via_transform.finalize();
    assert_mask_bytes(&via_baked.finalize().data, &rotated);

    let mut unrotated = LcdMaskBuilder::new(16, 12);
    add(&mut unrotated, &id(), rectangle(l, bt, r, t));
    assert!(rotated.data.iter().any(|&v| v > 0));
    assert_ne!(rotated.data, unrotated.finalize().data);
}

/// Nothing to paint when the padded bbox misses the destination.
#[test]
fn bounded_mask_reports_nothing_to_paint_when_fully_off_buffer() {
    assert!(bounded(64, 64, rectangle(-50.0, -50.0, -40.0, -40.0), &id(), None).is_none());
}

/// A `MIN..MAX` "unbounded" clip behaves exactly as no clip.  C#'s
/// `RectangleDouble(MinValue, MinValue, MaxValue, MaxValue)` is
/// `(MIN, MIN, MAX - MIN, MAX - MIN)` as agg-gui's `(x, y, w, h)`.
#[test]
fn bounded_mask_treats_an_unbounded_clip_sentinel_as_unclipped() {
    let unbounded = Some((f64::MIN, f64::MIN, f64::MAX - f64::MIN, f64::MAX - f64::MIN));
    let (plain, px, py) = bounded(64, 64, fractional_triangle(), &id(), None).expect("plain");
    let (sentinel, sx, sy) =
        bounded(64, 64, fractional_triangle(), &id(), unbounded).expect("sentinel");
    assert_eq!(sx, px);
    assert_eq!(sy, py);
    assert!(plain.data.iter().any(|&v| v > 0));
    assert_mask_bytes(&plain.data, &sentinel);
}

/// Out-of-range bounds clamp to the nearest `i32` limit instead of
/// inverting a region; NaN gives 0.
#[test]
fn out_of_range_bounds_saturate_rather_than_inverting() {
    // (floor(v), floor(v), ceil(v), ceil(v)) through the production conversion.
    let fc = |v: f64| {
        let (f, _, c, _) = rect_to_pixel_clip((v, v, 0.0, 0.0));
        (f, c)
    };
    let huge = 1e12;
    assert_eq!(fc(huge).1, i32::MAX);
    assert_eq!(fc(f64::MAX).1, i32::MAX);
    assert_eq!(fc(f64::INFINITY).1, i32::MAX);
    assert_eq!(fc(-huge).0, i32::MIN);
    assert_eq!(fc(f64::MIN).0, i32::MIN);
    assert_eq!(fc(f64::NEG_INFINITY).0, i32::MIN);
    assert_eq!(fc(2.5), (2, 3));
    assert_eq!(fc(-2.5), (-3, -2));
    assert_eq!(fc(f64::NAN), (0, 0));
    assert_eq!(5i32.saturating_mul(3), 15);
    assert_eq!(i32::MAX.saturating_mul(3), i32::MAX);
    assert_eq!(i32::MIN.saturating_mul(3), i32::MIN);
}

/// Degenerate input (no area, no vertices) hands back no mask.
#[test]
fn bounded_mask_reports_nothing_to_paint_for_degenerate_input() {
    assert!(bounded(0, 64, rectangle(1.0, 1.0, 3.0, 3.0), &id(), None).is_none());
    assert!(bounded(64, 0, rectangle(1.0, 1.0, 3.0, 3.0), &id(), None).is_none());
    assert!(bounded(64, 64, PathStorage::new(), &id(), None).is_none());
}

/// The gray sibling has the LCD mask's dimensions and no chroma; the LCD
/// mask over the same raster varies within a pixel.
#[test]
fn gray_finalize_matches_lcd_dimensions_and_carries_no_chroma() {
    let mut b = LcdMaskBuilder::new(4, 1);
    add(&mut b, &id(), rectangle(1.0, 0.0, 3.0, 1.0));
    let lcd = b.clone().finalize();
    let gray = b.finalize_gray();
    assert_eq!(gray.width, lcd.width);
    assert_eq!(gray.height, lcd.height);
    assert_eq!(gray.data.len(), lcd.data.len());
    for px in gray.data.chunks(3) {
        assert_eq!(px[0], px[1]);
        assert_eq!(px[1], px[2]);
    }
    assert!(lcd.data.chunks(3).any(|p| p[0] != p[1] || p[1] != p[2]));
}

/// A fractional path varies per channel (max > 20, max - min > 10) while the
/// gray sibling has r == g == b and partial-coverage bytes.
#[test]
fn fractional_path_varies_per_channel_while_the_gray_sibling_does_not() {
    let mut b = LcdMaskBuilder::new(20, 20);
    add(&mut b, &id(), fractional_triangle());
    let lcd = b.clone().finalize();
    let saw_variation = lcd.data.chunks(3).any(|p| {
        let max = p[0].max(p[1]).max(p[2]) as i32;
        let min = p[0].min(p[1]).min(p[2]) as i32;
        max > 20 && max - min > 10
    });
    assert!(saw_variation, "no per-channel variation at the path edges");
    let gray = b.finalize_gray();
    for px in gray.data.chunks(3) {
        assert_eq!(px[0], px[1]);
        assert_eq!(px[1], px[2]);
    }
    assert!(
        gray.data.iter().any(|&v| v > 8 && v < 248),
        "the gray mask has no partial-coverage bytes - its edges are aliased"
    );
}

/// The rasterizer's clip box may not change a single byte of geometry
/// running off the buffer: the comparison is against no box at all.
#[test]
fn geometry_crossing_the_buffer_edge_is_rasterized_as_if_unclipped() {
    let mut clipped = LcdMaskBuilder::new(8, 8);
    add(&mut clipped, &id(), shallow_wedge());
    let mut unclipped = LcdMaskBuilder::new_with_unclipped_rasterizer(8, 8);
    add(&mut unclipped, &id(), shallow_wedge());
    assert!(
        unclipped.gray_buffer().iter().any(|&v| v > 0 && v < 255),
        "the wedge left no partial coverage, so there is nothing for a clip to shift"
    );
    let gw = unclipped.gray_width() as usize;
    for (i, (&got, &want)) in clipped
        .gray_buffer()
        .iter()
        .zip(unclipped.gray_buffer())
        .enumerate()
    {
        assert_eq!(got, want, "gray row {} subpixel {}", i / gw, i % gw);
    }
}

/// An extreme coordinate clips rather than overflowing the rasterizer.
///
/// agg-rust workaround: ignored until agg-rust's `RasterizerSlClipInt`
/// computes its clip differences (`x2 - x1`, `y2 - y1`) without `i32`
/// overflow; a 1e12 coordinate upscales to a saturated `i32::MIN/MAX` and
/// the subtraction panics in an overflow-checked build.
#[test]
#[ignore = "agg-rust: rasterizer_sl_clip overflows i32 on saturated coordinates (upstream fix pending)"]
fn extreme_coordinates_clip_instead_of_overflowing_the_rasterizer() {
    let huge = 1e12;
    let mut everywhere = LcdMaskBuilder::new(4, 3);
    add(&mut everywhere, &id(), rectangle(-huge, -huge, huge, huge));
    let mut whole = LcdMaskBuilder::new(4, 3);
    add(&mut whole, &id(), rectangle(-8.0, -8.0, 8.0, 8.0));
    let whole_mask = whole.finalize();
    assert!(
        whole_mask.data.iter().all(|&v| v > 0),
        "a rectangle over the whole buffer must leave coverage in every mask byte"
    );
    assert_mask_bytes(&whole_mask.data, &everywhere.finalize());

    let mut off_right = LcdMaskBuilder::new(4, 3);
    add(&mut off_right, &id(), rectangle(1.0, 1.0, huge, 2.0));
    let mut in_bounds = LcdMaskBuilder::new(4, 3);
    add(&mut in_bounds, &id(), rectangle(1.0, 1.0, 8.0, 2.0));
    let in_bounds_mask = in_bounds.finalize();
    assert!(in_bounds_mask.data.iter().any(|&v| v > 0));
    assert_mask_bytes(&in_bounds_mask.data, &off_right.finalize());
}
