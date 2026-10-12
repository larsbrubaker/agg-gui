//! Rust-only tests of `aa_texture_mesh.rs`: they evaluate the mesh the way
//! the GPU does (the texcoord `U` interpolated across each triangle, read
//! through the 1024-wide alpha-step texture with LINEAR filtering) and check
//! coverage at chosen points.  The cross-backend goldens in
//! `agg-gui-wgpu/src/golden/golden_2d_primitive_tests.rs` check the same
//! shapes against the software renderer.

use super::aa_texture_mesh::{tessellate_path_aa_texture, AaTexVertex};
use crate::draw_ctx::FillRule;
use agg_rust::path_storage::PathStorage;

fn polygon(points: &[(f64, f64)]) -> PathStorage {
    let mut path = PathStorage::new();
    path.move_to(points[0].0, points[0].1);
    for p in &points[1..] {
        path.line_to(p.0, p.1);
    }
    path.close_polygon(0);
    path
}

/// The alpha-step texture under LINEAR filtering: texel 0 (alpha 0) has its
/// centre at `U = 0.5 / 1024`, texel 1 (opaque) at `1.5 / 1024`.
fn step_alpha(u: f32) -> f32 {
    (u * 1024.0 - 0.5).clamp(0.0, 1.0)
}

/// The alpha the mesh paints at `p`: source-over of every triangle that
/// covers it (one colour, so only the alphas combine).
fn coverage(mesh: &(Vec<AaTexVertex>, Vec<u32>), p: [f32; 2]) -> f32 {
    let (verts, indices) = mesh;
    let mut alpha = 0.0f32;
    for t in indices.chunks_exact(3) {
        let (a, b, c) = (
            verts[t[0] as usize],
            verts[t[1] as usize],
            verts[t[2] as usize],
        );
        let cross = |o: [f32; 2], u: [f32; 2], v: [f32; 2]| {
            (u[0] - o[0]) * (v[1] - o[1]) - (u[1] - o[1]) * (v[0] - o[0])
        };
        let area = cross(a.pos, b.pos, c.pos);
        if area.abs() < 1e-9 {
            continue;
        }
        let wa = cross(b.pos, c.pos, p) / area;
        let wb = cross(c.pos, a.pos, p) / area;
        let wc = cross(a.pos, b.pos, p) / area;
        if wa < 0.0 || wb < 0.0 || wc < 0.0 {
            continue;
        }
        let u = wa * a.uv[0] + wb * b.uv[0] + wc * c.uv[0];
        let s = step_alpha(u);
        alpha = s + alpha * (1.0 - s);
    }
    alpha
}

/// Distance from `p` to the segment `a`–`b`.
fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    let (qx, qy) = (a[0] + t * dx, a[1] + t * dy);
    ((p[0] - qx).powi(2) + (p[1] - qy).powi(2)).sqrt()
}

fn inside(p: [f32; 2], poly: &[[f32; 2]]) -> bool {
    let mut odd = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                odd = !odd;
            }
        }
    }
    odd
}

/// Every point inside `points` at least `margin` from its outline is opaque.
fn assert_opaque_inside(points: &[(f64, f64)], margin: f32) {
    let mesh =
        tessellate_path_aa_texture(&mut polygon(points), FillRule::NonZero).expect("tessellates");
    let poly: Vec<[f32; 2]> = points.iter().map(|p| [p.0 as f32, p.1 as f32]).collect();
    let (mut min, mut max) = ([f32::MAX; 2], [f32::MIN; 2]);
    for p in &poly {
        for k in 0..2 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    let mut y = min[1];
    while y <= max[1] {
        let mut x = min[0];
        while x <= max[0] {
            let p = [x, y];
            let edge = (0..poly.len())
                .map(|i| segment_distance(p, poly[i], poly[(i + 1) % poly.len()]))
                .fold(f32::MAX, f32::min);
            if inside(p, &poly) && edge >= margin {
                let a = coverage(&mesh, p);
                assert!(a > 0.999, "alpha {a} at {p:?}, {edge} inside the outline");
            }
            x += 0.25;
        }
        y += 0.25;
    }
}

/// tess2 fans the star's reflex vertices into triangles whose third vertex
/// lies past the end of their outline edge; the interior beyond that edge's
/// end must stay solid, not ramp by distance to the edge's line.
#[test]
fn rust_only_reflex_vertices_leave_the_interior_solid() {
    let star: Vec<(f64, f64)> = (0..10)
        .map(|point| {
            let angle = point as f64 * std::f64::consts::PI * 2.0 / 10.0;
            let radius = if point % 2 == 0 { 80.0 } else { 32.0 };
            (110.0 + angle.cos() * radius, 270.0 + angle.sin() * radius)
        })
        .collect();
    assert_opaque_inside(&star, 0.6);
}

/// An arrow's barbs are acute corners whose triangles have two outline
/// edges; a point just inside the corner tip is only partly covered, and
/// the interior away from the edges is still solid.
#[test]
fn rust_only_acute_corners_ramp_on_the_inside() {
    let arrow = [
        (0.0, -18.0),
        (70.0, -18.0),
        (70.0, -34.0),
        (104.0, 0.0),
        (70.0, 34.0),
        (70.0, 18.0),
        (0.0, 18.0),
    ];
    let mesh =
        tessellate_path_aa_texture(&mut polygon(&arrow), FillRule::NonZero).expect("tessellates");
    // A tenth of a pixel inside both edges at the barb (70, 34).
    let tip = coverage(&mesh, [70.1, 33.6]);
    assert!(tip < 0.9, "barb tip alpha {tip}");
    assert_opaque_inside(&arrow, 0.6);
}
