//! `is_local_rect_in_paint_clip` is public, so widgets outside agg-gui can
//! gate self-animation on visibility the way `Spinner` and `ProgressBar` do.
//!
//! The traversal still paints children a clipping ancestor hides; a
//! self-animating widget there must not re-arm its wake-up. This drives a
//! real `paint_subtree` pass with one child inside its parent's clip and
//! one outside it and checks what each sees.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::widget::{is_local_rect_in_paint_clip, paint_subtree};
use agg_gui::{DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Rect, Size, Widget};

type Seen = Rc<RefCell<Vec<(&'static str, bool)>>>;

struct Node {
    name: &'static str,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    seen: Seen,
}

impl Widget for Node {
    fn type_name(&self) -> &'static str {
        "Node"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let (w, h) = (self.bounds.width, self.bounds.height);
        let visible = is_local_rect_in_paint_clip(ctx, 0.0, 0.0, w, h);
        self.seen.borrow_mut().push((self.name, visible));
    }
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn node(name: &'static str, bounds: Rect, children: Vec<Box<dyn Widget>>, seen: &Seen) -> Node {
    Node {
        name,
        bounds,
        children,
        seen: Rc::clone(seen),
    }
}

#[test]
fn clipped_out_child_reports_not_visible() {
    let seen: Seen = Rc::default();
    let inside = node("inside", Rect::new(10.0, 10.0, 20.0, 20.0), vec![], &seen);
    let outside = node("outside", Rect::new(150.0, 10.0, 20.0, 20.0), vec![], &seen);
    let mut root = node(
        "root",
        Rect::new(0.0, 0.0, 100.0, 100.0),
        vec![Box::new(inside), Box::new(outside)],
        &seen,
    );
    let mut fb = Framebuffer::new(200, 100);
    let mut ctx = GfxCtx::new(&mut fb);
    paint_subtree(&mut root, &mut ctx);
    assert_eq!(
        *seen.borrow(),
        vec![("root", true), ("inside", true), ("outside", false)]
    );
}
