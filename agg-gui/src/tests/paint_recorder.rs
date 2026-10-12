//! `PaintRecorder` — a no-op [`DrawCtx`] that records what a widget's
//! `paint()` asked for, shared by widget unit tests.
//!
//! Several widget test modules (`radio_group`, `progress_bar`, …) carry
//! their own near-identical mock contexts. New widgets should use this
//! one instead of growing another copy: it captures every `fill()` /
//! `stroke()` with the colour in effect at that moment plus the kind of
//! path primitive that preceded it, which is enough to assert things
//! like "exactly one shape is filled with the accent colour" or "the
//! spinner strokes twelve spokes" without rasterizing anything. It also
//! tracks the device bounds of each filled path inside the clip
//! (`filled_area`), the cost measure of an LCD fill.

use std::sync::Arc;

use agg_rust::comp_op::CompOp;
use agg_rust::math_stroke::{LineCap, LineJoin};
use agg_rust::trans_affine::TransAffine;

use crate::color::Color;
use crate::draw_ctx::{DrawCtx, FillRule, GlPaint, LinearGradientPaint, RadialGradientPaint};
use crate::geometry::Rect;
use crate::text::{Font, TextMetrics};

/// The last path primitive appended before a `fill()` / `stroke()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PathKind {
    /// `begin_path` only — nothing appended yet.
    Empty,
    /// Built from `move_to` / `line_to` / `cubic_to` / `quad_to` / `arc_to`.
    Freeform,
    Circle,
    Rect,
    RoundedRect,
}

/// One recorded path-building call, with its coordinates.
///
/// Used by tests that need to inspect the geometry a `DrawCtx` default
/// implementation emits (e.g. `DrawCtx::ellipse` lowering to `cubic_to`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PathOp {
    MoveTo(f64, f64),
    LineTo(f64, f64),
    /// `(cx1, cy1, cx2, cy2, x, y)`
    CubicTo(f64, f64, f64, f64, f64, f64),
    ClosePath,
}

/// One recorded `fill()` or `stroke()` call.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PaintOp {
    pub color: Color,
    pub kind: PathKind,
}

/// A device-space rectangle: left, bottom, right, top.
pub(crate) type DeviceRect = (f64, f64, f64, f64);

pub(crate) struct PaintRecorder {
    transform: TransAffine,
    stack: Vec<(TransAffine, Option<DeviceRect>)>,
    /// The clip in device space, `None` for none.
    clip: Option<DeviceRect>,
    /// Device bounds of the path built since the last `begin_path`.
    path_bounds: Option<DeviceRect>,
    /// Sum over every `fill()` of its path's device bounds inside the clip
    /// (agg-sharp's `FillAreaCountingGraphics2D`): what an LCD fill costs.
    pub filled_area: f64,
    /// Every `clear_rect` call, in device space.
    pub clears: Vec<DeviceRect>,
    fill_color: Color,
    stroke_color: Color,
    last_kind: PathKind,
    pub fills: Vec<PaintOp>,
    pub strokes: Vec<PaintOp>,
    /// Path-building calls in order, with coordinates.
    pub path_ops: Vec<PathOp>,
    /// Every `fill_text` call: the text and its baseline origin.
    pub texts: Vec<(String, f64, f64)>,
    /// When set ([`PaintRecorder::with_real_fonts`]), `measure_text` measures
    /// with the font and size the widget set, instead of the fixed seven
    /// pixels a character.
    real_fonts: bool,
    font: Option<Arc<Font>>,
    font_size: f64,
}

impl PaintRecorder {
    pub(crate) fn new() -> Self {
        Self {
            transform: TransAffine::new(),
            stack: Vec::new(),
            clip: None,
            path_bounds: None,
            filled_area: 0.0,
            clears: Vec::new(),
            fill_color: Color::rgba(0.0, 0.0, 0.0, 0.0),
            stroke_color: Color::rgba(0.0, 0.0, 0.0, 0.0),
            last_kind: PathKind::Empty,
            fills: Vec::new(),
            strokes: Vec::new(),
            path_ops: Vec::new(),
            texts: Vec::new(),
            real_fonts: false,
            font: None,
            font_size: 0.0,
        }
    }

