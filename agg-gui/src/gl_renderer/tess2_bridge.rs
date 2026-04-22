//! tess2 bridge — converts 2D polygon contours into GL triangle meshes.
//!
//! # Architecture (based on MatterCAD agg-sharp reference)
//!
//! Both fills and AA strokes follow the same pipeline:
//!
//! ```text
//! Contours (Vec<Vec<[f32;2]>>)
//!   │  (stroke: pre-expand via AGG Stroke first)
//!   ▼
//! tess2::Tessellator::add_contour()   ← one call per contour ring
//!   ▼
//! tess2::tessellate(NonZero, Polygons, 3, 2)
//!   ▼
//! out_vertices: flat [x,y, x,y, …]
//! out_elements: flat [i0,i1,i2, i0,i1,i2, …]   ← triangle indices
//!   ▼
//! (Vec<f32>, Vec<u32>)   ready for glBufferData
//! ```
//!
//! Anti-aliased edge expansion (for strokes) follows the approach in
//! `AARenderTesselator.cs`: each boundary edge is expanded by 1 px outward and
//! a coverage ramp (0 → alpha) is applied across the expansion quad. For now
//! we implement non-AA fill tessellation; AA strokes are planned as Phase D ext.

use tess2_rust::{ElementType, Tessellator, WindingRule};
use agg_rust::basics::{is_end_poly, is_move_to, is_stop, VertexSource};

// ---------------------------------------------------------------------------
// Universal AGG VertexSource → tess2 contours
// ---------------------------------------------------------------------------

/// Walk any AGG `VertexSource` (a `PathStorage`, a `ConvStroke<…>`, a
/// `ConvCurve<…>`, an `Ellipse`, etc.) and produce tess2-ready contours.
///
/// Protocol (mirrors MatterCAD's `VertexSourceToTesselator.SendShapeToTesselator`):
///   - `move_to` → start a new contour.
///   - `line_to` → append vertex to the current contour.
///   - `end_poly` / implicit close → finalise the current contour.
///   - `stop` → we're done.
///
/// Degenerate contours (< 3 vertices after de-duplication) are dropped, since
/// tess2 can't triangulate them and frequently panics on zero-length edges.
///
/// This is the ONE place in the codebase that does path → contour conversion.
/// Both fill and stroke rendering go through it so the two code paths share
/// the same battle-tested handling of close/end-poly semantics.
pub fn agg_path_to_contours<VS: VertexSource>(path: &mut VS) -> Vec<Vec<[f32; 2]>> {
    let mut out: Vec<Vec<[f32; 2]>> = Vec::new();
    let mut cur: Vec<[f32; 2]> = Vec::new();

    path.rewind(0);
    loop {
        let (mut x, mut y) = (0.0, 0.0);
        let cmd = path.vertex(&mut x, &mut y);
        if is_stop(cmd) {
            break;
        }
        if is_move_to(cmd) {
            // Finish the previous contour before starting a new one.
            if !cur.is_empty() {
                push_contour(&mut out, std::mem::take(&mut cur));
            }
            cur.push([x as f32, y as f32]);
        } else if is_end_poly(cmd) {
            if !cur.is_empty() {
                push_contour(&mut out, std::mem::take(&mut cur));
            }
        } else {
            // line_to (drawing command): append to current contour.
            cur.push([x as f32, y as f32]);
        }
    }
    if !cur.is_empty() {
        push_contour(&mut out, cur);
    }
    out
}

fn push_contour(out: &mut Vec<Vec<[f32; 2]>>, mut contour: Vec<[f32; 2]>) {
    // De-duplicate consecutive identical vertices and strip a trailing
    // closing duplicate (first == last) — tess2 panics on zero-length
    // edges and on redundant closing vertices.
    contour = deduplicate_contour_v(&contour);
    if contour.len() < 3 { return; }
    if signed_area_2x(&contour).abs() < 1.0 { return; }
    out.push(contour);
}

/// Tessellate any AGG vertex source.  Convenience wrapper over
/// [`agg_path_to_contours`] + [`tessellate_fill`] — use this for every fill /
/// stroke rendering path so there's a single code-path from an AGG path to
/// GPU triangles.
pub fn tessellate_path<VS: VertexSource>(path: &mut VS) -> Option<(Vec<f32>, Vec<u32>)> {
    let contours = agg_path_to_contours(path);
    if contours.is_empty() { return None; }
    tessellate_fill(&contours)
}

