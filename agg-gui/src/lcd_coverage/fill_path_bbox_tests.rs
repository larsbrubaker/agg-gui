//! Reference-equivalence tests for the bbox-bounded `LcdBuffer::fill_path`.
//!
//! `fill_path` was optimized to size its coverage mask to the transformed
//! path's bounding box instead of the whole buffer (per-fill cost went from
//! O(buffer) to O(bbox)).  Correctness hinges on that producing pixels
//! byte-identical to the original full-buffer construction — a TextArea
//! dirty-strip edit must stay pixel-for-pixel identical to a full repaint.
//!
//! Each test fills a path two ways and asserts both planes are byte-equal:
//!   A. production `LcdBuffer::fill_path` (bbox-bounded);
//!   B. `fill_path_reference` below, which replicates the pre-optimization
//!      full-buffer code verbatim (full-size `LcdMaskBuilder` + composite at
//!      origin 0,0 with the composite-time clip).
//! If these diverge the optimization changed visible output.

use super::*;
use agg_rust::basics::PATH_FLAGS_NONE;

/// Verbatim copy of the pre-optimization `fill_path` body: a full-buffer
/// mask composited at (0, 0).  Kept here (not in production) so the test
/// pins the *old* behaviour as the reference oracle.
fn fill_path_reference(
    buf: &mut LcdBuffer,
    path: &mut PathStorage,
    color: Color,
    transform: &TransAffine,
    clip: Option<(f64, f64, f64, f64)>,
    rule: FillRule,
) {
    if buf.width() == 0 || buf.height() == 0 {
        return;
    }
    let mut builder = LcdMaskBuilder::new(buf.width(), buf.height())
        .with_clip(clip)
        .with_fill_rule(rule);
    builder.with_paths(transform, |add| {
        add(path);
    });
    let mask = builder.finalize();
    let clip_i = clip.map(rect_to_pixel_clip);
    buf.composite_mask(&mask, color, 0, 0, clip_i);
}

/// Fill `make_path()` into two buffers — production vs. reference — and
/// assert both planes byte-equal.  `prefill` optionally clears both buffers
/// to a solid colour first (exercises the src-over composite interaction).
#[allow(clippy::too_many_arguments)]
fn assert_fill_equiv<P: FnMut() -> PathStorage>(
    w: u32,
    h: u32,
    prefill: Option<Color>,
    mut make_path: P,
    color: Color,
    transform: &TransAffine,
    clip: Option<(f64, f64, f64, f64)>,
    rule: FillRule,
    label: &str,
) {
    let mut a = LcdBuffer::new(w, h);
    let mut b = LcdBuffer::new(w, h);
    if let Some(c) = prefill {
        a.clear(c);
        b.clear(c);
    }
    let mut pa = make_path();
    let mut pb = make_path();
    a.fill_path(&mut pa, color, transform, clip, rule);
    fill_path_reference(&mut b, &mut pb, color, transform, clip, rule);
    assert_eq!(
        a.color_plane(),
        b.color_plane(),
        "color plane mismatch: {label}"
    );
    assert_eq!(
        a.alpha_plane(),
        b.alpha_plane(),
        "alpha plane mismatch: {label}"
    );
}

fn rect(x1: f64, y1: f64, x2: f64, y2: f64) -> PathStorage {
    let mut p = PathStorage::new();
    p.move_to(x1, y1);
    p.line_to(x2, y1);
    p.line_to(x2, y2);
    p.line_to(x1, y2);
    p.close_polygon(PATH_FLAGS_NONE);
    p
}

/// The measured hot shape: an axis-aligned strip-background rect at
/// fractional coords in the middle of the buffer.
#[test]
fn fill_path_bbox_axis_aligned_rect() {
    assert_fill_equiv(
        120,
        60,
        Some(Color::white()),
        || rect(12.3, 8.7, 96.4, 30.2),
        Color::rgba(0.1, 0.2, 0.3, 1.0),
        &TransAffine::new(),
        None,
        FillRule::NonZero,
        "axis-aligned strip rect",
    );
}

/// Same rect but with a clip rect that partially overlaps it — the bounded
/// mask is intersected with the clip, and the reference clips at composite
/// time; both must land the identical pixels.
#[test]
fn fill_path_bbox_rect_with_partial_clip() {
    assert_fill_equiv(
        120,
        60,
        Some(Color::white()),
        || rect(12.3, 8.7, 96.4, 30.2),
        Color::black(),
        &TransAffine::new(),
        // Clip covers only the left portion of the rect.
        Some((20.0, 5.0, 40.0, 40.0)),
        FillRule::NonZero,
        "rect with partial clip",
    );
}

/// Non-identity transform with rotation + scale: a triangle rotated and
/// scaled into the buffer.  The bbox is computed from transformed vertices.
#[test]
fn fill_path_bbox_rotated_scaled_triangle() {
    let mut t = TransAffine::new_rotation(0.35);
    t.scale(1.6, 1.2);
    t.translate(55.0, 30.0);
    let make = || {
        let mut p = PathStorage::new();
        p.move_to(-15.0, -10.0);
        p.line_to(18.0, -6.0);
        p.line_to(2.0, 16.0);
        p.close_polygon(PATH_FLAGS_NONE);
        p
    };
    assert_fill_equiv(
        130,
        70,
        Some(Color::white()),
        make,
        Color::rgba(0.9, 0.1, 0.2, 1.0),
        &t,
        None,
        FillRule::NonZero,
        "rotated + scaled triangle",
    );
}

