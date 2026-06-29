//! Regression tests: `Label` text colour must follow the active theme.
//!
//! These pin the fix for the Mobile Keyboard demo, where every caption was
//! built with a hard-coded light-grey `Color::from_rgb8(...)`.  Those greys
//! were tuned for the dark palette and turned into unreadable light-on-light
//! text after a switch to the light theme.  The cure is to let `Label`
//! resolve its colour from `Visuals` at paint time:
//!
//! - no explicit colour      → `visuals().text_color`
//! - `.with_dim(true)`       → `visuals().text_dim`  (hints / captions)
//!
//! Both must re-resolve when the palette flips, so a dark/light toggle keeps
//! the text legible.  We capture the colour `Label::paint` feeds to
//! `set_fill_color` (called once, immediately before `fill_text`) and assert
//! it tracks the palette.

use super::*;

use crate::draw_ctx::{DrawCtx, FillRule, GlPaint, LinearGradientPaint};
use crate::text::{Font, TextMetrics};
use crate::theme::{current_visuals, set_visuals, Visuals};
use crate::widgets::Label;
use crate::Rect;
use agg_rust::comp_op::CompOp;
use agg_rust::math_stroke::{LineCap, LineJoin};
use agg_rust::trans_affine::TransAffine;
use std::sync::Arc;

/// A `DrawCtx` that records the most recent `set_fill_color`.  `measure_text`
/// returns a non-`None` metric so `Label::paint` actually reaches its
/// `fill_text` call (and thus the `set_fill_color` that precedes it).
struct ColorCaptureCtx {
    transform: TransAffine,
    stack: Vec<TransAffine>,
    last_fill: Option<Color>,
}

impl ColorCaptureCtx {
    fn new() -> Self {
        Self {
            transform: TransAffine::new(),
            stack: Vec::new(),
            last_fill: None,
        }
    }
}

impl DrawCtx for ColorCaptureCtx {
    fn set_fill_color(&mut self, color: Color) {
        self.last_fill = Some(color);
    }
    fn set_stroke_color(&mut self, _color: Color) {}
    fn set_fill_linear_gradient(&mut self, _gradient: LinearGradientPaint) {}
    fn set_fill_radial_gradient(&mut self, _gradient: crate::draw_ctx::RadialGradientPaint) {}
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
    fn begin_path(&mut self) {}
    fn move_to(&mut self, _x: f64, _y: f64) {}
    fn line_to(&mut self, _x: f64, _y: f64) {}
    fn cubic_to(&mut self, _cx1: f64, _cy1: f64, _cx2: f64, _cy2: f64, _x: f64, _y: f64) {}
    fn quad_to(&mut self, _cx: f64, _cy: f64, _x: f64, _y: f64) {}
    fn arc_to(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64, _ccw: bool) {}
    fn circle(&mut self, _cx: f64, _cy: f64, _r: f64) {}
    fn rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn rounded_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64, _r: f64) {}
    fn close_path(&mut self) {}
    fn fill(&mut self) {}
    fn stroke(&mut self) {}
    fn fill_and_stroke(&mut self) {}
    fn draw_triangles_aa(&mut self, _vertices: &[[f32; 3]], _indices: &[u32], _color: Color) {}
    fn fill_text(&mut self, _text: &str, _x: f64, _y: f64) {}
    fn fill_text_gsv(&mut self, _text: &str, _x: f64, _y: f64, _size: f64) {}
    fn measure_text(&self, _text: &str) -> Option<TextMetrics> {
        Some(TextMetrics {
            width: 40.0,
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
        self.transform.premultiply(&TransAffine::new_scaling(sx, sy));
    }
    fn set_transform(&mut self, m: TransAffine) {
        self.transform = m;
    }
    fn reset_transform(&mut self) {
        self.transform = TransAffine::new();
    }
    fn gl_paint(&mut self, _screen_rect: Rect, _painter: &mut dyn GlPaint) {}
}

/// Paint `label` once and return the colour it fed to `set_fill_color`.
fn painted_color(label: &mut Label) -> Color {
    label.set_bounds(Rect::new(0.0, 0.0, 120.0, 20.0));
    let mut ctx = ColorCaptureCtx::new();
    label.paint(&mut ctx);
    ctx.last_fill.expect("Label::paint must set a fill colour")
}

fn test_font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("font ok"))
}

/// A plain label (no explicit colour) follows `visuals().text_color`, and a
/// `.with_dim(true)` label follows `visuals().text_dim`, in BOTH palettes.
/// The dark→light flip is the crux: a hard-coded grey would not move.
#[test]
fn label_colors_track_theme_palette() {
    let font = test_font();

    for visuals in [Visuals::light(), Visuals::dark()] {
        set_visuals(visuals);
        let v = current_visuals();

        let mut body = Label::new("Body", Arc::clone(&font));
        assert_eq!(
            painted_color(&mut body),
            v.text_color,
            "plain Label must paint with the theme's text_color"
        );

        let mut hint = Label::new("Hint", Arc::clone(&font)).with_dim(true);
        assert_eq!(
            painted_color(&mut hint),
            v.text_dim,
            "with_dim(true) Label must paint with the theme's text_dim"
        );
    }
}

/// `text_dim` and `text_color` are distinct, so a dim caption is visibly
/// dimmer than body text (the visual hierarchy the demo relies on) rather
/// than identical to it.
#[test]
fn dim_label_differs_from_body_label() {
    set_visuals(Visuals::light());
    let v = current_visuals();
    assert_ne!(
        v.text_dim, v.text_color,
        "dim and body text must be distinguishable for the hint hierarchy"
    );
}

/// An explicit `.with_color(...)` still wins over the theme — dim/auto modes
/// must not hijack a caller that asked for a specific colour.
#[test]
fn explicit_color_overrides_dim_and_theme() {
    set_visuals(Visuals::dark());
    let font = test_font();
    let forced = Color::from_rgb8(0x12, 0x34, 0x56);
    let mut label = Label::new("Forced", font)
        .with_dim(true)
        .with_color(forced);
    assert_eq!(
        painted_color(&mut label),
        forced,
        "explicit with_color must override both with_dim and the theme"
    );
}