// ---------------------------------------------------------------------------
// Analytic edge AA via tess2 edge flags
// ---------------------------------------------------------------------------

/// Tessellate `path` and produce **`(x, y, alpha)` vertices + triangle
/// indices** for analytic edge-AA rendering.
///
/// For every triangle-vertex tess2 emits, the matching
/// [`edge_flags`](tess2_rust::Tessellator::edge_flags) entry tells us
/// whether the edge starting at that vertex (going CCW around the same
/// triangle) is an **original polygon boundary edge**.  For each such
/// boundary edge we emit a 1-pixel-wide halo quad extending OUTWARD:
///   - inner vertices (on the polygon edge): alpha = 1.0
///   - outer vertices (one pixel outside):   alpha = 0.0
/// The GPU linearly interpolates alpha across the halo, giving a clean
/// analytic edge-coverage ramp at the silhouette — no hardware MSAA needed.
///
/// This mirrors the MatterCAD agg-sharp `AARenderTesselator` strategy, with
/// a shader-side alpha attribute instead of a texture-coord trick.
///
/// `halo_px` is the halo strip width in logical pixels; `1.0` is the
/// convention (gives one pixel of alpha falloff outward from the boundary).
/// Cached result of tessellating an AGG path **once** at load time.
///
/// Callers who want to rotate / scale / skew the same geometry across
/// many frames should use [`tessellate_interior`] to build one of these,
/// stash it, and call [`expand_aa_halo`] every frame with the transformed
/// vertex positions.  That keeps the triangle set and its edge flags
/// deterministic — the same polygons render the same way regardless of
/// the current transform, which matters because tess2 is not
/// numerically stable across all transforms (small precision changes in
/// input coords can drop or re-order whole polygons in its output).
///
/// The battle-tested MatterCAD agg-sharp `CachedTesselator` pattern: tess
/// once, apply the view transform afterwards.
#[derive(Clone, Debug)]
pub struct CachedTess {
    /// Interior triangle vertices in the caller's input coord system
    /// (flat `[x, y, x, y, …]`).
    pub vertices: Vec<f32>,
    /// Triangle vertex indices into `vertices` (each triple is one triangle).
    pub indices:  Vec<u32>,
    /// Parallel to `indices`: `1` if the edge starting at this vertex
    /// (going CCW around the triangle) is an **original polygon boundary
    /// edge**, `0` if it's an interior edge added by the tessellation.
    /// Used by [`expand_aa_halo`] to build 1-pixel halo strips only along
    /// real silhouette edges.
    pub edge_flags: Vec<u8>,
}

/// Tessellate any AGG `VertexSource` ONCE into a cacheable triangle list
/// with edge flags.  Does not build halo strips — see [`expand_aa_halo`].
///
/// Returns `None` if the path yielded no usable triangles (empty path or
/// a degenerate shape tess2 can't handle).
pub fn tessellate_interior<VS: VertexSource>(path: &mut VS) -> Option<CachedTess> {
    let contours = agg_path_to_contours(path);
    if contours.is_empty() { return None; }

    let mut tess = Tessellator::new();
    for c in &contours {
        // tess2 accepts f64 coordinates — promote our f32 contours at the
        // boundary so the sweep's edge-sign predicates have the full f64
        // margin to absorb floating-point noise.
        let flat: Vec<f64> = c.iter().flat_map(|v| [v[0] as f64, v[1] as f64]).collect();
        tess.add_contour(2, &flat);
    }
    let ok = tess.tessellate(WindingRule::Odd, ElementType::Polygons, 3, 2, None);
    if !ok || tess.vertex_count() == 0 { return None; }
    Some(CachedTess {
        vertices:   tess.vertices().iter().map(|&v| v as f32).collect(),
        indices:    tess.elements().to_vec(),
        edge_flags: tess.edge_flags().to_vec(),
    })
}

