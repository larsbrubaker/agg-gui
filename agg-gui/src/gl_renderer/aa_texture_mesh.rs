//! Texture-based AA mesh — direct port of MatterCAD agg-sharp's
//! `AARenderTesselator.cs`.
//!
//! # Why this exists
//!
//! The older [`super::tessellate_path_aa`] in this crate generates a
//! 1-pixel-wide halo strip with per-vertex alpha 1 → 0 *across the
//! strip*.  For ADJACENT polygons that share an edge (the pixel
//! alignment test, or any tiled rect) one polygon's outward halo
//! paints α≈0.5 into the neighbour's interior, mixing white + black
//! to gray under SrcOver — the long-standing pixel-test bug.
//!
//! AGG-sharp's `AARenderTesselator` solves this by routing the AA
//! through a **1024-wide alpha-step texture** (col 0 α=0, cols 1+
//! α=opaque) sampled with LINEAR filtering.  Each AA edge produces a
//! triangle fan whose texture coordinates are arranged so that:
//!
//! - The polygon edge vertices map to `U = 1/1023` — exactly between
//!   texel 0 and texel 1's centres, so the LINEAR filter returns
//!   α ≈ 0.5 right ON the polygon edge.
//! - The 1-unit-outward extruded vertices map to `U = 0` — at texel
//!   0 (α = 0).
//! - The non-AA interior vertex maps to `U = (1 + edgeDotP3) / 1023`
//!   — well past texel 1, so the interior renders at α = opaque.
//!
//! With this geometry the alpha transition lives *entirely within one
//! texel*, which in screen space maps to about half a pixel of fade
//! centred on the polygon edge.  No outward strip painting alpha
//! beyond the polygon's true extent, no bleed into neighbours.
//!
//! # Driver: per-triangle edge-flag count (RenderLastToGL)
//!
//! `tess2` emits triangles with per-vertex edge flags telling us
//! whether the edge starting at that vertex is on the original
//! polygon boundary.  Sum across the three vertices to get the
//! triangle's AA-edge count:
//!
//! - 0 → [`draw_non_aa_triangle`] (interior triangle, sampled at high
//!   U so α = opaque everywhere)
//! - 1 → [`draw_1_edge_triangle`] with the right vertex permutation
//! - 2 → [`draw_2_edge_triangle`] — two AA edges + non-AA filler
//! - 3 → [`draw_3_edge_triangle`] — three AA edges meeting at the
//!   centroid

use crate::draw_ctx::FillRule;
use crate::gl_renderer::tess2_bridge::{
    agg_path_to_contours, to_tess_winding_rule, try_tessellate,
};
use agg_rust::basics::VertexSource;

/// One vertex of an AA-texture mesh: position in path/screen space + a
/// 2-D texcoord into the 1024-wide alpha-step texture.
///
/// `#[repr(C)]` with no padding — four `f32`s = 16 bytes per vertex —
/// so callers can `bytemuck::cast_slice` straight from this slice.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AaTexVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
}

