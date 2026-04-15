//! Bézier-flattening text pipeline for AGG-based software rasterization.
//!
//! This module provides the older, non-cached text path that flattens glyph
//! Bézier curves into polylines suitable for either the AGG software rasterizer
//! or tess2 tessellation.  It is kept for the demo WASM soft-render path and
//! for the test suite that validates subdivision correctness.
//!
//! The primary GL rendering path now uses [`crate::gl_renderer::GlyphCache`]
//! together with [`super::shape_glyphs`] and [`super::flatten_glyph_at_origin`],
//! which tessellates each glyph once and caches the result indefinitely.
//!
//! # Relation to the rest of the text pipeline
//!
//! ```text
//! shape_and_flatten_text       — direct flatten, flatness-controlled, no cache
//! shape_and_flatten_text_via_agg — AGG ConvCurve flatten, per-glyph grouping
//! flatten_glyph_at_origin      — single-glyph, origin 0,0, feeds GlyphCache
//! ```

use agg_rust::basics::{is_end_poly, is_move_to, is_stop, PATH_CMD_LINE_TO, VertexSource};
use agg_rust::conv_curve::ConvCurve;

use super::{Font, GlyphPathBuilder};

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Shape `text` and return all glyph contours flattened to polylines.
///
/// Bézier curves are approximated with line segments at `flatness` pixels
/// tolerance (a good default is `0.5`).  The returned `Vec<Vec<[f32;2]>>`
/// has one inner `Vec` per closed contour; tess2 can tessellate the whole
/// list directly.
///
/// `(x, y)` is the baseline-left origin in Y-up pixel space.
pub fn shape_and_flatten_text(
    font: &Font,
    text: &str,
    size: f64,
    x: f64,
    y: f64,
    flatness: f64,
) -> Vec<Vec<[f32; 2]>> {
    let scale = size / font.units_per_em() as f64;
    font.with_rb_face(|face| {
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        let output = rustybuzz::shape(face, &[], buffer);
        let mut all_contours: Vec<Vec<[f32; 2]>> = Vec::new();
        let mut pen_x = x;

        for (info, pos) in output
            .glyph_infos()
            .iter()
            .zip(output.glyph_positions().iter())
        {
            let gid = ttf_parser::GlyphId(info.glyph_id as u16);
            let gx = pen_x + pos.x_offset as f64 * scale;
            let gy = y + pos.y_offset as f64 * scale;

            let mut builder = FlatContourBuilder::new(gx, gy, scale, flatness);
            face.outline_glyph(gid, &mut builder);
            builder.flush();
            all_contours.extend(builder.contours);

            pen_x += pos.x_advance as f64 * scale;
        }
        all_contours
    })
}