/// Given a pre-computed `CachedTess` whose vertices have already been
/// transformed into screen space, emit `(x, y, alpha)` vertices + triangle
/// indices for the halo-AA solid pipeline.  Interior triangles get
/// alpha = 1.0 on every vertex; every boundary edge additionally spawns a
/// `halo_px`-wide outward quad with the outer pair at alpha = 0.0, giving
/// analytic 1-pixel edge coverage.
///
/// The outward normal is taken in screen space (`(dy, -dx)` of each edge
/// direction, Y-up CCW → right = outside), so the halo is always exactly
/// `halo_px` logical pixels wide regardless of the transform applied to
/// the interior vertices.
pub fn expand_aa_halo(
    transformed_xy: &[f32],
    cached: &CachedTess,
    halo_px: f32,
) -> Option<(Vec<[f32; 3]>, Vec<u32>)> {
    let n_interior = transformed_xy.len() / 2;
    let n_indices  = cached.indices.len();
    if n_indices == 0 { return None; }

    let mut out_verts: Vec<[f32; 3]> = Vec::with_capacity(n_interior + n_indices * 4);
    let mut out_indices: Vec<u32>    = Vec::with_capacity(n_indices + n_indices * 2);

    for i in 0..n_interior {
        out_verts.push([transformed_xy[i * 2], transformed_xy[i * 2 + 1], 1.0]);
    }
    out_indices.extend_from_slice(&cached.indices);

    let n_tris = n_indices / 3;
    for t in 0..n_tris {
        let ia = cached.indices[t * 3    ] as usize;
        let ib = cached.indices[t * 3 + 1] as usize;
        let ic = cached.indices[t * 3 + 2] as usize;
        if ia >= n_interior || ib >= n_interior || ic >= n_interior { continue; }
        let p = [
            [transformed_xy[ia * 2], transformed_xy[ia * 2 + 1]],
            [transformed_xy[ib * 2], transformed_xy[ib * 2 + 1]],
            [transformed_xy[ic * 2], transformed_xy[ic * 2 + 1]],
        ];
        let flag = [
            cached.edge_flags.get(t * 3    ).copied().unwrap_or(0),
            cached.edge_flags.get(t * 3 + 1).copied().unwrap_or(0),
            cached.edge_flags.get(t * 3 + 2).copied().unwrap_or(0),
        ];
        for k in 0..3 {
            if flag[k] == 0 { continue; }
            let a = p[k];
            let b = p[(k + 1) % 3];
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-6 { continue; }
            let nx =  dy / len * halo_px;
            let ny = -dx / len * halo_px;
            let base = out_verts.len() as u32;
            out_verts.push([a[0],      a[1],      1.0]);
            out_verts.push([b[0],      b[1],      1.0]);
            out_verts.push([a[0] + nx, a[1] + ny, 0.0]);
            out_verts.push([b[0] + nx, b[1] + ny, 0.0]);
            out_indices.extend_from_slice(&[
                base, base + 1, base + 2,
                base + 1, base + 3, base + 2,
            ]);
        }
    }

    Some((out_verts, out_indices))
}

