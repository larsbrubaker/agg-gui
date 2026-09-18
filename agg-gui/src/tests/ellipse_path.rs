//! Unit tests for the `DrawCtx::ellipse` trait default implementation.
//!
//! `ellipse` is lowered to `move_to` + `cubic_to` inside the trait (see
//! `draw_ctx.rs`), so these tests drive it through the shared
//! [`crate::tests::paint_recorder::PaintRecorder`] mock context and assert on
//! the emitted path ops rather than on rasterised pixels.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::draw_ctx::DrawCtx;
use crate::tests::paint_recorder::{PaintRecorder, PathOp};

/// All segment endpoints (`move_to` start point plus every cubic endpoint).
fn endpoints(ops: &[PathOp]) -> Vec<(f64, f64)> {
    ops.iter()
        .filter_map(|op| match *op {
            PathOp::MoveTo(x, y) => Some((x, y)),
            PathOp::CubicTo(_, _, _, _, x, y) => Some((x, y)),
            _ => None,
        })
        .collect()
}

fn cubic_count(ops: &[PathOp]) -> usize {
    ops.iter()
        .filter(|op| matches!(op, PathOp::CubicTo(..)))
        .count()
}

#[test]
fn full_ellipse_emits_four_cubics_on_the_ellipse() {
    let mut ctx = PaintRecorder::new();
    let (cx, cy, rx, ry) = (100.0, 50.0, 40.0, 20.0);
    ctx.begin_path();
    ctx.ellipse(cx, cy, rx, ry, 0.0, 0.0, TAU, false);

    assert_eq!(cubic_count(&ctx.path_ops), 4);
    assert_eq!(ctx.path_ops.last(), Some(&PathOp::ClosePath));
    for (x, y) in endpoints(&ctx.path_ops) {
        let d = ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2);
        assert!(
            (d - 1.0).abs() < 1e-9,
            "endpoint ({x}, {y}) off the ellipse"
        );
    }
}

#[test]
fn quarter_arc_emits_one_cubic_and_is_not_closed() {
    let mut ctx = PaintRecorder::new();
    ctx.begin_path();
    ctx.ellipse(0.0, 0.0, 10.0, 5.0, 0.0, 0.0, FRAC_PI_2, false);

    assert_eq!(cubic_count(&ctx.path_ops), 1);
    assert!(!ctx.path_ops.contains(&PathOp::ClosePath));
    let pts = endpoints(&ctx.path_ops);
    assert!((pts[0].0 - 10.0).abs() < 1e-9 && pts[0].1.abs() < 1e-9);
    assert!(pts[1].0.abs() < 1e-9 && (pts[1].1 - 5.0).abs() < 1e-9);
}

#[test]
fn ccw_reverses_the_sweep() {
    let mut cw = PaintRecorder::new();
    cw.begin_path();
    cw.ellipse(0.0, 0.0, 10.0, 10.0, 0.0, 0.0, FRAC_PI_2, false);

    let mut ccw = PaintRecorder::new();
    ccw.begin_path();
    ccw.ellipse(0.0, 0.0, 10.0, 10.0, 0.0, 0.0, FRAC_PI_2, true);

    // Same start point, opposite direction: increasing angle ends at +y,
    // decreasing angle wraps the long way round and ends at +y as well but
    // via three quarter-turns (sweep = -3π/2).
    let cw_pts = endpoints(&cw.path_ops);
    let ccw_pts = endpoints(&ccw.path_ops);
    assert_eq!(cubic_count(&cw.path_ops), 1);
    assert_eq!(cubic_count(&ccw.path_ops), 3);
    assert!((cw_pts[0].0 - 10.0).abs() < 1e-9);
    assert!((ccw_pts[0].0 - 10.0).abs() < 1e-9);
    // First ccw segment goes to -y, the opposite of the cw one.
    assert!((ccw_pts[1].1 + 10.0).abs() < 1e-9, "{ccw_pts:?}");
    assert!((cw_pts[1].1 - 10.0).abs() < 1e-9, "{cw_pts:?}");
    // Both end at the same place (+y) after their respective sweeps.
    let ccw_end = *ccw_pts.last().expect("endpoint");
    assert!((ccw_end.0).abs() < 1e-9 && (ccw_end.1 - 10.0).abs() < 1e-9);
}

