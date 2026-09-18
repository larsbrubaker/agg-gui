//! Free-function bodies for [`crate::draw_ctx::DrawCtx`]'s default methods.
//!
//! The `DrawCtx` trait deliberately ships working defaults for a handful of
//! methods (elliptical arcs, pixel snapping, the corner-quad and LCD-plane
//! fallbacks) so every backend gets them for free.  Their *logic* lives here,
//! as plain functions, and the trait defaults are thin adapters that replay
//! the result through the backend's own primitives.
//!
//! Splitting them out keeps `draw_ctx.rs` — which is primarily an interface
//! definition plus its documentation — inside the project's file-length
//! budget, and makes the maths unit-testable without a rendering backend.

use agg_rust::trans_affine::TransAffine;

// ---------------------------------------------------------------------------
// Elliptical arcs
// ---------------------------------------------------------------------------

/// One path operation emitted by [`ellipse_ops`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EllipseOp {
    MoveTo(f64, f64),
    CubicTo(f64, f64, f64, f64, f64, f64),
    ClosePath,
}

/// Lower a canvas-2D `ellipse(...)` call to a `move_to` + cubic Bézier chain.
///
/// Direction handling follows the canvas spec: the arc is a **full turn** only
/// when the sweep runs at least a whole revolution *in the requested
/// direction* (`end - start >= 2π` clockwise-in-angle, i.e. `!ccw`; or
/// `start - end >= 2π` for `ccw`).  Otherwise the sweep is normalised into the
/// half-open range `[0, 2π)` (`!ccw`) or `(-2π, 0]` (`ccw`), so e.g.
/// `ellipse(.., 0.0, -TAU, false)` degenerates to a zero-length arc rather
/// than silently drawing a full ellipse the wrong way round.
#[allow(clippy::too_many_arguments)]
pub fn ellipse_ops(
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    rotation: f64,
    start_angle: f64,
    end_angle: f64,
    ccw: bool,
) -> Vec<EllipseOp> {
    use std::f64::consts::{FRAC_PI_2, TAU};

    let (sin_r, cos_r) = rotation.sin_cos();
    // Point on the rotated, translated ellipse at parameter `t`.
    let point = |t: f64| {
        let (st, ct) = t.sin_cos();
        let (ex, ey) = (rx * ct, ry * st);
        (cx + cos_r * ex - sin_r * ey, cy + sin_r * ex + cos_r * ey)
    };
    // Derivative d/dt of the same, rotated but not translated.
    let deriv = |t: f64| {
        let (st, ct) = t.sin_cos();
        let (ex, ey) = (-rx * st, ry * ct);
        (cos_r * ex - sin_r * ey, sin_r * ex + cos_r * ey)
    };

    let full = if ccw {
        start_angle - end_angle >= TAU
    } else {
        end_angle - start_angle >= TAU
    };
    let sweep = if full {
        if ccw {
            -TAU
        } else {
            TAU
        }
    } else if ccw {
        // Normalise into (-2π, 0]: sweep towards decreasing angle.
        -((start_angle - end_angle).rem_euclid(TAU))
    } else {
        // Normalise into [0, 2π): sweep towards increasing angle.
        (end_angle - start_angle).rem_euclid(TAU)
    };

    let (sx, sy) = point(start_angle);
    let mut ops = vec![EllipseOp::MoveTo(sx, sy)];
    if sweep.abs() < 1e-12 {
        return ops;
    }

    let segments = (sweep.abs() / FRAC_PI_2).ceil().max(1.0) as usize;
    let step = sweep / segments as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    for i in 0..segments {
        let a = start_angle + step * i as f64;
        let b = a + step;
        let (p0x, p0y) = point(a);
        let (d0x, d0y) = deriv(a);
        let (p3x, p3y) = point(b);
        let (d3x, d3y) = deriv(b);
        ops.push(EllipseOp::CubicTo(
            p0x + k * d0x,
            p0y + k * d0y,
            p3x - k * d3x,
            p3y - k * d3y,
            p3x,
            p3y,
        ));
    }
    if full {
        ops.push(EllipseOp::ClosePath);
    }
    ops
}

// ---------------------------------------------------------------------------
// Pixel snapping
// ---------------------------------------------------------------------------

/// Fractional part of `t`'s translation, or `None` when it is already
/// pixel-aligned.  Callers translate by the negated pair to snap.
pub fn pixel_snap_offset(t: &TransAffine) -> Option<(f64, f64)> {
    let fx = t.tx - t.tx.floor();
    let fy = t.ty - t.ty.floor();
    if fx != 0.0 || fy != 0.0 {
        Some((fx, fy))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Corner-quad fallback
// ---------------------------------------------------------------------------

/// Axis-aligned bounding rect `(x, y, w, h)` of four quad corners — the
/// fallback destination for backends that cannot draw a distorted quad.
pub fn corners_bounding_rect(corners: [(f64, f64); 4]) -> (f64, f64, f64, f64) {
    let (min_x, min_y, max_x, max_y) = corners
        .iter()
        .fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |a, c| {
            (a.0.min(c.0), a.1.min(c.1), a.2.max(c.0), a.3.max(c.1))
        });
    (
        min_x,
        min_y,
        (max_x - min_x).max(0.0),
        (max_y - min_y).max(0.0),
    )
}

// ---------------------------------------------------------------------------
// LCD backbuffer collapse
// ---------------------------------------------------------------------------

/// Collapse a two-plane LCD-coverage backbuffer (premultiplied per-channel
/// colour + per-channel alpha, both 3 bytes per pixel, top-row-first) into
/// straight-alpha RGBA8, or `None` when either plane is too short.
///
/// Uses the same per-pixel rule as `LcdBuffer::to_rgba8_top_down_collapsed`
/// via [`crate::lcd_coverage::collapse_lcd_pixel`] — keeping the maths in one
/// place is deliberate: this site kept an independent `max` collapse after the
/// other was fixed, which is how the "LCD text is bolder" bug stayed alive on
/// the nested-backbuffer path.
pub fn collapse_lcd_planes(color: &[u8], alpha: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let w_u = w as usize;
    let h_u = h as usize;
    if color.len() < w_u * h_u * 3 || alpha.len() < w_u * h_u * 3 {
        return None;
    }
    let mut rgba = vec![0u8; w_u * h_u * 4];
    for i in 0..(w_u * h_u) {
        let ci = i * 3;
        let di = i * 4;
        let px = crate::lcd_coverage::collapse_lcd_pixel(
            [color[ci], color[ci + 1], color[ci + 2]],
            [alpha[ci], alpha[ci + 1], alpha[ci + 2]],
        );
        rgba[di..di + 4].copy_from_slice(&px);
    }
    Some(rgba)
}
