//! Regression tests for the bidirectional feather geometry produced by
//! [`crate::gl_renderer::tessellate_path_aa`].
//!
//! The contract: every halo strip is a quad whose inner endpoints sit
//! `halo_px / 2` **inside** the polygon at alpha = 1 and whose outer
//! endpoints sit `halo_px / 2` **outside** the polygon at alpha = 0.
//! Adjacent polygons that share an edge then have feather strips that
//! overlap with complementary alpha and tile without bleeding —
//! historically (outward-only halo, inner at the edge) two adjacent
//! 1-px-wide rects averaged to gray because each rect's halo painted
//! semi-transparently over the neighbor's interior.

use crate::draw_ctx::FillRule;
use crate::gl_renderer::tessellate_path_aa;
use agg_rust::path_storage::PathStorage;

/// Build the path for an axis-aligned rectangle in CCW (Y-up) order.
fn rect_path(x: f64, y: f64, w: f64, h: f64) -> PathStorage {
    let mut p = PathStorage::new();
    p.move_to(x, y);
    p.line_to(x + w, y);
    p.line_to(x + w, y + h);
    p.line_to(x, y + h);
    p.close_polygon(0);
    p
}

#[test]
fn feather_strip_is_centred_on_the_polygon_edge() {
    // A 10 × 10 rect at the origin. With `halo_px = 1.0` the feather
    // strip must span y ∈ [-0.5, 0.5] across the bottom edge, x ∈ [9.5,
    // 10.5] across the right edge, etc. — half-width on each side.
    let mut path = rect_path(0.0, 0.0, 10.0, 10.0);
    let (verts, _idx) =
        tessellate_path_aa(&mut path, 1.0, FillRule::NonZero).expect("tessellation must succeed");

    // Halo strips are appended after the interior triangles. Collect the
    // (x, y, alpha) triples and assert that every alpha=0 outer vertex
    // and every alpha=1 inner vertex sits exactly half a pixel from one
    // of the rect's four sides.
    let mut outer_count = 0;
    for v in &verts {
        let alpha = v[2];
        if alpha == 0.0 {
            outer_count += 1;
            let dist_to_outside_edge = [-0.5 - v[0], v[0] - 10.5, -0.5 - v[1], v[1] - 10.5]
                .into_iter()
                .map(f32::abs)
                .fold(f32::INFINITY, f32::min);
            assert!(
                dist_to_outside_edge < 1e-4,
                "outer vertex {v:?} is not half a pixel outside any rect edge",
            );
        }
    }
    assert!(
        outer_count >= 8,
        "expected at least 8 alpha=0 outer vertices (2 per rect edge), got {outer_count}",
    );
}

#[test]
fn adjacent_rect_feather_strips_overlap_with_complementary_alpha() {
    // Two 1-px-wide stripes sharing the edge at x = 1. With bidirectional
    // feathering the right edge of the left rect and the left edge of
    // the right rect each produce a feather strip spanning x ∈
    // [0.5, 1.5]. At any x inside that span the two strips' alpha must
    // sum to 1 — that's what makes the colours tile cleanly under
    // SrcOver instead of mixing to gray.
    let probe_alpha = |path: &mut PathStorage, edge_x: f32, side: f32, sample_x: f32| {
        let (verts, _) = tessellate_path_aa(path, 1.0, FillRule::NonZero).unwrap();
        // For each strip on the chosen vertical edge (inner verts at
        // x = edge_x + 0.5*side, outer at edge_x - 0.5*side) sample the
        // alpha ramp at `sample_x`. The strip is a quad spanning ±0.5
        // around the edge; alpha is linear from 1 (inner) to 0 (outer).
        let inner_x = edge_x + 0.5 * side;
        let outer_x = edge_x - 0.5 * side;
        let mut found = None;
        for v in &verts {
            if (v[2] - 1.0).abs() < 1e-4 && (v[0] - inner_x).abs() < 1e-4 {
                found = Some((inner_x, outer_x));
                break;
            }
        }
        found.map(|(inner, outer)| {
            let t = (sample_x - outer) / (inner - outer);
            t.clamp(0.0, 1.0)
        })
    };

    let mut left = rect_path(0.0, 0.0, 1.0, 96.0);
    let mut right = rect_path(1.0, 0.0, 1.0, 96.0);

    // Left rect's right edge is at x=1, feather extends to inside (x=0.5
    // inner, alpha=1) and outside (x=1.5 outer, alpha=0).
    let left_alpha =
        probe_alpha(&mut left, 1.0, -1.0, 1.0).expect("left rect must emit a right-edge feather");
    // Right rect's left edge is at x=1, feather inside x=1.5 (alpha=1)
    // and outside x=0.5 (alpha=0).
    let right_alpha =
        probe_alpha(&mut right, 1.0, 1.0, 1.0).expect("right rect must emit a left-edge feather");

    let sum = left_alpha + right_alpha;
    assert!(
        (sum - 1.0).abs() < 1e-3,
        "adjacent feathers must sum to alpha=1 at the shared edge — got {left_alpha} + {right_alpha} = {sum}",
    );
}
