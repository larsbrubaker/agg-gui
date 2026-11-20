//! Unit tests for the editor's app-absolute origin (`NodeEditor::app_origin`,
//! `app_to_canvas`) when it lives inside a floating `agg_gui::Window` painted
//! through retained GPU layers, as a host that pops the editor out does.
//!
//! [`GpuLayerCtx`] stands in for the GPU backend: retained layers persist by
//! key and a clean one is only composited (no widget `paint` runs), each layer
//! starts at its own origin and `root_transform` adds the enclosing layers'
//! origins back, as `agg-gui-wgpu` does. The frame is the real `App::paint`,
//! so the window's and the editor's own retained layers are both in play.

use std::collections::HashSet;
use std::sync::Arc;

use super::tests_common::{fixture_with_typed_handle, install_test_font_once};
use super::*;
use agg_gui::draw_ctx::{LinearGradientPaint, RadialGradientPaint};
use agg_gui::{
    App, Color, CompOp, FillRule, LineCap, LineJoin, Point, Rect, Stack, TextMetrics, TransAffine,
    Window,
};

/// One open layer: where it sits in its parent's device pixels and the
/// parent's transform state to restore on pop.
struct OpenLayer {
    origin: (f64, f64),
    saved: TransAffine,
    saved_stack: Vec<TransAffine>,
}

/// A draw context with retained layers, laid out as the GPU backend lays
/// them out (see the module header). It draws nothing.
#[derive(Default)]
struct GpuLayerCtx {
    transform: TransAffine,
    stack: Vec<TransAffine>,
    layers: Vec<OpenLayer>,
    retained: HashSet<u64>,
}

impl GpuLayerCtx {
    fn open_layer(&mut self) {
        let t = self.transform;
        self.layers.push(OpenLayer {
            origin: (t.tx, t.ty),
            saved: t,
            saved_stack: std::mem::take(&mut self.stack),
        });
        // The layer's own coordinates start at its origin.
        self.transform = TransAffine::new_custom(t.sx, t.shy, t.shx, t.sy, 0.0, 0.0);
    }
}

impl DrawCtx for GpuLayerCtx {
    fn set_fill_color(&mut self, _c: Color) {}
    fn set_stroke_color(&mut self, _c: Color) {}
    fn set_fill_linear_gradient(&mut self, _g: LinearGradientPaint) {}
    fn set_fill_radial_gradient(&mut self, _g: RadialGradientPaint) {}
    fn set_line_width(&mut self, _w: f64) {}
    fn set_line_join(&mut self, _join: LineJoin) {}
    fn set_line_cap(&mut self, _cap: LineCap) {}
    fn set_miter_limit(&mut self, _limit: f64) {}
    fn set_line_dash(&mut self, _dashes: &[f64], _offset: f64) {}
    fn set_blend_mode(&mut self, _mode: CompOp) {}
    fn set_global_alpha(&mut self, _alpha: f64) {}
    fn set_fill_rule(&mut self, _rule: FillRule) {}
    fn set_font(&mut self, _font: Arc<agg_gui::Font>) {}
    fn set_font_size(&mut self, _size: f64) {}
    fn clip_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn reset_clip(&mut self) {}
    fn clear(&mut self, _color: Color) {}
    fn begin_path(&mut self) {}
    fn move_to(&mut self, _x: f64, _y: f64) {}
    fn line_to(&mut self, _x: f64, _y: f64) {}
    fn cubic_to(&mut self, _a: f64, _b: f64, _c: f64, _d: f64, _e: f64, _f: f64) {}
    fn quad_to(&mut self, _cx: f64, _cy: f64, _x: f64, _y: f64) {}
    fn arc_to(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64, _ccw: bool) {}
    fn circle(&mut self, _cx: f64, _cy: f64, _r: f64) {}
    fn rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn rounded_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64, _r: f64) {}
    fn close_path(&mut self) {}
    fn fill(&mut self) {}
    fn stroke(&mut self) {}
    fn fill_and_stroke(&mut self) {}
    fn draw_triangles_aa(&mut self, _v: &[[f32; 3]], _i: &[u32], _c: Color) {}
    fn fill_text(&mut self, _text: &str, _x: f64, _y: f64) {}
    fn fill_text_gsv(&mut self, _text: &str, _x: f64, _y: f64, _size: f64) {}
    fn measure_text(&self, _text: &str) -> Option<TextMetrics> {
        None
    }
    fn transform(&self) -> TransAffine {
        self.transform
    }
    fn root_transform(&self) -> TransAffine {
        // Each origin is in its parent's device pixels: added after the
        // in-layer transform, innermost layer first.
        let mut t = self.transform;
        for layer in self.layers.iter().rev() {
            t.multiply(&TransAffine::new_translation(
                layer.origin.0,
                layer.origin.1,
            ));
        }
        t
    }
    fn set_transform(&mut self, m: TransAffine) {
        self.transform = m;
    }
    fn reset_transform(&mut self) {
        self.transform = TransAffine::new();
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
    fn supports_compositing_layers(&self) -> bool {
        true
    }
    fn supports_retained_layers(&self) -> bool {
        true
    }
    fn push_layer(&mut self, _width: f64, _height: f64) {
        self.open_layer();
    }
    fn composite_retained_layer(&mut self, key: u64, _w: f64, _h: f64, _alpha: f64) -> bool {
        self.retained.contains(&key)
    }
    fn push_retained_layer_with_alpha(&mut self, key: u64, _w: f64, _h: f64, _alpha: f64) {
        self.retained.insert(key);
        self.open_layer();
    }
    fn pop_layer(&mut self) {
        if let Some(layer) = self.layers.pop() {
            self.transform = layer.saved;
            self.stack = layer.saved_stack;
        }
    }
}