/// Tessellate a path into a mesh ready for the AGG-sharp-style
/// texture-AA pipeline.
///
/// Mirrors `AARenderTesselator::RenderLastToGL`: per-triangle the
/// boundary-edge count from tess2 selects the appropriate emit
/// helper.  No CPU-side halo strip; no per-vertex alpha attribute.
pub fn tessellate_path_aa_texture<VS: VertexSource>(
    path: &mut VS,
    fill_rule: FillRule,
) -> Option<(Vec<AaTexVertex>, Vec<u32>)> {
    let contours = agg_path_to_contours(path);
    if contours.is_empty() {
        return None;
    }

    struct TessOut {
        verts: Vec<f32>,
        indices: Vec<u32>,
        flags: Vec<u8>,
        vcount: usize,
    }
    let out = try_tessellate(
        &contours,
        to_tess_winding_rule(fill_rule),
        "tessellate_path_aa_texture",
        |tess| {
            Some(TessOut {
                verts: tess.vertices().iter().map(|&v| v as f32).collect(),
                indices: tess.elements().to_vec(),
                flags: tess.edge_flags().to_vec(),
                vcount: tess.vertex_count(),
            })
        },
    )?;

    let in_verts = &out.verts;
    let in_indices = &out.indices;
    let edge_flags = &out.flags;

    let n_interior = out.vcount;
    let n_indices = in_indices.len();
    if n_indices == 0 {
        return None;
    }

    let mut out_verts: Vec<AaTexVertex> = Vec::with_capacity(n_indices * 5);
    let mut out_indices: Vec<u32> = Vec::with_capacity(n_indices * 3);

    let n_tris = n_indices / 3;
    for t in 0..n_tris {
        let ia = in_indices[t * 3] as usize;
        let ib = in_indices[t * 3 + 1] as usize;
        let ic = in_indices[t * 3 + 2] as usize;
        if ia >= n_interior || ib >= n_interior || ic >= n_interior {
            continue;
        }
        let v0 = [in_verts[ia * 2], in_verts[ia * 2 + 1]];
        let v1 = [in_verts[ib * 2], in_verts[ib * 2 + 1]];
        let v2 = [in_verts[ic * 2], in_verts[ic * 2 + 1]];

        let e0 = edge_flags.get(t * 3).copied().unwrap_or(0);
        let e1 = edge_flags.get(t * 3 + 1).copied().unwrap_or(0);
        let e2 = edge_flags.get(t * 3 + 2).copied().unwrap_or(0);

        match e0 + e1 + e2 {
            0 => draw_non_aa_triangle(&mut out_verts, &mut out_indices, v0, v1, v2),
            1 => {
                if e0 == 1 {
                    draw_1_edge_triangle(&mut out_verts, &mut out_indices, v0, v1, v2);
                } else if e1 == 1 {
                    draw_1_edge_triangle(&mut out_verts, &mut out_indices, v1, v2, v0);
                } else {
                    draw_1_edge_triangle(&mut out_verts, &mut out_indices, v2, v0, v1);
                }
            }
            2 => {
                if e0 == 1 {
                    if e1 == 1 {
                        draw_2_edge_triangle(&mut out_verts, &mut out_indices, v0, v1, v2);
                    } else {
                        draw_2_edge_triangle(&mut out_verts, &mut out_indices, v2, v0, v1);
                    }
                } else {
                    draw_2_edge_triangle(&mut out_verts, &mut out_indices, v1, v2, v0);
                }
            }
            3 => draw_3_edge_triangle(&mut out_verts, &mut out_indices, v0, v1, v2),
            _ => {}
        }
    }

    Some((out_verts, out_indices))
}

/// Emit an interior triangle whose three vertices all sample at high
/// U (≫ 1/1023) — the alpha-step texture returns α = opaque there, so
/// the triangle renders at full polygon color with no edge AA.
///
/// Mirrors `AARenderTesselator::DrawNonAATriangle` — same exact texcoords.
fn draw_non_aa_triangle(
    verts: &mut Vec<AaTexVertex>,
    indices: &mut Vec<u32>,
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
) {
    let base = verts.len() as u32;
    verts.push(AaTexVertex {
        pos: p0,
        uv: [0.2, 0.25],
    });
    verts.push(AaTexVertex {
        pos: p1,
        uv: [0.2, 0.75],
    });
    verts.push(AaTexVertex {
        pos: p2,
        uv: [0.9, 0.5],
    });
    indices.extend_from_slice(&[base, base + 1, base + 2]);
}