/// Shape `text` and return glyph contours flattened by AGG's own `ConvCurve`,
/// grouped **per glyph**.
///
/// Uses AGG's `ConvCurve` — the same Bézier flattener that `GfxCtx::fill` /
/// `rasterize_fill_path` use internally — so tess2 sees identical geometry to
/// the software rasterizer.
///
/// Returns `Vec<Vec<Vec<[f32; 2]>>>`:
/// - outer `Vec`: one entry per shaped glyph
/// - middle `Vec`: contours belonging to that glyph (e.g. 'O' has outer + inner)
/// - inner `Vec`: flattened polyline points for one contour
///
/// Keeping contours grouped per glyph lets the caller tessellate each glyph
/// with the EvenOdd rule so counters (holes in O, D, B, R …) are handled
/// correctly, while strokes from different glyphs never interact.
///
/// `(x, y)` is the baseline-left origin in Y-up pixel space.
pub fn shape_and_flatten_text_via_agg(
    font: &Font,
    text: &str,
    size: f64,
    x: f64,
    y: f64,
) -> Vec<Vec<Vec<[f32; 2]>>> {
    let scale = size / font.units_per_em() as f64;
    font.with_rb_face(|face| {
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        let output = rustybuzz::shape(face, &[], buffer);
        let mut all_glyphs: Vec<Vec<Vec<[f32; 2]>>> = Vec::new();
        let mut pen_x = x;

        for (info, pos) in output
            .glyph_infos()
            .iter()
            .zip(output.glyph_positions().iter())
        {
            let gid = ttf_parser::GlyphId(info.glyph_id as u16);
            let gx = pen_x + pos.x_offset as f64 * scale;
            let gy = y + pos.y_offset as f64 * scale;

            let mut builder = GlyphPathBuilder::new(gx, gy, scale);
            let has_outline = face.outline_glyph(gid, &mut builder).is_some();

            if has_outline && builder.has_outline {
                // Flatten via AGG's ConvCurve — same algorithm as the software path.
                let mut curves = ConvCurve::new(builder.path);
                curves.rewind(0);

                let mut glyph_contours: Vec<Vec<[f32; 2]>> = Vec::new();
                let mut current: Vec<[f32; 2]> = Vec::new();

                loop {
                    let (mut cx, mut cy) = (0.0_f64, 0.0_f64);
                    let cmd = curves.vertex(&mut cx, &mut cy);
                    if is_stop(cmd) { break; }
                    if is_move_to(cmd) {
                        if current.len() >= 3 {
                            glyph_contours.push(std::mem::take(&mut current));
                        } else {
                            current.clear();
                        }
                        current.push([cx as f32, cy as f32]);
                    } else if cmd == PATH_CMD_LINE_TO {
                        current.push([cx as f32, cy as f32]);
                    } else if is_end_poly(cmd) {
                        if current.len() >= 3 {
                            glyph_contours.push(std::mem::take(&mut current));
                        } else {
                            current.clear();
                        }
                    }
                }
                if current.len() >= 3 {
                    glyph_contours.push(current);
                }
                if !glyph_contours.is_empty() {
                    all_glyphs.push(glyph_contours);
                }
            }

            pen_x += pos.x_advance as f64 * scale;
        }
        all_glyphs
    })
}

// ---------------------------------------------------------------------------
// FlatContourBuilder — custom Bézier subdivision into polylines
// ---------------------------------------------------------------------------

/// Converts ttf-parser outline callbacks into flat polyline contours.
///
/// Bézier curves are subdivided until each segment is within `flatness` pixels.
struct FlatContourBuilder {
    pub contours: Vec<Vec<[f32; 2]>>,
    current: Vec<[f32; 2]>,
    ox: f64,
    oy: f64,
    scale: f64,
    flatness_sq: f64,
    /// Last pen position in font units (before origin/scale).
    pen: [f64; 2],
}

impl FlatContourBuilder {
    fn new(ox: f64, oy: f64, scale: f64, flatness: f64) -> Self {
        Self {
            contours: Vec::new(),
            current: Vec::new(),
            ox, oy, scale,
            flatness_sq: flatness * flatness,
            pen: [0.0, 0.0],
        }
    }

    #[inline]
    fn screen(&self, fx: f32, fy: f32) -> [f32; 2] {
        [(self.ox + fx as f64 * self.scale) as f32,
         (self.oy + fy as f64 * self.scale) as f32]
    }

    fn push(&mut self, fx: f32, fy: f32) {
        let pt = self.screen(fx, fy);
        self.current.push(pt);
        self.pen = [fx as f64, fy as f64];
    }

    fn flatten_quad(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = [self.pen[0] as f32, self.pen[1] as f32];
        subdivide_quad(p0, [x1, y1], [x, y], self.flatness_sq, &mut self.current,
                       self.ox, self.oy, self.scale);
        let pt = self.screen(x, y);
        self.current.push(pt);
        self.pen = [x as f64, y as f64];
    }

    fn flatten_cubic(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = [self.pen[0] as f32, self.pen[1] as f32];
        subdivide_cubic(p0, [x1, y1], [x2, y2], [x, y], self.flatness_sq,
                        &mut self.current, self.ox, self.oy, self.scale);
        let pt = self.screen(x, y);
        self.current.push(pt);
        self.pen = [x as f64, y as f64];
    }

    fn flush(&mut self) {
        if self.current.len() >= 3 {
            let c = std::mem::take(&mut self.current);
            self.contours.push(c);
        } else {
            self.current.clear();
        }
    }
}

