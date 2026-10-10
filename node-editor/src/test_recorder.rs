//! A recording `DrawCtx` for the presentation tests (`socket_style_tests`,
//! `widget/tests_presentation`): every fill and stroke becomes a [`Shot`]
//! holding its colour, line width, dash pattern and path, with coordinates
//! mapped through the translate / uniform scale in effect, so a test can
//! assert exactly which shapes were drawn where.

use agg_gui::draw_ctx::{LinearGradientPaint, RadialGradientPaint};
use agg_gui::{
    Color, CompOp, DrawCtx, FillRule, Font, LineCap, LineJoin, TextMetrics, TransAffine,
};

/// One path element, in recorder (device) coordinates.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Op {
    Circle([f64; 2], f64),
    Rect([f64; 2], [f64; 2]),
    RoundedRect([f64; 2], [f64; 2], f64),
    MoveTo([f64; 2]),
    LineTo([f64; 2]),
    /// A cubic: its two control points and its end.
    Cubic([f64; 2], [f64; 2], [f64; 2]),
    Close,
}

/// One fill or stroke.
#[derive(Clone, Debug)]
pub(crate) struct Shot {
    pub fill: bool,
    pub color: Color,
    pub width: f64,
    pub dash: Vec<f64>,
    pub path: Vec<Op>,
}

#[derive(Clone, Copy)]
struct Xf {
    s: f64,
    tx: f64,
    ty: f64,
}

pub(crate) struct Recorder {
    fill_color: Color,
    stroke_color: Color,
    width: f64,
    dash: Vec<f64>,
    path: Vec<Op>,
    xf: Xf,
    stack: Vec<Xf>,
    pub shots: Vec<Shot>,
    pub texts: Vec<String>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            fill_color: Color::rgba(0.0, 0.0, 0.0, 1.0),
            stroke_color: Color::rgba(0.0, 0.0, 0.0, 1.0),
            width: 1.0,
            dash: Vec::new(),
            path: Vec::new(),
            xf: Xf {
                s: 1.0,
                tx: 0.0,
                ty: 0.0,
            },
            stack: Vec::new(),
            shots: Vec::new(),
            texts: Vec::new(),
        }
    }
}

/// Colour equality to f32 rounding.
pub(crate) fn same(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 1e-4
        && (a.g - b.g).abs() < 1e-4
        && (a.b - b.b).abs() < 1e-4
        && (a.a - b.a).abs() < 1e-4
}

impl Recorder {
    fn p(&self, x: f64, y: f64) -> [f64; 2] {
        [x * self.xf.s + self.xf.tx, y * self.xf.s + self.xf.ty]
    }
    fn shoot(&mut self, fill: bool) {
        let path = std::mem::take(&mut self.path);
        self.shots.push(Shot {
            fill,
            color: if fill {
                self.fill_color
            } else {
                self.stroke_color
            },
            width: self.width * self.xf.s,
            dash: self.dash.clone(),
            path,
        });
    }
    pub fn fills(&self) -> impl Iterator<Item = &Shot> {
        self.shots.iter().filter(|s| s.fill)
    }
    pub fn strokes(&self) -> impl Iterator<Item = &Shot> {
        self.shots.iter().filter(|s| !s.fill)
    }
}

impl DrawCtx for Recorder {
    fn set_fill_color(&mut self, color: Color) {
        self.fill_color = color;
    }
    fn set_stroke_color(&mut self, color: Color) {
        self.stroke_color = color;
    }
    fn set_fill_linear_gradient(&mut self, _g: LinearGradientPaint) {}
    fn set_fill_radial_gradient(&mut self, _g: RadialGradientPaint) {}
    fn set_line_width(&mut self, w: f64) {
        self.width = w;
    }
    fn set_line_join(&mut self, _join: LineJoin) {}
    fn set_line_cap(&mut self, _cap: LineCap) {}
    fn set_miter_limit(&mut self, _limit: f64) {}
    fn set_line_dash(&mut self, dashes: &[f64], _offset: f64) {
        self.dash = dashes.to_vec();
    }
    fn set_blend_mode(&mut self, _mode: CompOp) {}
    fn set_global_alpha(&mut self, _alpha: f64) {}
    fn set_fill_rule(&mut self, _rule: FillRule) {}
    fn set_font(&mut self, _font: std::sync::Arc<Font>) {}
    fn set_font_size(&mut self, _size: f64) {}
    fn clip_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn reset_clip(&mut self) {}
    fn clear(&mut self, _color: Color) {}
    fn begin_path(&mut self) {
        self.path.clear();
    }
    fn move_to(&mut self, x: f64, y: f64) {
        let p = self.p(x, y);
        self.path.push(Op::MoveTo(p));
    }
    fn line_to(&mut self, x: f64, y: f64) {
        let p = self.p(x, y);
        self.path.push(Op::LineTo(p));
    }
    fn cubic_to(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        let op = Op::Cubic(self.p(a, b), self.p(c, d), self.p(e, f));
        self.path.push(op);
    }
    fn quad_to(&mut self, _cx: f64, _cy: f64, _x: f64, _y: f64) {}
    fn arc_to(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64, _ccw: bool) {}
    fn circle(&mut self, cx: f64, cy: f64, r: f64) {
        let op = Op::Circle(self.p(cx, cy), r * self.xf.s);
        self.path.push(op);
    }
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let op = Op::Rect(self.p(x, y), [w * self.xf.s, h * self.xf.s]);
        self.path.push(op);
    }
    fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        let s = self.xf.s;
        let op = Op::RoundedRect(self.p(x, y), [w * s, h * s], r * s);
        self.path.push(op);
    }
    fn close_path(&mut self) {
        self.path.push(Op::Close);
    }
    fn fill(&mut self) {
        self.shoot(true);
    }
    fn stroke(&mut self) {
        self.shoot(false);
    }
    fn fill_and_stroke(&mut self) {
        self.shoot(true);
    }
    fn draw_triangles_aa(&mut self, _v: &[[f32; 3]], _i: &[u32], _c: Color) {}
    fn fill_text(&mut self, text: &str, _x: f64, _y: f64) {
        self.texts.push(text.to_string());
    }
    fn fill_text_gsv(&mut self, text: &str, _x: f64, _y: f64, _size: f64) {
        self.texts.push(text.to_string());
    }
    fn measure_text(&self, _text: &str) -> Option<TextMetrics> {
        None
    }
    fn transform(&self) -> TransAffine {
        TransAffine::new()
    }
    fn set_transform(&mut self, _m: TransAffine) {}
    fn reset_transform(&mut self) {}
    fn save(&mut self) {
        self.stack.push(self.xf);
    }
    fn restore(&mut self) {
        if let Some(x) = self.stack.pop() {
            self.xf = x;
        }
    }
    fn translate(&mut self, tx: f64, ty: f64) {
        self.xf.tx += tx * self.xf.s;
        self.xf.ty += ty * self.xf.s;
    }
    fn rotate(&mut self, _radians: f64) {}
    fn scale(&mut self, sx: f64, _sy: f64) {
        self.xf.s *= sx;
    }
}