const VIEWPORT: Size = Size::new(800.0, 600.0);

/// An app whose root stack floats one window holding an editor, the window
/// placed at `at` and movable through the returned reset cell.
fn app_with_windowed_editor(at: Rect) -> (App, Rc<Cell<Option<Rect>>>) {
    install_test_font_once();
    let font = agg_gui::font_settings::current_system_font().expect("test font installed");
    let (model, _memory) = fixture_with_typed_handle();
    let editor = NodeEditor::new(model);
    let move_to = Rc::new(Cell::new(None));
    let window = Window::new("Graph", font, Box::new(editor))
        .with_bounds(at)
        .with_reset_cell(Rc::clone(&move_to));
    let mut app = App::new(Box::new(Stack::new().add(Box::new(window))));
    app.layout(VIEWPORT);
    (app, move_to)
}

/// The editor in `w`'s subtree and its bottom-left in the root's
/// coordinates, summed from the laid-out bounds (where hit testing and the
/// drop's pointer are).
fn find_editor(w: &dyn Widget, at: Point) -> Option<(&NodeEditor, Point)> {
    let b = w.bounds();
    let here = Point::new(at.x + b.x, at.y + b.y);
    if let Some(editor) = w.as_any().and_then(|a| a.downcast_ref::<NodeEditor>()) {
        return Some((editor, here));
    }
    w.children()
        .iter()
        .find_map(|c| find_editor(c.as_ref(), here))
}

fn editor_and_layout_origin(app: &App) -> (&NodeEditor, Point) {
    find_editor(app.root(), Point::new(0.0, 0.0)).expect("the window holds the editor")
}

fn frame(app: &mut App, ctx: &mut GpuLayerCtx) {
    app.layout(VIEWPORT);
    app.paint(ctx);
}

#[test]
fn a_windowed_editor_reports_its_app_origin_through_the_retained_layers() {
    let (mut app, _move_to) = app_with_windowed_editor(Rect::new(100.0, 100.0, 420.0, 300.0));
    let mut ctx = GpuLayerCtx::default();
    frame(&mut app, &mut ctx);

    let (editor, origin) = editor_and_layout_origin(&app);
    assert!(
        origin.x > 100.0 - 1e-9 && origin.y > 100.0 - 1e-9,
        "inside the window: {origin:?}"
    );
    let got = editor.app_origin();
    assert_eq!((got.x, got.y), (origin.x, origin.y));
}

#[test]
fn a_moved_window_keeps_the_editors_app_origin_without_repainting_it() {
    let (mut app, move_to) = app_with_windowed_editor(Rect::new(100.0, 100.0, 420.0, 300.0));
    let mut ctx = GpuLayerCtx::default();
    frame(&mut app, &mut ctx);
    frame(&mut app, &mut ctx);
    let repaints_before = editor_and_layout_origin(&app).0.backbuffer.repaint_count;

    // Dragging a window composites its cached layer at the new place; the
    // content is not re-rasterised.
    move_to.set(Some(Rect::new(40.0, 260.0, 420.0, 300.0)));
    frame(&mut app, &mut ctx);

    let (editor, origin) = editor_and_layout_origin(&app);
    assert_eq!(
        editor.backbuffer.repaint_count, repaints_before,
        "the move only composites the cached layers"
    );
    let got = editor.app_origin();
    assert_eq!((got.x, got.y), (origin.x, origin.y));
    // A drop at the pointer lands where the pointer is over the canvas.
    let drop = Point::new(origin.x + 25.0, origin.y + 40.0);
    assert_eq!(
        editor.app_to_canvas(drop),
        editor.screen_to_canvas(Point::new(25.0, 40.0))
    );
}
