//! `event_rect_to_root` gives a widget its root-space placement while it
//! handles an event.
//!
//! A button that opens a popup anchored to itself needs its bounds in the
//! App root's space at click time; `bounds()` is parent-local, so before
//! this API widgets recorded their root origin during paint. These tests
//! drive real `App` input through containers with offsets and a scaling
//! `child_transform`, through a nested dispatch into an owned sub-root, and
//! through pointer enter delivery, and check the reported rect each time.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::widget::dispatch_event_dyn;
use agg_gui::{
    event_rect_to_root, App, DrawCtx, Event, EventResult, Modifiers, MouseButton, Point, Rect,
    Size, TransAffine, Widget,
};

type Log = Rc<RefCell<Vec<(&'static str, Option<Rect>)>>>;

/// Leaf that logs `event_rect_to_root` of its own bounds for every event.
struct Probe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    log: Log,
}

impl Widget for Probe {
    fn type_name(&self) -> &'static str {
        "Probe"
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
        let kind = match e {
            Event::MouseDown { .. } => "down",
            Event::MouseEnter => "enter",
            _ => return EventResult::Ignored,
        };
        let own = Rect::new(0.0, 0.0, self.bounds.width, self.bounds.height);
        self.log.borrow_mut().push((kind, event_rect_to_root(own)));
        EventResult::Consumed
    }
}

/// Container that keeps its children where they were placed, optionally
/// scaling them through `child_transform` (a pan/zoom canvas).
struct Frame {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    child_scale: f64,
}

impl Widget for Frame {
    fn type_name(&self) -> &'static str {
        "Frame"
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
    fn child_transform(&self) -> Option<TransAffine> {
        (self.child_scale != 1.0).then(|| {
            let mut t = TransAffine::new();
            t.scale_uniform(self.child_scale);
            t
        })
    }
}

/// Widget owning a sub-tree outside `children()` (as `Window` owns its
/// title bar) and routing presses into it with a nested dispatch.
struct Owner {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    bar: Box<dyn Widget>,
}

impl Widget for Owner {
    fn type_name(&self) -> &'static str {
        "Owner"
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
        if let Event::MouseDown { pos, .. } = e {
            let b = self.bar.bounds();
            let local = Point::new(pos.x - b.x, pos.y - b.y);
            return dispatch_event_dyn(self.bar.as_mut(), &[0], e, local);
        }
        EventResult::Ignored
    }
}

fn probe(bounds: Rect, log: &Log) -> Box<dyn Widget> {
    Box::new(Probe {
        bounds,
        children: Vec::new(),
        log: Rc::clone(log),
    })
}

fn frame(bounds: Rect, child_scale: f64, children: Vec<Box<dyn Widget>>) -> Box<dyn Widget> {
    Box::new(Frame {
        bounds,
        children,
        child_scale,
    })
}

/// 800x600 root; `panel` at (100, 50) scales its children by 2; the probe
/// sits at (10, 20) inside it, so its 30x10 bounds cover (120, 90)-(180, 110)
/// in root space.
fn scaled_app(log: &Log) -> App {
    let panel = frame(
        Rect::new(100.0, 50.0, 400.0, 300.0),
        2.0,
        vec![probe(Rect::new(10.0, 20.0, 30.0, 10.0), log)],
    );
    let root = frame(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0, vec![panel]);
    let mut app = App::new(root);
    app.layout(Size::new(800.0, 600.0));
    app
}

#[test]
fn click_reports_root_rect_through_offsets_and_child_transform() {
    let log: Log = Rc::default();
    let mut app = scaled_app(&log);
    // Root (150, 100) Y-up is screen y = 600 - 100 (Y-down).
    app.on_mouse_down(150.0, 500.0, MouseButton::Left, Modifiers::default());
    let downs: Vec<_> = log
        .borrow()
        .iter()
        .filter(|e| e.0 == "down")
        .cloned()
        .collect();
    assert_eq!(
        downs,
        vec![("down", Some(Rect::new(120.0, 90.0, 60.0, 20.0)))]
    );
}

#[test]
fn pointer_enter_reports_root_rect() {
    let log: Log = Rc::default();
    let mut app = scaled_app(&log);
    app.on_mouse_move(150.0, 500.0);
    let enters: Vec<_> = log
        .borrow()
        .iter()
        .filter(|e| e.0 == "enter")
        .cloned()
        .collect();
    assert_eq!(
        enters,
        vec![("enter", Some(Rect::new(120.0, 90.0, 60.0, 20.0)))]
    );
}

#[test]
fn nested_dispatch_places_sub_root_at_its_bounds_in_the_caller() {
    let log: Log = Rc::default();
    let bar = frame(
        Rect::new(5.0, 6.0, 50.0, 20.0),
        1.0,
        vec![probe(Rect::new(1.0, 2.0, 10.0, 10.0), &log)],
    );
    let owner = Box::new(Owner {
        bounds: Rect::new(200.0, 100.0, 100.0, 50.0),
        children: Vec::new(),
        bar,
    });
    let root = frame(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0, vec![owner]);
    let mut app = App::new(root);
    app.layout(Size::new(800.0, 600.0));
    // Root (210, 112): owner-local (10, 12), bar-local (5, 6), probe-local (4, 4).
    app.on_mouse_down(
        210.0,
        600.0 - 112.0,
        MouseButton::Left,
        Modifiers::default(),
    );
    let downs: Vec<_> = log
        .borrow()
        .iter()
        .filter(|e| e.0 == "down")
        .cloned()
        .collect();
    assert_eq!(
        downs,
        vec![("down", Some(Rect::new(206.0, 108.0, 10.0, 10.0)))]
    );
}

#[test]
fn outside_dispatch_there_is_no_event_root() {
    let log: Log = Rc::default();
    let mut app = scaled_app(&log);
    app.on_mouse_down(150.0, 500.0, MouseButton::Left, Modifiers::default());
    // The stack unwinds once dispatch returns.
    assert_eq!(event_rect_to_root(Rect::new(0.0, 0.0, 1.0, 1.0)), None);
    assert_eq!(agg_gui::event_root_transform(), None);
}
