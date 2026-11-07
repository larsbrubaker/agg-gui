//! `previewed_child` tells a previewing ancestor which of its children a
//! routed event goes into next, as the App's own dispatch chose it.
//!
//! The root here hosts main content (child 0) and two layers over it: a
//! panel that hits only in its own rectangle (child 1) and a layer that turns
//! modal on demand (child 2). Presses go through the real `App` input path;
//! the root and the content record what `previewed_child` answers in their
//! previews, and the leaf (the target, which has no preview of its own in the
//! dispatch) and code outside any dispatch see `None`. Direct dispatches cover
//! a dispatch nested inside a preview (the outer answer comes back, also after
//! a panic) and a stale path naming a child that is gone.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui::widget::{dispatch_event_dyn, previewed_child};
use agg_gui::{
    App, DrawCtx, Event, EventResult, Modifiers, MouseButton, Point, Rect, Size, Widget,
};

type Log = Rc<RefCell<Vec<(&'static str, Option<usize>)>>>;

/// A container or leaf that logs `previewed_child` in its preview (and, as a
/// target, in `on_event`), hit only inside `hit` when given.
struct Node {
    name: &'static str,
    bounds: Rect,
    hit: Option<Rect>,
    modal: Option<Rc<Cell<bool>>>,
    panics: bool,
    children: Vec<Box<dyn Widget>>,
    log: Log,
}

impl Node {
    fn new(name: &'static str, bounds: Rect, log: &Log) -> Self {
        Self {
            name,
            bounds,
            hit: None,
            modal: None,
            panics: false,
            children: Vec::new(),
            log: Rc::clone(log),
        }
    }
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
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn hit_test(&self, pos: Point) -> bool {
        let r = self
            .hit
            .unwrap_or(Rect::new(0.0, 0.0, self.bounds.width, self.bounds.height));
        pos.x >= r.x && pos.y >= r.y && pos.x <= r.x + r.width && pos.y <= r.y + r.height
    }
    fn has_active_modal(&self) -> bool {
        self.modal.as_ref().is_some_and(|m| m.get())
    }
    fn preview_event(&mut self, e: &Event) -> EventResult {
        if matches!(e, Event::MouseDown { .. }) {
            self.log.borrow_mut().push((self.name, previewed_child()));
            if self.panics {
                panic!("{} panics in its preview", self.name);
            }
        }
        EventResult::Ignored
    }
    fn on_event(&mut self, e: &Event) -> EventResult {
        if matches!(e, Event::MouseDown { .. }) {
            self.log.borrow_mut().push(("target", previewed_child()));
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
}

/// The App over root [content [leaf, leaf], panel, modal layer], 800x600.
fn app(log: &Log, modal: &Rc<Cell<bool>>) -> App {
    let full = Rect::new(0.0, 0.0, 800.0, 600.0);
    let mut content = Node::new("content", full, log);
    content.children.push(Box::new(Node::new(
        "left",
        Rect::new(0.0, 0.0, 400.0, 600.0),
        log,
    )));
    content.children.push(Box::new(Node::new(
        "right",
        Rect::new(400.0, 0.0, 400.0, 600.0),
        log,
    )));
    let mut panel = Node::new("panel", full, log);
    // The panel takes presses only in its middle (a dialog on a clear layer).
    panel.hit = Some(Rect::new(300.0, 200.0, 200.0, 200.0));
    let mut layer = Node::new("layer", full, log);
    layer.hit = Some(Rect::new(0.0, 0.0, 0.0, 0.0));
    layer.modal = Some(Rc::clone(modal));
    let mut root = Node::new("root", full, log);
    root.children = vec![Box::new(content), Box::new(panel), Box::new(layer)];
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(800.0, 600.0));
    app
}

/// A press at (`x`, `y`), Y-down window pixels, and what was logged.
fn press(app: &mut App, log: &Log, x: f64, y: f64) -> Vec<(&'static str, Option<usize>)> {
    log.borrow_mut().clear();
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
    let logged = log.borrow().clone();
    logged
}

#[test]
fn a_press_on_the_content_names_it_at_each_level() {
    let log = Log::default();
    let modal = Rc::new(Cell::new(false));
    let mut app = app(&log, &modal);
    assert_eq!(
        press(&mut app, &log, 600.0, 50.0),
        [("root", Some(0)), ("content", Some(1)), ("target", None)]
    );
    assert_eq!(previewed_child(), None, "nothing is previewing now");
}

#[test]
fn a_press_on_a_layer_over_the_content_names_the_layer() {
    let log = Log::default();
    let modal = Rc::new(Cell::new(false));
    let mut app = app(&log, &modal);
    // The panel's middle: the panel is the target, so only the root previews.
    assert_eq!(
        press(&mut app, &log, 400.0, 300.0),
        [("root", Some(1)), ("target", None)]
    );
}

fn mouse_down() -> Event {
    Event::MouseDown {
        pos: Point::new(10.0, 10.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    }
}

/// A container whose preview routes the event into a sub-tree it owns outside `children()` (as
/// a window routes into its title bar), logging `previewed_child` before and after; the
/// sub-tree's own preview panics when `owned.panics` is set, and the container catches it.
struct Nester {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    owned: Node,
    log: Log,
}

impl Widget for Nester {
    fn type_name(&self) -> &'static str {
        "Nester"
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
    fn preview_event(&mut self, e: &Event) -> EventResult {
        self.log
            .borrow_mut()
            .push(("nester before", previewed_child()));
        let owned = &mut self.owned;
        let nested = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            dispatch_event_dyn(owned, &[0], e, Point::new(10.0, 10.0))
        }));
        if nested.is_err() {
            self.log.borrow_mut().push(("caught", None));
        }
        self.log
            .borrow_mut()
            .push(("nester after", previewed_child()));
        EventResult::Ignored
    }
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// A Nester (owning [sub-root [leaf]]) whose own child 1 the outer dispatch enters.
fn nester(log: &Log, inner_panics: bool) -> Nester {
    let full = Rect::new(0.0, 0.0, 100.0, 100.0);
    let mut owned = Node::new("sub-root", full, log);
    owned.panics = inner_panics;
    owned
        .children
        .push(Box::new(Node::new("sub-leaf", full, log)));
    Nester {
        bounds: full,
        children: vec![
            Box::new(Node::new("first", full, log)),
            Box::new(Node::new("second", full, log)),
        ],
        owned,
        log: Rc::clone(log),
    }
}

#[test]
fn a_dispatch_nested_in_a_preview_restores_the_outer_answer() {
    let log = Log::default();
    let mut root = nester(&log, false);
    dispatch_event_dyn(&mut root, &[1], &mouse_down(), Point::new(10.0, 10.0));
    assert_eq!(
        *log.borrow(),
        [
            ("nester before", Some(1)),
            ("sub-root", Some(0)),
            // The nested target's on_event is no preview: None, not the outer 1.
            ("target", None),
            ("nester after", Some(1)),
            ("target", None),
        ]
    );
    assert_eq!(previewed_child(), None);
}

#[test]
fn a_preview_that_panics_still_restores_the_outer_answer() {
    let log = Log::default();
    let mut root = nester(&log, true);
    dispatch_event_dyn(&mut root, &[1], &mouse_down(), Point::new(10.0, 10.0));
    assert_eq!(
        *log.borrow(),
        [
            ("nester before", Some(1)),
            ("sub-root", Some(0)),
            ("caught", None),
            ("nester after", Some(1)),
            ("target", None),
        ]
    );
    // And a panic unwinding out of the outermost dispatch leaves nothing behind.
    let mut lone = Node::new("lone", Rect::new(0.0, 0.0, 100.0, 100.0), &log);
    lone.panics = true;
    lone.children.push(Box::new(Node::new(
        "leaf",
        Rect::new(0.0, 0.0, 100.0, 100.0),
        &log,
    )));
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dispatch_event_dyn(&mut lone, &[0], &mouse_down(), Point::new(10.0, 10.0))
    }));
    assert!(unwound.is_err());
    assert_eq!(previewed_child(), None);
}

#[test]
fn a_stale_path_names_no_child() {
    let log = Log::default();
    let full = Rect::new(0.0, 0.0, 100.0, 100.0);
    let mut root = Node::new("root", full, &log);
    root.children.push(Box::new(Node::new("only", full, &log)));
    // A path captured before child 3 went away: the dispatch stops at the root.
    dispatch_event_dyn(&mut root, &[3, 0], &mouse_down(), Point::new(10.0, 10.0));
    assert_eq!(*log.borrow(), [("root", None), ("target", None)]);
}

#[test]
fn an_active_modal_takes_the_press_wherever_it_lands() {
    let log = Log::default();
    let modal = Rc::new(Cell::new(true));
    let mut app = app(&log, &modal);
    assert_eq!(
        press(&mut app, &log, 600.0, 50.0),
        [("root", Some(2)), ("target", None)]
    );
}
