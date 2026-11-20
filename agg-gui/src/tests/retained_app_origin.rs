//! Where a widget inside a floating, retained `Window` learns its
//! app-absolute position, the contract `logical_root_transform` and
//! `Widget::paint_global_overlay` document: a clean retained layer is only
//! composited (its widgets' `paint` does not run), even after the window
//! moved, while the global-overlay pass visits every widget each painted
//! frame on the App's own context, so a position taken there follows the
//! window. Uses the retained-layer context from `retained_layers`.

use super::retained_layers::RetainedLayerCtx;
use crate::widget::logical_root_transform;
use crate::widgets::window::Window;
use crate::{DrawCtx, Event, EventResult, Point, Rect, Size, Stack, Widget};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// Records how often it paints and its root-logical origin as of the last
/// global-overlay pass.
struct OriginProbe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    paints: Rc<Cell<usize>>,
    origin: Rc<Cell<Option<Point>>>,
}

impl Widget for OriginProbe {
    fn type_name(&self) -> &'static str {
        "OriginProbe"
    }
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
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        self.paints.set(self.paints.get() + 1);
    }
    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let (mut x, mut y) = (0.0, 0.0);
        logical_root_transform(ctx).transform(&mut x, &mut y);
        self.origin.set(Some(Point::new(x, y)));
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// The probe's bottom-left in root coordinates, summed from the laid-out
/// bounds (where hit testing puts it).
fn layout_origin(w: &dyn Widget, at: Point) -> Option<Point> {
    let b = w.bounds();
    let here = Point::new(at.x + b.x, at.y + b.y);
    if w.type_name() == "OriginProbe" {
        return Some(here);
    }
    w.children()
        .iter()
        .find_map(|c| layout_origin(c.as_ref(), here))
}

#[test]
fn global_overlay_pass_follows_a_moved_retained_window() {
    let font = Arc::new(crate::text::Font::from_slice(super::TEST_FONT).expect("test font"));
    let paints = Rc::new(Cell::new(0));
    let origin = Rc::new(Cell::new(None));
    let probe = OriginProbe {
        bounds: Rect::default(),
        children: Vec::new(),
        paints: Rc::clone(&paints),
        origin: Rc::clone(&origin),
    };
    let move_to = Rc::new(Cell::new(None));
    let window = Window::new("Probe", font, Box::new(probe))
        .with_bounds(Rect::new(100.0, 100.0, 240.0, 160.0))
        .with_reset_cell(Rc::clone(&move_to));
    let mut app = crate::App::new(Box::new(Stack::new().add(Box::new(window))));
    let viewport = Size::new(800.0, 600.0);
    let mut ctx = RetainedLayerCtx::new();
    for _ in 0..2 {
        app.layout(viewport);
        app.paint(&mut ctx);
    }
    let before = layout_origin(app.root(), Point::new(0.0, 0.0)).expect("probe in the tree");
    assert_eq!(origin.get(), Some(before));
    let paints_before = paints.get();

    move_to.set(Some(Rect::new(40.0, 260.0, 240.0, 160.0)));
    app.layout(viewport);
    app.paint(&mut ctx);

    let after = layout_origin(app.root(), Point::new(0.0, 0.0)).expect("probe in the tree");
    assert_eq!((after.x - before.x, after.y - before.y), (-60.0, 160.0));
    assert_eq!(
        paints.get(),
        paints_before,
        "moving a clean retained window only composites its layer"
    );
    assert_eq!(origin.get(), Some(after));
}