    /// A recorder whose `measure_text` measures with the real font the
    /// widget set (`measure_text_metrics`), so recorded baselines are the
    /// ones the widget computes for that face.
    pub(crate) fn with_real_fonts() -> Self {
        Self {
            real_fonts: true,
            ..Self::new()
        }
    }

    /// A recorder whose target is `width`×`height` device pixels: fills
    /// outside it are clipped off [`filled_area`](Self::filled_area).
    pub(crate) fn with_target(width: f64, height: f64) -> Self {
        Self {
            clip: Some((0.0, 0.0, width, height)),
            ..Self::new()
        }
    }

    /// Grow the path's device bounds by the local point `(x, y)`.
    fn extend(&mut self, mut x: f64, mut y: f64) {
        self.transform.transform(&mut x, &mut y);
        let b = self.path_bounds.unwrap_or((x, y, x, y));
        self.path_bounds = Some((b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y)));
    }

    fn extend_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        for (px, py) in [(x, y), (x + w, y), (x, y + h), (x + w, y + h)] {
            self.extend(px, py);
        }
    }

    /// Number of `fill()` calls made with exactly `color` in effect.
    pub(crate) fn fills_with(&self, color: Color) -> usize {
        self.fills.iter().filter(|op| op.color == color).count()
    }

    /// Number of `stroke()` calls made with exactly `color` in effect.
    pub(crate) fn strokes_with(&self, color: Color) -> usize {
        self.strokes.iter().filter(|op| op.color == color).count()
    }
}

