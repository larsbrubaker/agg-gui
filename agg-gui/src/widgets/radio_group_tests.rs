//! Tests for [`RadioGroup`](super::RadioGroup): wrapped and vertical
//! layout, hit-testing the dots, and painting the selected one. Split out of
//! `radio_group.rs` (800-line guardrail).

use super::*;
use crate::color::Color;
use crate::draw_ctx::{FillRule, GlPaint, LinearGradientPaint, RadialGradientPaint};
use crate::event::Modifiers;
use crate::geometry::Point;
use crate::text::TextMetrics;
use crate::theme::{current_visuals, set_visuals, Visuals};
use agg_rust::comp_op::CompOp;
use agg_rust::math_stroke::{LineCap, LineJoin};
use agg_rust::trans_affine::TransAffine;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn test_font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Records the fill colour of every filled *circle* so a test can assert
/// how many radio dots are painted with the accent (selected) surface.
/// A circle is "filled" when `fill()` is called while the most recent
/// path primitive was `circle()`; we snapshot the current fill colour at
/// that moment. RadioGroup draws exactly one accent-filled outer circle —
/// the selected dot — plus per-dot strokes and a `widget_bg` inner dot.
struct CircleFillRecorder {
    transform: TransAffine,
    stack: Vec<TransAffine>,
    fill_color: Color,
    last_was_circle: bool,
    filled_circles: Vec<Color>,
}

impl CircleFillRecorder {
    fn new() -> Self {
        Self {
            transform: TransAffine::new(),
            stack: Vec::new(),
            fill_color: Color::rgba(0.0, 0.0, 0.0, 0.0),
            last_was_circle: false,
            filled_circles: Vec::new(),
        }
    }
}

impl DrawCtx for CircleFillRecorder {
    fn set_fill_color(&mut self, color: Color) {
        self.fill_color = color;
    }
    fn set_stroke_color(&mut self, _color: Color) {}
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
        self.last_was_circle = false;
    }
    fn move_to(&mut self, _x: f64, _y: f64) {
        self.last_was_circle = false;
    }
    fn line_to(&mut self, _x: f64, _y: f64) {
        self.last_was_circle = false;
    }
    fn cubic_to(&mut self, _cx1: f64, _cy1: f64, _cx2: f64, _cy2: f64, _x: f64, _y: f64) {
        self.last_was_circle = false;
    }
    fn quad_to(&mut self, _cx: f64, _cy: f64, _x: f64, _y: f64) {
        self.last_was_circle = false;
    }
    fn arc_to(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64, _ccw: bool) {
        self.last_was_circle = false;
    }
    fn circle(&mut self, _cx: f64, _cy: f64, _r: f64) {
        self.last_was_circle = true;
    }
    fn rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {
        self.last_was_circle = false;
    }
    fn rounded_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64, _r: f64) {
        self.last_was_circle = false;
    }
    fn close_path(&mut self) {}
    fn fill(&mut self) {
        if self.last_was_circle {
            self.filled_circles.push(self.fill_color);
        }
    }
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

/// Paint the group and count how many outer dots are filled with the
/// theme accent (i.e. painted as "selected").
fn accent_dot_count(g: &mut RadioGroup) -> usize {
    let v = current_visuals();
    let mut ctx = CircleFillRecorder::new();
    g.paint(&mut ctx);
    ctx.filled_circles
        .iter()
        .filter(|c| **c == v.accent)
        .count()
}

/// Regression: exactly ONE radio dot may be painted with the accent
/// (selected) surface — the currently-selected option. This pins the
/// "every option looks selected" report: neither hover state nor the
/// horizontal-wrap layout may leak the accent fill onto other dots.
#[test]
fn exactly_one_dot_painted_selected() {
    set_visuals(Visuals::dark());
    for sel in 0..3 {
        let mut g = RadioGroup::new(vec!["First", "Second", "Third"], sel, test_font());
        g.layout(Size::new(200.0, 0.0));
        assert_eq!(
            accent_dot_count(&mut g),
            1,
            "vertical group with selected={sel} must paint exactly one accent dot"
        );
    }
}

/// The same invariant must hold in horizontal-wrap mode, and hovering a
/// *different* option must not add a second accent dot.
#[test]
fn hover_does_not_add_a_second_selected_dot() {
    set_visuals(Visuals::dark());
    let mut g = RadioGroup::new(vec!["First", "Second", "Third"], 0, test_font())
        .with_horizontal_wrap(true);
    g.layout(Size::new(400.0, 0.0));
    // Hover the second item's centre.
    let (cx, cy, _, _, _) = g.hwrap_items[1];
    let _ = g.on_event(&Event::MouseMove {
        pos: Point::new(cx, cy),
    });
    assert_eq!(
        accent_dot_count(&mut g),
        1,
        "hovering an unselected option must not paint it as selected"
    );
}

fn empty_group(n: usize) -> RadioGroup {
    let opts: Vec<String> = (0..n).map(|_| String::new()).collect();
    RadioGroup::new(opts, 0, test_font()).with_horizontal_wrap(true)
}

#[test]
fn empty_labels_wrap_onto_multiple_rows() {
    let mut g = empty_group(64);
    // Narrow width forces many rows; a single dot is ~16px + spacing.
    let size = g.layout(Size::new(80.0, 0.0));
    assert!(size.height > ROW_H, "64 dots at 80px wide must wrap");
    assert_eq!(g.hwrap_items.len(), 64);
    // Row count is height / ROW_H; must be > 1 for a wrapped layout.
    let rows = (size.height / ROW_H).round() as usize;
    assert!(rows > 1, "expected multiple rows, got {rows}");
}

/// The row height follows the input profile's touch-target minimum, a
/// process-wide setting the keyboard tests change: hold the lock and
/// lay out as on the desktop.
fn desktop_profile() -> std::sync::MutexGuard<'static, ()> {
    let lock = crate::input_profile::profile_test_lock();
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
    lock
}

#[test]
fn single_wide_row_does_not_wrap() {
    let _profile = desktop_profile();
    let mut g = empty_group(4);
    let size = g.layout(Size::new(2000.0, 0.0));
    assert_eq!(
        (size.height / ROW_H).round() as usize,
        1,
        "4 dots in 2000px must stay on one row"
    );
}

#[test]
fn click_selects_the_hit_dot() {
    let mut g = empty_group(8);
    g.layout(Size::new(2000.0, 0.0)); // one row, all dots side by side
                                      // Aim at the 3rd dot's centre.
    let (cx, cy, _, _, _) = g.hwrap_items[2];
    let down = Event::MouseDown {
        pos: Point::new(cx, cy),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    };
    let r = g.on_event(&down);
    assert_eq!(r, EventResult::Consumed);
    assert_eq!(g.selected(), 2);
}

#[test]
fn vertical_mode_is_unaffected() {
    let _profile = desktop_profile();
    let mut g = RadioGroup::new(vec!["a", "b", "c"], 0, test_font());
    let size = g.layout(Size::new(200.0, 0.0));
    assert_eq!(size.height, 3.0 * ROW_H);
    assert!(g.hwrap_items.is_empty());
}