#[test]
fn rotation_half_pi_swaps_the_axes() {
    let mut ctx = PaintRecorder::new();
    ctx.begin_path();
    ctx.ellipse(0.0, 0.0, 10.0, 5.0, FRAC_PI_2, 0.0, TAU, false);

    let pts = endpoints(&ctx.path_ops);
    // Angle 0 in the ellipse frame is (rx, 0) rotated by +90° → (0, rx).
    assert!(
        pts[0].0.abs() < 1e-9 && (pts[0].1 - 10.0).abs() < 1e-9,
        "{pts:?}"
    );
    // Quarter turn: (0, ry) rotated → (-ry, 0).
    assert!(
        (pts[1].0 + 5.0).abs() < 1e-9 && pts[1].1.abs() < 1e-9,
        "{pts:?}"
    );
    for (x, y) in &pts {
        let d = (x / 5.0).powi(2) + (y / 10.0).powi(2);
        assert!((d - 1.0).abs() < 1e-9, "point ({x}, {y}) off the ellipse");
    }
}

#[test]
fn half_turn_emits_two_cubics() {
    let mut ctx = PaintRecorder::new();
    ctx.begin_path();
    ctx.ellipse(0.0, 0.0, 10.0, 10.0, 0.0, 0.0, PI, false);
    assert_eq!(cubic_count(&ctx.path_ops), 2);
}

// ── Direction-aware full-turn detection (canvas-2D spec) ────────────────────
//
// A full ellipse is drawn only when the sweep covers a revolution *in the
// requested direction*; otherwise the sweep is normalised into the half-open
// range for that direction.

#[test]
fn negative_end_angle_cw_normalises_to_a_half_turn() {
    let mut ctx = PaintRecorder::new();
    ctx.begin_path();
    // -3π is not a full turn for `ccw == false`; it normalises to +π.
    ctx.ellipse(0.0, 0.0, 10.0, 10.0, 0.0, 0.0, -3.0 * PI, false);

    assert_eq!(cubic_count(&ctx.path_ops), 2);
    assert!(!ctx.path_ops.contains(&PathOp::ClosePath));
    let end = *endpoints(&ctx.path_ops).last().expect("endpoint");
    assert!(
        (end.0 + 10.0).abs() < 1e-9 && end.1.abs() < 1e-9,
        "half turn must end on the -x axis; got {end:?}"
    );
}

#[test]
fn full_negative_turn_cw_draws_nothing() {
    let mut ctx = PaintRecorder::new();
    ctx.begin_path();
    ctx.ellipse(0.0, 0.0, 10.0, 10.0, 0.0, 0.0, -TAU, false);

    assert_eq!(cubic_count(&ctx.path_ops), 0);
    assert!(!ctx.path_ops.contains(&PathOp::ClosePath));
    assert_eq!(ctx.path_ops, vec![PathOp::MoveTo(10.0, 0.0)]);
}

#[test]
fn full_positive_turn_ccw_draws_nothing() {
    let mut ctx = PaintRecorder::new();
    ctx.begin_path();
    ctx.ellipse(0.0, 0.0, 10.0, 10.0, 0.0, 0.0, TAU, true);

    assert_eq!(cubic_count(&ctx.path_ops), 0);
    assert!(!ctx.path_ops.contains(&PathOp::ClosePath));
    assert_eq!(ctx.path_ops, vec![PathOp::MoveTo(10.0, 0.0)]);
}

#[test]
fn full_positive_turn_cw_is_a_closed_ellipse() {
    let mut ctx = PaintRecorder::new();
    ctx.begin_path();
    ctx.ellipse(0.0, 0.0, 10.0, 10.0, 0.0, 0.0, TAU, false);

    assert_eq!(cubic_count(&ctx.path_ops), 4);
    assert_eq!(ctx.path_ops.last(), Some(&PathOp::ClosePath));
}
