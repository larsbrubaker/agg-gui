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

    for contour in contours {
        if contour.len() < 3 { continue; }
        // Flatten to [x0, y0, x1, y1, …]
        let flat: Vec<f32> = contour.iter().flat_map(|v| [v[0], v[1]]).collect();
        tess.add_contour(2, &flat);
    }

    let ok = tess.tessellate(
        WindingRule::NonZero,
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
