//! Retained backbuffer invalidation tests.
//!
//! These cover the shared widget paint traversal rather than a particular
//! window implementation, so both FBO-backed windows and future retained
//! backbuffer widgets inherit the same invalidation rules.

use super::*;

use crate::draw_ctx::{FillRule, GlPaint, LinearGradientPaint};
use crate::text::{Font, TextMetrics};
use crate::widget::{paint_subtree, BackbufferKind, BackbufferSpec, BackbufferState};
use crate::{DrawCtx, Event, EventResult, Rect};
use agg_rust::comp_op::CompOp;
use agg_rust::math_stroke::{LineCap, LineJoin};
use agg_rust::trans_affine::TransAffine;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

struct RetainedProbe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    backbuffer: BackbufferState,
    paints: Rc<Cell<usize>>,
}

impl Widget for RetainedProbe {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        self.paints.set(self.paints.get() + 1);
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }

    fn backbuffer_spec(&mut self) -> BackbufferSpec {
        BackbufferSpec {
            kind: BackbufferKind::GlFbo,
            cached: true,
            alpha: 1.0,
            outsets: crate::Insets::ZERO,
            rounded_clip: None,
        }
    }

    fn backbuffer_state_mut(&mut self) -> Option<&mut BackbufferState> {
        Some(&mut self.backbuffer)
    }
}

struct RetainedLayerCtx {
    transform: TransAffine,
    stack: Vec<TransAffine>,
    has_retained_layer: bool,
}

impl RetainedLayerCtx {
    fn new() -> Self {
        Self {
            transform: TransAffine::new(),
            stack: Vec::new(),
            has_retained_layer: false,
        }
    }
}

impl DrawCtx for RetainedLayerCtx {
    fn set_fill_color(&mut self, _color: Color) {}
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
    fn arc_to(
        &mut self,
        _cx: f64,
        _cy: f64,
        _r: f64,
        _start_angle: f64,
        _end_angle: f64,
        _ccw: bool,
    ) {
    }
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
        None
    }
    fn transform(&self) -> TransAffine {
        self.transform
    }
    fn save(&mut self) {
        self.stack.push(self.transform);
    }
    fn restore(&mut self) {
        if let Some(transform) = self.stack.pop() {
            self.transform = transform;
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
    fn supports_compositing_layers(&self) -> bool {
        true
    }
    fn supports_retained_layers(&self) -> bool {
        true
    }
    fn composite_retained_layer(
        &mut self,
        _key: u64,
        _width: f64,
        _height: f64,
        _alpha: f64,
    ) -> bool {
        self.has_retained_layer
    }
    fn push_retained_layer_with_alpha(
        &mut self,
        _key: u64,
        _width: f64,
        _height: f64,
        _alpha: f64,
    ) {
        self.has_retained_layer = true;
    }
    fn pop_layer(&mut self) {}
    fn gl_paint(&mut self, _screen_rect: Rect, _painter: &mut dyn GlPaint) {}
}

#[test]
fn test_theme_change_invalidates_retained_widget_layer() {
    let paints = Rc::new(Cell::new(0));
    let mut widget = RetainedProbe {
        bounds: Rect::new(0.0, 0.0, 20.0, 20.0),
        children: Vec::new(),
        backbuffer: BackbufferState::new(),
        paints: Rc::clone(&paints),
    };
    let mut ctx = RetainedLayerCtx::new();

    paint_subtree(&mut widget, &mut ctx);
    paint_subtree(&mut widget, &mut ctx);
    assert_eq!(paints.get(), 1, "clean retained layer should be reused");

    crate::set_visuals(crate::Visuals::light());
    paint_subtree(&mut widget, &mut ctx);

    assert_eq!(
        paints.get(),
        2,
        "theme changes must repaint retained widget layers from the library path"
    );

    // Restore the common default for later tests in this process.
    crate::set_visuals(crate::Visuals::dark());
}

#[test]
fn test_idle_retained_window_does_not_request_reactive_redraw() {
    use crate::widget::paint_subtree;
    use crate::widgets::window::Window;
    use crate::widgets::ToggleSwitch;

    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
    let toggle = ToggleSwitch::new(false);
    let mut window = Window::new("Idle", Arc::clone(&font), Box::new(toggle))
        .with_bounds(Rect::new(0.0, 0.0, 120.0, 80.0));
    window.layout(Size::new(200.0, 120.0));

    let mut ctx = RetainedLayerCtx::new();
    for _ in 0..2 {
        crate::animation::clear_draw_request();
        paint_subtree(&mut window, &mut ctx);
    }

    assert!(
        !crate::animation::wants_draw() && !window.needs_draw(),
        "an idle retained window must let the reactive host go idle"
    );
}
