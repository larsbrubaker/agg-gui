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

        // Flatten to [x0, y0, x1, y1, …]
        let flat: Vec<f32> = cleaned.iter().flat_map(|v| [v[0], v[1]]).collect();
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

    // out_vertices: flat [x, y, x, y, …]
    let verts: Vec<f32> = tess.vertices().to_vec();

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