pub fn tessellate_path_aa<VS: VertexSource>(
    path: &mut VS,
    halo_px: f32,
) -> Option<(Vec<[f32; 3]>, Vec<u32>)> {
    let contours = agg_path_to_contours(path);
    if contours.is_empty() { return None; }

    struct TessOut {
        verts:   Vec<f32>,
        indices: Vec<u32>,
        flags:   Vec<u8>,
        vcount:  usize,
    }
    let out = {
        let mut tess = Tessellator::new();
        for c in &contours {
            let flat: Vec<f64> = c.iter().flat_map(|v| [v[0] as f64, v[1] as f64]).collect();
            tess.add_contour(2, &flat);
        }
        let ok = tess.tessellate(WindingRule::Odd, ElementType::Polygons, 3, 2, None);
        if !ok || tess.vertex_count() == 0 { return None; }
        TessOut {
            verts:   tess.vertices().iter().map(|&v| v as f32).collect(),
            indices: tess.elements().to_vec(),
            flags:   tess.edge_flags().to_vec(),
            vcount:  tess.vertex_count(),
        }
    };

    let in_verts:   &[f32] = &out.verts;   // flat [x, y, x, y, …]
    let in_indices: &[u32] = &out.indices; // [i0, i1, i2, …]
    let edge_flags: &[u8]  = &out.flags;   // parallel to in_indices

    let n_interior = out.vcount;
    let n_indices  = in_indices.len();
    if n_indices == 0 { return None; }

    let mut out_verts: Vec<[f32; 3]> = Vec::with_capacity(n_interior + n_indices * 4);
    let mut out_indices: Vec<u32>    = Vec::with_capacity(n_indices + n_indices * 2);

    // Interior triangles — alpha 1.0 everywhere.
    for i in 0..n_interior {
        out_verts.push([in_verts[i * 2], in_verts[i * 2 + 1], 1.0]);
    }
    out_indices.extend_from_slice(in_indices);

    // Halo strips — one quad per boundary edge, outward from the polygon.
    // Tess2 emits triangles in CCW order with the fill on the LEFT of each
    // edge direction, so "right-of-edge" = OUTSIDE the polygon in Y-up.
    let n_tris = n_indices / 3;
    for t in 0..n_tris {
        let ia = in_indices[t * 3    ] as usize;
        let ib = in_indices[t * 3 + 1] as usize;
        let ic = in_indices[t * 3 + 2] as usize;
        if ia >= n_interior || ib >= n_interior || ic >= n_interior { continue; }
        let p = [
            [in_verts[ia * 2], in_verts[ia * 2 + 1]],
            [in_verts[ib * 2], in_verts[ib * 2 + 1]],
            [in_verts[ic * 2], in_verts[ic * 2 + 1]],
        ];
        let flag = [
            edge_flags.get(t * 3    ).copied().unwrap_or(0),
            edge_flags.get(t * 3 + 1).copied().unwrap_or(0),
            edge_flags.get(t * 3 + 2).copied().unwrap_or(0),
        ];
        for k in 0..3 {
            if flag[k] == 0 { continue; }
            let a = p[k];
            let b = p[(k + 1) % 3];
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-6 { continue; }
            // Y-up CCW traversal → interior on LEFT, outward on RIGHT.
            // Right-hand perpendicular of (dx, dy) is (dy, -dx).
            let nx =  dy / len * halo_px;
            let ny = -dx / len * halo_px;

            let base = out_verts.len() as u32;
            out_verts.push([a[0],      a[1],      1.0]); // 0: inner a
            out_verts.push([b[0],      b[1],      1.0]); // 1: inner b
            out_verts.push([a[0] + nx, a[1] + ny, 0.0]); // 2: outer a
            out_verts.push([b[0] + nx, b[1] + ny, 0.0]); // 3: outer b
            // Two tris; winding doesn't matter (2-D, no cull).
            out_indices.extend_from_slice(&[
                base, base + 1, base + 2,
                base + 1, base + 3, base + 2,
            ]);
        }
    }

    Some((out_verts, out_indices))
}