impl DrawCtx for PaintRecorder {
    fn set_fill_color(&mut self, color: Color) {
        self.fill_color = color;
    }
    fn set_stroke_color(&mut self, color: Color) {
        self.stroke_color = color;
    }
    fn set_fill_linear_gradient(&mut self, _gradient: LinearGradientPaint) {}
    fn set_fill_radial_gradient(&mut self, _gradient: RadialGradientPaint) {}
    fn set_line_width(&mut self, _w: f64) {}
    fn set_line_join(&mut self, _join: LineJoin) {}
    fn set_line_cap(&mut self, _cap: LineCap) {}
    fn set_miter_limit(&mut self, _limit: f64) {}
    fn set_line_dash(&mut self, _dashes: &[f64], _offset: f64) {}
    fn set_blend_mode(&mut self, _mode: CompOp) {}
    fn set_global_alpha(&mut self, _alpha: f64) {}
    fn set_fill_rule(&mut self, _rule: FillRule) {}
    fn set_font(&mut self, font: Arc<Font>) {
        self.font = Some(font);
    }
    fn set_font_size(&mut self, size: f64) {
        self.font_size = size;
    }
    fn clip_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let saved = self.path_bounds.take();
        self.extend_rect(x, y, w, h);
        let r = self.path_bounds.take().unwrap_or((x, y, x, y));
        self.path_bounds = saved;
        self.clip = Some(match self.clip {
            Some(c) => (c.0.max(r.0), c.1.max(r.1), c.2.min(r.2), c.3.min(r.3)),
            None => r,
        });
    }
    fn reset_clip(&mut self) {
        self.clip = None;
    }
    fn clear(&mut self, _color: Color) {}
    fn clear_rect(&mut self, x: f64, y: f64, w: f64, h: f64, _color: Color) {
        self.clears.push((x, y, x + w, y + h));
    }
    fn begin_path(&mut self) {
        self.last_kind = PathKind::Empty;
        self.path_ops.clear();
        self.path_bounds = None;
    }
    fn move_to(&mut self, x: f64, y: f64) {
        self.last_kind = PathKind::Freeform;
        self.path_ops.push(PathOp::MoveTo(x, y));
        self.extend(x, y);
    }
    fn line_to(&mut self, x: f64, y: f64) {
        self.last_kind = PathKind::Freeform;
        self.path_ops.push(PathOp::LineTo(x, y));
        self.extend(x, y);
    }
    fn cubic_to(&mut self, cx1: f64, cy1: f64, cx2: f64, cy2: f64, x: f64, y: f64) {
        self.last_kind = PathKind::Freeform;
        self.extend(cx1, cy1);
        self.extend(cx2, cy2);
        self.extend(x, y);
        self.path_ops
            .push(PathOp::CubicTo(cx1, cy1, cx2, cy2, x, y));
    }
    fn quad_to(&mut self, _cx: f64, _cy: f64, _x: f64, _y: f64) {
        self.last_kind = PathKind::Freeform;
    }
    fn arc_to(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64, _ccw: bool) {
        self.last_kind = PathKind::Freeform;
    }
    fn circle(&mut self, _cx: f64, _cy: f64, _r: f64) {
        self.last_kind = PathKind::Circle;
    }
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.last_kind = PathKind::Rect;
        self.extend_rect(x, y, w, h);
    }
    fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, _r: f64) {
        self.last_kind = PathKind::RoundedRect;
        self.extend_rect(x, y, w, h);
    }
    fn close_path(&mut self) {
        self.path_ops.push(PathOp::ClosePath);
    }
    fn fill(&mut self) {
        if let Some(b) = self.path_bounds {
            let c = self.clip.unwrap_or(b);
            let w = b.2.min(c.2) - b.0.max(c.0);
            let h = b.3.min(c.3) - b.1.max(c.1);
            if w > 0.0 && h > 0.0 {
                self.filled_area += w * h;
            }
        }
        self.fills.push(PaintOp {
            color: self.fill_color,
            kind: self.last_kind,
        });
    }
    fn stroke(&mut self) {
        self.strokes.push(PaintOp {
            color: self.stroke_color,
            kind: self.last_kind,
        });
    }
    fn fill_and_stroke(&mut self) {
        self.fill();
        self.stroke();
    }
    fn draw_triangles_aa(&mut self, _vertices: &[[f32; 3]], _indices: &[u32], _color: Color) {}
    fn fill_text(&mut self, text: &str, x: f64, y: f64) {
        self.texts.push((text.to_string(), x, y));
    }
    fn fill_text_gsv(&mut self, _text: &str, _x: f64, _y: f64, _size: f64) {}
    fn measure_text(&self, text: &str) -> Option<TextMetrics> {
        if let (true, Some(font)) = (self.real_fonts, self.font.as_ref()) {
            return Some(crate::text::measure_text_metrics(
                font,
                text,
                self.font_size,
            ));
        }
        Some(TextMetrics {
            width: text.chars().count() as f64 * 7.0,
            ascent: 10.0,
            descent: 3.0,
            line_height: 16.0,
        })
    }
    fn transform(&self) -> TransAffine {
        self.transform
    }
    fn save(&mut self) {
        self.stack.push((self.transform, self.clip));
    }
    fn restore(&mut self) {
        if let Some((t, clip)) = self.stack.pop() {
            self.transform = t;
            self.clip = clip;
        }
    }
    fn translate(&mut self, tx: f64, ty: f64) {
        self.transform
            .premultiply(&TransAffine::new_translation(tx, ty));
    }
    fn rotate(&mut self, radians: f64) {
        self.transform
            .premultiply(&TransAffine::new_rotation(radians));
    }
    fn scale(&mut self, sx: f64, sy: f64) {
        self.transform
            .premultiply(&TransAffine::new_scaling(sx, sy));
    }
    fn set_transform(&mut self, m: TransAffine) {
        self.transform = m;
    }
    fn reset_transform(&mut self) {
        self.transform = TransAffine::new();
    }
    fn gl_paint(&mut self, _screen_rect: Rect, _painter: &mut dyn GlPaint) {}
}
