//! File-drop dispatch guarantees:
//!
//! 1. `Event::FileDropped` positions are translated into each widget's
//!    local space on the way down the tree (like mouse events) — the
//!    handler must not see root coordinates.
//! 2. A drop whose position lands on a widget that ignores it is
//!    re-offered to the rest of the tree (`dispatch_event_broadcast`),
//!    because native shells can't always produce an accurate drop
//!    point (winit's Windows backend discards the OLE drop position
//!    and emits no CursorMoved during the drag).

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::{App, DrawCtx, Event, EventResult, Point, Rect, Size, Widget};

/// Records the local position of any FileDropped it receives, then
/// consumes it.
struct DropCatcher {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    received: Rc<RefCell<Vec<Point>>>,
}

impl Widget for DropCatcher {
    fn type_name(&self) -> &'static str {
        "DropCatcher"
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
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, e: &Event) -> EventResult {
        if let Event::FileDropped { pos, .. } = e {
            self.received.borrow_mut().push(*pos);
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
}

/// Fixed-layout root: catcher pane on the left half, inert filler on the
/// right half. Both children hit-test by bounds (Widget default).
struct SplitRoot {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for SplitRoot {
    fn type_name(&self) -> &'static str {
        "SplitRoot"
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
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        let half = available.width / 2.0;
        self.children[0].set_bounds(Rect::new(0.0, 0.0, half, available.height));
        self.children[0].layout(Size::new(half, available.height));
        self.children[1].set_bounds(Rect::new(half, 0.0, half, available.height));
        self.children[1].layout(Size::new(half, available.height));
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}

struct Filler {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Filler {
    fn type_name(&self) -> &'static str {
        "Filler"
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
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn build_app(received: Rc<RefCell<Vec<Point>>>) -> App {
    agg_gui::set_device_scale(1.0);
    let catcher = Box::new(DropCatcher {
        bounds: Rect::default(),
        children: Vec::new(),
        received,
    });
    let filler = Box::new(Filler {
        bounds: Rect::default(),
        children: Vec::new(),
    });
    let root: Box<dyn Widget> = Box::new(SplitRoot {
        bounds: Rect::default(),
        children: vec![catcher, filler],
    });
    let mut app = App::new(root);
    app.layout(Size::new(800.0, 600.0));
    app
}

#[test]
fn drop_position_is_translated_to_widget_local_space() {
    let received = Rc::new(RefCell::new(Vec::new()));
    let mut app = build_app(received.clone());

    // Screen coords are Y-down; the catcher pane spans x in [0, 400).
    // Drop at screen (100, 500) → world (100, 100) → catcher-local
    // (100, 100) since the pane sits at the root origin.
    app.on_file_dropped(100.0, 500.0, vec![std::path::PathBuf::from("a.stl")]);

    let got = received.borrow();
    assert_eq!(got.len(), 1, "catcher must receive the drop");
    assert!(
        (got[0].x - 100.0).abs() < 1e-6 && (got[0].y - 100.0).abs() < 1e-6,
        "expected local (100, 100), got ({}, {})",
        got[0].x,
        got[0].y
    );
}

#[test]
fn unconsumed_drop_is_reoffered_to_the_rest_of_the_tree() {
    let received = Rc::new(RefCell::new(Vec::new()));
    let mut app = build_app(received.clone());

    // Drop over the right-hand filler (x=600), which ignores it. The
    // broadcast fallback must still deliver it to the catcher, with the
    // position translated into the catcher's local space (600 - 0 = 600,
    // i.e. outside its bounds — receivers clamp as needed).
    app.on_file_dropped(600.0, 300.0, vec![std::path::PathBuf::from("b.stl")]);

    let got = received.borrow();
    assert_eq!(
        got.len(),
        1,
        "drop over a non-handling widget must fall back to the catcher"
    );
}