/// Tessellate a filled polygon described by one or more contour rings.
///
/// Each contour is a list of 2-D vertices in Y-up order.  Outer contours
/// should be wound counter-clockwise; holes clockwise (standard even-odd /
/// non-zero fill rules).
///
/// Returns `(flat_xy_vertices, triangle_indices)` where:
/// - `flat_xy_vertices`: interleaved `[x0, y0, x1, y1, …]` as `f32`
/// - `triangle_indices`: triples referencing vertex positions (indices into the
///   vertex array — divide by 2 to get the vertex number)
///
/// Returns `None` if tessellation fails or produces no output.
pub fn tessellate_fill(contours: &[Vec<[f32; 2]>]) -> Option<(Vec<f32>, Vec<u32>)> {
    if contours.is_empty() { return None; }

    let mut tess = Tessellator::new();
    let mut n_added = 0;

    for contour in contours {
        if contour.len() < 3 { continue; }

        // Remove consecutive duplicate vertices (tess2 can panic on zero-length edges).
        let cleaned = deduplicate_contour(contour);
        if cleaned.len() < 3 { continue; }

        // Skip near-zero-area contours — tess2 panics on degenerate (collinear)
        // faces rather than returning an error.  Any polygon with area < 0.5 px²
        // is invisible anyway.
        if signed_area_2x(&cleaned).abs() < 1.0 { continue; }

        // Flatten to [x0, y0, x1, y1, …] — promote to f64 at the tess2
        // boundary so the sweep's edge-sign predicates operate in double
        // precision (required for rotation-stable topology).
        let flat: Vec<f64> = cleaned.iter().flat_map(|v| [v[0] as f64, v[1] as f64]).collect();
        tess.add_contour(2, &flat);
        n_added += 1;
    }

    if n_added == 0 { return None; }

    let ok = tess.tessellate(
        WindingRule::Odd,   // EvenOdd — NonZero panics in tess2-rust on some inputs
        ElementType::Polygons,
        3,   // triangles
        2,   // 2-D vertices
        None,
    );

    if !ok || tess.vertex_count() == 0 { return None; }

    // out_vertices: flat [x, y, x, y, …] — demote back to f32 for the GL
    // vertex buffer (which is f32 today).
    let verts: Vec<f32> = tess.vertices().iter().map(|&v| v as f32).collect();

    // out_elements: flat [i0, i1, i2, …] — triangle vertex indices
    let indices: Vec<u32> = tess.elements().to_vec();

    Some((verts, indices))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// 2× the signed area of a polygon (positive = CCW in Y-up).
/// If |result| < 1.0 the polygon covers less than 0.5 px² and is invisible.
fn signed_area_2x(pts: &[[f32; 2]]) -> f32 {
    let n = pts.len();
    let mut a = 0.0f32;
    for i in 0..n {
        let j = (i + 1) % n;
        a += pts[i][0] * pts[j][1] - pts[j][0] * pts[i][1];
    }
    a
}

/// Remove consecutive duplicate vertices and strip the closing duplicate if
/// the contour ends with a copy of its first vertex.
fn deduplicate_contour(pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    deduplicate_contour_v(pts)
}

/// Same as `deduplicate_contour` but with a shorter name suitable for
/// internal re-use — kept as a separate symbol to avoid churning the public
/// one while the path converter lands.
fn deduplicate_contour_v(pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(pts.len());
    for &pt in pts {
        match out.last() {
            Some(&prev) if prev == pt => {}
            _ => out.push(pt),
        }
    }
    if out.len() >= 2 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// Convert an axis-aligned rectangle into a single contour and tessellate it.
/// Useful for solid-colour fills without a full path builder.
pub fn tessellate_rect(x: f32, y: f32, w: f32, h: f32) -> Option<(Vec<f32>, Vec<u32>)> {
    let contour = vec![
        [x,     y    ],
        [x + w, y    ],
        [x + w, y + h],
        [x,     y + h],
    ];
    tessellate_fill(&[contour])
}

/// Convert a rounded rectangle into a contour (approximated as a polygon with
/// `segments` points per quarter-circle arc) and tessellate.
pub fn tessellate_rounded_rect(
    x: f32, y: f32, w: f32, h: f32,
    r: f32,
    segments: usize,
) -> Option<(Vec<f32>, Vec<u32>)> {
    let r = r.min(w * 0.5).min(h * 0.5);
    let seg = segments.max(3);
    let mut contour: Vec<[f32; 2]> = Vec::with_capacity(seg * 4 + 4);

    use std::f32::consts::PI;

    // Four arc centres (inner rect corners), CCW starting bottom-right.
    let corners = [
        (x + w - r, y + r,     -PI * 0.5, 0.0),        // bottom-right
        (x + w - r, y + h - r,  0.0,      PI * 0.5),   // top-right
        (x + r,     y + h - r,  PI * 0.5, PI),          // top-left
        (x + r,     y + r,      PI,       PI * 1.5),    // bottom-left
    ];

    for &(cx, cy, start, end) in &corners {
        for i in 0..=seg {
            let t = i as f32 / seg as f32;
            let angle = start + t * (end - start);
            contour.push([cx + angle.cos() * r, cy + angle.sin() * r]);
        }
    }

    tessellate_fill(&[contour])
}

/// Build a circle contour and tessellate it.
pub fn tessellate_circle(cx: f32, cy: f32, r: f32, segments: usize) -> Option<(Vec<f32>, Vec<u32>)> {
    let seg = segments.max(8);
    use std::f32::consts::TAU;
    let contour: Vec<[f32; 2]> = (0..seg)
        .map(|i| {
            let angle = i as f32 / seg as f32 * TAU;
            [cx + angle.cos() * r, cy + angle.sin() * r]
        })
        .collect();
    tessellate_fill(&[contour])
}