/// Emit a triangle fan for ONE anti-aliased edge.  Direct port of
/// `AARenderTesselator::Draw1EdgeTriangle`.
///
/// `aa_p0 → aa_p1` is the AA edge (a polygon boundary); `non_aa_point`
/// is the triangle's third vertex (an interior point).  We extrude
/// the AA edge 1 unit OUTWARD (away from `non_aa_point`) and emit a
/// fan anchored at `aa_p0` with three triangles:
///
/// 1. `(aa_p0, p0_offset, p1_offset)` — first half of the extruded strip
/// 2. `(aa_p0, p1_offset, aa_p1)` — second half of the extruded strip
/// 3. `(aa_p0, aa_p1, non_aa_point)` — the original triangle interior
///
/// Texcoords are arranged so the alpha-step texture's α = 0 column lands
/// at the extruded edge and the α = opaque columns cover the interior.
fn draw_1_edge_triangle(
    verts: &mut Vec<AaTexVertex>,
    indices: &mut Vec<u32>,
    aa_p0: [f32; 2],
    aa_p1: [f32; 2],
    non_aa_point: [f32; 2],
) {
    if aa_p0 == aa_p1 || aa_p1 == non_aa_point || non_aa_point == aa_p0 {
        return;
    }

    let edge = [aa_p1[0] - aa_p0[0], aa_p1[1] - aa_p0[1]];
    let len = (edge[0] * edge[0] + edge[1] * edge[1]).sqrt();
    if len < 1e-6 {
        return;
    }
    let edge_n = [edge[0] / len, edge[1] / len];

    // PerpendicularRight in agg-sharp Vector2 convention: (x, y) → (y, -x).
    let mut normal = [edge_n[1], -edge_n[0]];
    let dot_n_third = normal[0] * (non_aa_point[0] - aa_p0[0])
        + normal[1] * (non_aa_point[1] - aa_p0[1]);
    let edge_dot_p3 = if dot_n_third < 0.0 {
        -dot_n_third
    } else {
        // Flip the normal so it points AWAY from non_aa_point — same
        // sign-fix as AGG-sharp's `Draw1EdgeTriangle`.
        normal = [-normal[0], -normal[1]];
        dot_n_third
    };

    // 1-unit outward offset.  `tessellate_path_aa_texture` is called with
    // the path already CTM-transformed on the CPU side (same convention
    // as the legacy halo path), so one path unit = one screen pixel for
    // identity scale.
    let p0_offset = [aa_p0[0] + normal[0], aa_p0[1] + normal[1]];
    let p1_offset = [aa_p1[0] + normal[0], aa_p1[1] + normal[1]];

    // Same texcoord constants as agg-sharp.  `1/1023` puts the polygon
    // edge between texel 0 (α=0) and texel 1 (α=opaque) of the
    // 1024-wide step texture — under LINEAR filtering that produces
    // the analytic α≈0.5 transition right at the geometric edge.
    let inv_1023 = 1.0 / 1023.0_f32;
    let tex_p0 = [inv_1023, 0.25];
    let tex_p1 = [inv_1023, 0.75];
    let tex_p2 = [(1.0 + edge_dot_p3) * inv_1023, 0.25];
    let tex_p0_off = [0.0, 0.25];
    let tex_p1_off = [0.0, 0.75];

    let base = verts.len() as u32;
    verts.push(AaTexVertex {
        pos: aa_p0,
        uv: tex_p0,
    });
    verts.push(AaTexVertex {
        pos: p0_offset,
        uv: tex_p0_off,
    });
    verts.push(AaTexVertex {
        pos: p1_offset,
        uv: tex_p1_off,
    });
    verts.push(AaTexVertex {
        pos: aa_p1,
        uv: tex_p1,
    });
    verts.push(AaTexVertex {
        pos: non_aa_point,
        uv: tex_p2,
    });
    indices.extend_from_slice(&[
        base,
        base + 1,
        base + 2,
        base,
        base + 2,
        base + 3,
        base,
        base + 3,
        base + 4,
    ]);
}

/// Two adjacent edges are AA: `p0→p1` and `p1→p2`.  `p2→p0` is interior.
///
/// Strategy is a direct port of agg-sharp's `Draw2EdgeTriangle`: emit a
/// Draw1Edge fan for each AA edge with a non-AA point placed just
/// inward of that edge's midpoint (so the fan's "interior triangle"
/// is essentially degenerate — covers a sliver near the edge only).
/// Then a non-AA triangle covers the full polygon interior.
fn draw_2_edge_triangle(
    verts: &mut Vec<AaTexVertex>,
    indices: &mut Vec<u32>,
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
) {
    let centroid = [
        (p0[0] + p1[0] + p2[0]) / 3.0,
        (p0[1] + p1[1] + p2[1]) / 3.0,
    ];
    let mid_p0p1 = [(p0[0] + p1[0]) * 0.5, (p0[1] + p1[1]) * 0.5];
    let mid_p1p2 = [(p1[0] + p2[0]) * 0.5, (p1[1] + p2[1]) * 0.5];
    // Same `.001` fudge agg-sharp uses so the inner point isn't
    // co-linear with the AA edge (which would zero out the fan).
    let inner_p0p1 = [
        mid_p0p1[0] + (centroid[0] - mid_p0p1[0]) * 0.001,
        mid_p0p1[1] + (centroid[1] - mid_p0p1[1]) * 0.001,
    ];
    let inner_p1p2 = [
        mid_p1p2[0] + (centroid[0] - mid_p1p2[0]) * 0.001,
        mid_p1p2[1] + (centroid[1] - mid_p1p2[1]) * 0.001,
    ];
    draw_1_edge_triangle(verts, indices, p0, p1, inner_p0p1);
    draw_1_edge_triangle(verts, indices, p1, p2, inner_p1p2);
    draw_non_aa_triangle(verts, indices, p0, p1, p2);
}

/// All three edges are AA — port of `Draw3EdgeTriangle`.  Each edge
/// gets its own Draw1Edge fan with the centroid as the non-AA point;
/// together the three fans tile the triangle interior exactly.
fn draw_3_edge_triangle(
    verts: &mut Vec<AaTexVertex>,
    indices: &mut Vec<u32>,
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
) {
    let centroid = [
        (p0[0] + p1[0] + p2[0]) / 3.0,
        (p0[1] + p1[1] + p2[1]) / 3.0,
    ];
    draw_1_edge_triangle(verts, indices, p0, p1, centroid);
    draw_1_edge_triangle(verts, indices, p1, p2, centroid);
    draw_1_edge_triangle(verts, indices, p2, p0, centroid);
}