/// A path with a cubic Bézier: the vertex (control-hull) bbox is a
/// conservative superset of the flattened curve, so bounding by control
/// points must still capture every rasterized pixel.
#[test]
fn fill_path_bbox_cubic_curve() {
    let make = || {
        let mut p = PathStorage::new();
        p.move_to(15.0, 15.0);
        // Control points push well above the endpoints (y ~ 48) — the
        // flattened curve stays within this hull.
        p.curve4(22.0, 48.0, 70.0, 48.0, 78.0, 15.0);
        p.line_to(78.0, 15.0);
        p.close_polygon(PATH_FLAGS_NONE);
        p
    };
    assert_fill_equiv(
        100,
        60,
        Some(Color::white()),
        make,
        Color::rgba(0.2, 0.5, 0.1, 1.0),
        &TransAffine::new(),
        None,
        FillRule::NonZero,
        "cubic curve control-hull bbox",
    );
}

/// A quadratic Bézier variant — same control-hull reasoning.
#[test]
fn fill_path_bbox_quadratic_curve() {
    let make = || {
        let mut p = PathStorage::new();
        p.move_to(10.0, 12.0);
        p.curve3(40.0, 45.0, 70.0, 12.0);
        p.line_to(70.0, 12.0);
        p.close_polygon(PATH_FLAGS_NONE);
        p
    };
    assert_fill_equiv(
        90,
        55,
        Some(Color::white()),
        make,
        Color::black(),
        &TransAffine::new(),
        None,
        FillRule::NonZero,
        "quadratic curve control-hull bbox",
    );
}

/// A path partially off the left/bottom (negative coords): the bbox clamps
/// to the buffer rect and the visible remainder must match the reference.
#[test]
fn fill_path_bbox_partially_outside() {
    assert_fill_equiv(
        60,
        40,
        Some(Color::white()),
        || rect(-12.5, -6.5, 18.5, 22.5),
        Color::black(),
        &TransAffine::new(),
        None,
        FillRule::NonZero,
        "rect partially off bottom-left",
    );
}

/// A path entirely outside the buffer must be a no-op: the bounded mask is
/// empty and the buffer is left as the reference leaves it (untouched).
#[test]
fn fill_path_bbox_entirely_outside() {
    // Off past the top-right corner.
    assert_fill_equiv(
        60,
        40,
        Some(Color::white()),
        || rect(200.0, 200.0, 260.0, 260.0),
        Color::black(),
        &TransAffine::new(),
        None,
        FillRule::NonZero,
        "rect entirely off top-right",
    );
    // Off past the bottom-left corner.
    assert_fill_equiv(
        60,
        40,
        Some(Color::white()),
        || rect(-260.0, -260.0, -200.0, -200.0),
        Color::black(),
        &TransAffine::new(),
        None,
        FillRule::NonZero,
        "rect entirely off bottom-left",
    );
}

/// Rects flush against each buffer edge — checks the 2px filter-leak
/// padding is clamped correctly at the buffer boundary (no over/under-read).
#[test]
fn fill_path_bbox_flush_against_edges() {
    let w = 50u32;
    let h = 30u32;
    let cases: [(f64, f64, f64, f64, &str); 4] = [
        (0.0, 8.0, 10.0, 22.0, "flush left"),
        (w as f64 - 10.0, 8.0, w as f64, 22.0, "flush right"),
        (10.0, 0.0, 40.0, 8.0, "flush bottom"),
        (10.0, h as f64 - 8.0, 40.0, h as f64, "flush top"),
    ];
    for (x1, y1, x2, y2, label) in cases {
        assert_fill_equiv(
            w,
            h,
            Some(Color::white()),
            || rect(x1, y1, x2, y2),
            Color::black(),
            &TransAffine::new(),
            None,
            FillRule::NonZero,
            label,
        );
    }
}

/// EvenOdd fill rule with a self-overlapping path: an outer rect containing
/// an inner rect wound the same direction leaves a hole under EvenOdd.  The
/// bbox spans the outer rect; the fill rule must be honoured identically.
#[test]
fn fill_path_bbox_evenodd_self_overlapping() {
    let make = || {
        let mut p = PathStorage::new();
        // Outer rect.
        p.move_to(10.0, 10.0);
        p.line_to(70.0, 10.0);
        p.line_to(70.0, 50.0);
        p.line_to(10.0, 50.0);
        p.close_polygon(PATH_FLAGS_NONE);
        // Inner rect (same winding) — EvenOdd punches a hole here.
        p.move_to(25.0, 22.0);
        p.line_to(55.0, 22.0);
        p.line_to(55.0, 38.0);
        p.line_to(25.0, 38.0);
        p.close_polygon(PATH_FLAGS_NONE);
        p
    };
    assert_fill_equiv(
        90,
        60,
        Some(Color::white()),
        make,
        Color::rgba(0.15, 0.25, 0.35, 1.0),
        &TransAffine::new(),
        None,
        FillRule::EvenOdd,
        "evenodd self-overlapping rects",
    );
}

/// Non-opaque colour (alpha 0.5) over pre-existing buffer content: the
/// bounded composite must interact with the destination via src-over
/// identically to the full-buffer reference.
#[test]
fn fill_path_bbox_non_opaque_over_content() {
    assert_fill_equiv(
        80,
        50,
        // Pre-fill with a non-trivial colour so src-over is observable.
        Some(Color::rgba(0.8, 0.6, 0.4, 1.0)),
        || rect(15.4, 12.6, 60.4, 34.6),
        Color::rgba(0.0, 0.0, 1.0, 0.5),
        &TransAffine::new(),
        None,
        FillRule::NonZero,
        "half-alpha blue over solid content",
    );
}
