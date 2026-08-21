//! `PaintRecorder` — a no-op [`DrawCtx`] that records what a widget's
//! `paint()` asked for, shared by widget unit tests.
//!
//! Several widget test modules (`radio_group`, `progress_bar`, …) carry
//! their own near-identical mock contexts. New widgets should use this
//! one instead of growing another copy: it captures every `fill()` /
//! `stroke()` with the colour in effect at that moment plus the kind of
//! path primitive that preceded it, which is enough to assert things
//! like "exactly one shape is filled with the accent colour" or "the
//! spinner strokes twelve spokes" without rasterizing anything.

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

/// One recorded `fill()` or `stroke()` call.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PaintOp {
    pub color: Color,
    pub kind: PathKind,
}

pub(crate) struct PaintRecorder {
    transform: TransAffine,
    stack: Vec<TransAffine>,
    fill_color: Color,
    stroke_color: Color,
    last_kind: PathKind,
    pub fills: Vec<PaintOp>,
    pub strokes: Vec<PaintOp>,
}

impl PaintRecorder {
    pub(crate) fn new() -> Self {
        Self {
            transform: TransAffine::new(),
            stack: Vec::new(),
            fill_color: Color::rgba(0.0, 0.0, 0.0, 0.0),
            stroke_color: Color::rgba(0.0, 0.0, 0.0, 0.0),
            last_kind: PathKind::Empty,
            fills: Vec::new(),
            strokes: Vec::new(),
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
    fn set_font(&mut self, _font: Arc<Font>) {}
    fn set_font_size(&mut self, _size: f64) {}
    fn clip_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn reset_clip(&mut self) {}
    fn clear(&mut self, _color: Color) {}
    fn begin_path(&mut self) {
        self.last_kind = PathKind::Empty;
    }
    fn move_to(&mut self, _x: f64, _y: f64) {
        self.last_kind = PathKind::Freeform;
    }
    fn line_to(&mut self, _x: f64, _y: f64) {
        self.last_kind = PathKind::Freeform;
    }
    fn cubic_to(&mut self, _cx1: f64, _cy1: f64, _cx2: f64, _cy2: f64, _x: f64, _y: f64) {
        self.last_kind = PathKind::Freeform;
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
    fn rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {
        self.last_kind = PathKind::Rect;
    }
    fn rounded_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64, _r: f64) {
        self.last_kind = PathKind::RoundedRect;
    }
    fn close_path(&mut self) {}
    fn fill(&mut self) {
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
    fn fill_text(&mut self, _text: &str, _x: f64, _y: f64) {}
    fn fill_text_gsv(&mut self, _text: &str, _x: f64, _y: f64, _size: f64) {}
    fn measure_text(&self, text: &str) -> Option<TextMetrics> {
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
        self.stack.push(self.transform);
    }
    fn restore(&mut self) {
        if let Some(t) = self.stack.pop() {
            self.transform = t;
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