impl ttf_parser::OutlineBuilder for FlatContourBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        self.pen = [x as f64, y as f64];
        let pt = self.screen(x, y);
        self.current.push(pt);
    }
    fn line_to(&mut self, x: f32, y: f32) { self.push(x, y); }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.flatten_quad(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.flatten_cubic(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) { self.flush(); }
}

// ---------------------------------------------------------------------------
// Bézier subdivision helpers
// ---------------------------------------------------------------------------

/// Recursively subdivide a quadratic Bézier until flat (in screen space).
///
/// Control points are in **font units**; flatness_sq and output are in
/// **screen pixels**.  Mirrors the same approach used in `subdivide_cubic`.
fn subdivide_quad(
    p0: [f32; 2], p1: [f32; 2], p2: [f32; 2],
    flatness_sq: f64,
    out: &mut Vec<[f32; 2]>,
    ox: f64, oy: f64, scale: f64,
) {
    let s = |v: [f32; 2]| -> [f32; 2] {
        [(ox + v[0] as f64 * scale) as f32, (oy + v[1] as f64 * scale) as f32]
    };
    let sp0 = s(p0); let sp1 = s(p1); let sp2 = s(p2);
    let mx = (sp0[0] + 2.0 * sp1[0] + sp2[0]) * 0.25;
    let my = (sp0[1] + 2.0 * sp1[1] + sp2[1]) * 0.25;
    let mid_x = (sp0[0] + sp2[0]) * 0.5;
    let mid_y = (sp0[1] + sp2[1]) * 0.5;
    let dx = (mx - mid_x) as f64;
    let dy = (my - mid_y) as f64;
    if dx * dx + dy * dy <= flatness_sq { return; }
    let q0  = [(p0[0] + p1[0]) * 0.5, (p0[1] + p1[1]) * 0.5];
    let q1  = [(p1[0] + p2[0]) * 0.5, (p1[1] + p2[1]) * 0.5];
    let mid = [(q0[0] + q1[0]) * 0.5, (q0[1] + q1[1]) * 0.5];
    subdivide_quad(p0, q0, mid, flatness_sq, out, ox, oy, scale);
    out.push(s(mid));
    subdivide_quad(mid, q1, p2, flatness_sq, out, ox, oy, scale);
}

/// Recursively subdivide a cubic Bézier until flat (in screen space).
fn subdivide_cubic(
    p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2],
    flatness_sq: f64,
    out: &mut Vec<[f32; 2]>,
    ox: f64, oy: f64, scale: f64,
) {
    let s = |v: [f32; 2]| -> [f32; 2] {
        [(ox + v[0] as f64 * scale) as f32, (oy + v[1] as f64 * scale) as f32]
    };
    let sp0 = s(p0); let sp3 = s(p3);
    let sp1 = s(p1); let sp2 = s(p2);
    let ux = 3.0 * sp1[0] - 2.0 * sp0[0] - sp3[0];
    let uy = 3.0 * sp1[1] - 2.0 * sp0[1] - sp3[1];
    let vx = 3.0 * sp2[0] - 2.0 * sp3[0] - sp0[0];
    let vy = 3.0 * sp2[1] - 2.0 * sp3[1] - sp0[1];
    let u = ux * ux + uy * uy;
    let v = vx * vx + vy * vy;
    if (if u > v { u } else { v }) as f64 <= flatness_sq * 16.0 { return; }
    let q0 = [(p0[0]+p1[0])*0.5, (p0[1]+p1[1])*0.5];
    let q1 = [(p1[0]+p2[0])*0.5, (p1[1]+p2[1])*0.5];
    let q2 = [(p2[0]+p3[0])*0.5, (p2[1]+p3[1])*0.5];
    let r0 = [(q0[0]+q1[0])*0.5, (q0[1]+q1[1])*0.5];
    let r1 = [(q1[0]+q2[0])*0.5, (q1[1]+q2[1])*0.5];
    let mid = [(r0[0]+r1[0])*0.5, (r0[1]+r1[1])*0.5];
    subdivide_cubic(p0, q0, r0, mid, flatness_sq, out, ox, oy, scale);
    out.push(s(mid));
    subdivide_cubic(mid, r1, q2, p3, flatness_sq, out, ox, oy, scale);
}
