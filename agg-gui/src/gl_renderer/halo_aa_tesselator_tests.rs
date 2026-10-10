//! agg-sharp `Tests/Agg.Tests/Agg.RenderGl/HaloAaTesselatorTests.cs`, ported
//! 1:1 onto the code it was itself ported from: `HaloAaTesselator` is
//! [`tessellate_interior_with_rule`] (its `CachedTesselator` half —
//! `VerticesCache` / `IndicesCache`) plus [`expand_aa_halo`] (`BuildHaloMesh`,
//! `HaloVertices`), at the default [`AA_HALO_WIDTH`].

use super::tess2_bridge::{
    expand_aa_halo, tessellate_interior_with_rule, CachedTess, AA_HALO_WIDTH,
};
use crate::draw_ctx::FillRule;
use agg_rust::basics::PATH_CMD_END_POLY;
use agg_rust::path_storage::PathStorage;

/// The C# tesselator after `BuildHaloMesh`: the interior tessellation and the
/// halo mesh built from it.
struct HaloMesh {
    interior: CachedTess,
    halo_vertices: Vec<[f32; 3]>,
}

fn tessellate(path: &mut PathStorage, fill_rule: FillRule) -> HaloMesh {
    let interior = tessellate_interior_with_rule(path, fill_rule).expect("tessellates");
    let (halo_vertices, _) =
        expand_aa_halo(&interior.vertices, &interior, AA_HALO_WIDTH).expect("halo mesh");
    HaloMesh {
        interior,
        halo_vertices,
    }
}

fn in_triangle(p: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    let cross = |o: [f64; 2], u: [f64; 2], v: [f64; 2]| {
        (u[0] - o[0]) * (v[1] - o[1]) - (u[1] - o[1]) * (v[0] - o[0])
    };
    let (d0, d1, d2) = (cross(a, b, p), cross(b, c, p), cross(c, a, p));
    (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
}

/// Whether a point lies in an interior (fully opaque) triangle - the ones libtess made.
fn covered_by_interior(mesh: &HaloMesh, point: [f64; 2]) -> bool {
    let v = &mesh.interior.vertices;
    let at = |i: u32| [v[i as usize * 2] as f64, v[i as usize * 2 + 1] as f64];
    mesh.interior
        .indices
        .chunks_exact(3)
        .any(|t| in_triangle(point, at(t[0]), at(t[1]), at(t[2])))
}

#[test]
fn interior_is_opaque_and_every_outline_edge_fades_half_a_pixel_outward() {
    // Clockwise on purpose: the halo direction must not depend on winding.
    let mut square = PathStorage::new();
    square.move_to(10.0, 10.0);
    square.line_to(10.0, 20.0);
    square.line_to(20.0, 20.0);
    square.line_to(20.0, 10.0);
    square.close_polygon(0);

    let mesh = tessellate(&mut square, FillRule::NonZero);

    // Only outline edges get a halo: four quads, whatever diagonal libtess chose.
    let interior_vertices = mesh.interior.vertices.len() / 2;
    assert_eq!(mesh.halo_vertices.len() - interior_vertices, 16);
    assert!(mesh.halo_vertices[..interior_vertices]
        .iter()
        .all(|v| v[2] == 1.0));

    for outer in mesh.halo_vertices[interior_vertices..]
        .iter()
        .filter(|v| v[2] == 0.0)
    {
        // Half a pixel outside the square, never inside it.
        let (x, y) = (outer[0] as f64, outer[1] as f64);
        let outside = (10.0 - x).max(x - 20.0).max((10.0 - y).max(y - 20.0));
        assert!(
            (outside - 0.5).abs() <= 1e-12,
            "outer vertex {outer:?} is {outside} outside"
        );
    }

    // The ramp falls one coverage level per pixel, so the edge itself is half covered.
    assert_eq!(
        mesh.halo_vertices[interior_vertices..]
            .iter()
            .filter(|v| v[2] == 0.5)
            .count(),
        8
    );
}

/// An end_poly without the close flag (what SmoothPolygon ends an open path with) ends the contour and adds
/// no point; its position is (0, 0), which as a vertex drew a wedge out to the origin (conv_dash_marker).
#[test]
fn open_end_poly_adds_no_vertex() {
    let mut triangle = PathStorage::new();
    triangle.move_to(40.0, 40.0);
    triangle.line_to(80.0, 40.0);
    triangle.line_to(60.0, 80.0);
    triangle.add_vertex(0.0, 0.0, PATH_CMD_END_POLY);

    let mesh = tessellate(&mut triangle, FillRule::NonZero);

    assert!(!covered_by_interior(&mesh, [20.0, 20.0]));
    assert!(covered_by_interior(&mesh, [60.0, 50.0]));
}

#[test]
fn even_odd_leaves_the_pentagram_center_open() {
    let mut star = PathStorage::new();
    for i in 0..5 {
        let angle = std::f64::consts::PI / 2.0 + i as f64 * 4.0 * std::f64::consts::PI / 5.0;
        let (x, y) = (50.0 + 40.0 * angle.cos(), 50.0 + 40.0 * angle.sin());
        if i == 0 {
            star.move_to(x, y);
        } else {
            star.line_to(x, y);
        }
    }

    star.close_polygon(0);

    let center = [50.0, 50.0];
    let arm = [50.0, 85.0];
    let non_zero = tessellate(&mut star, FillRule::NonZero);
    let even_odd = tessellate(&mut star, FillRule::EvenOdd);

    assert!(covered_by_interior(&non_zero, center));
    assert!(!covered_by_interior(&even_odd, center));
    assert!(covered_by_interior(&even_odd, arm));
}
