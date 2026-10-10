//! `Widget::preview_event`: routed dispatch offers an event to every ancestor
//! of its target, root first, before the target's `on_event` and the bubble.
//!
//! The nodes here log every preview and delivery; the tests dispatch through
//! the real `tree::dispatch_event` (and the `App` press path) and check the
//! order, the local coordinates each ancestor sees, and that a consuming
//! preview intercepts the event.

use std::cell::RefCell;
use std::rc::Rc;

use crate::widget::dispatch_event;
use crate::{App, DrawCtx, Event, EventResult, Modifiers, MouseButton, Point, Rect, Size, Widget};

type Log = Rc<RefCell<Vec<String>>>;

/// A node that logs `"<name> preview (x,y)"` and `"<name> event"`, consuming
/// in preview or in `on_event` when told to.
struct Node {
    name: &'static str,
    bounds: Rect,
    log: Log,
    consume_preview: bool,
    consume_event: bool,
    children: Vec<Box<dyn Widget>>,
}

impl Node {
    fn new(name: &'static str, bounds: Rect, log: &Log) -> Self {
        Self {
            name,
            bounds,
            log: log.clone(),
            consume_preview: false,
            consume_event: false,
            children: Vec::new(),
        }
    }
    fn with_child(mut self, child: Node) -> Self {
        self.children.push(Box::new(child));
        self
    }
}

fn pos_of(event: &Event) -> String {
    match event {
        Event::MouseDown { pos, .. } | Event::MouseUp { pos, .. } | Event::MouseMove { pos } => {
            format!("({},{})", pos.x, pos.y)
        }
        _ => String::new(),
    }
}

impl Widget for Node {
    fn type_name(&self) -> &'static str {
        self.name
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
    fn preview_event(&mut self, event: &Event) -> EventResult {
        self.log
            .borrow_mut()
            .push(format!("{} preview {}", self.name, pos_of(event)));
        if self.consume_preview {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        self.log
            .borrow_mut()
            .push(format!("{} event {}", self.name, pos_of(event)));
        if self.consume_event {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }
}

fn down(x: f64, y: f64) -> Event {
    Event::MouseDown {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    }
}

/// root (0,0 200x200) > mid (at 10,20) > leaf (at 5,5).
fn tree(log: &Log, leaf_consumes: bool) -> Box<dyn Widget> {
    let mut leaf = Node::new("leaf", Rect::new(5.0, 5.0, 50.0, 50.0), log);
    leaf.consume_event = leaf_consumes;
    let mid = Node::new("mid", Rect::new(10.0, 20.0, 100.0, 100.0), log).with_child(leaf);
    Box::new(Node::new("root", Rect::new(0.0, 0.0, 200.0, 200.0), log).with_child(mid))
}

#[test]
fn preview_runs_root_first_on_ancestors_then_target_bubbles() {
    let log: Log = Rc::default();
    let mut root = tree(&log, false);
    let r = dispatch_event(
        &mut root,
        &[0, 0],
        &down(30.0, 40.0),
        Point::new(30.0, 40.0),
    );
    assert_eq!(r, EventResult::Ignored);
    assert_eq!(
        *log.borrow(),
        vec![
            "root preview (30,40)",
            "mid preview (20,20)",
            "leaf event (15,15)",
            "mid event (20,20)",
            "root event (30,40)",
        ]
    );
}

#[test]
fn preview_reaches_ancestors_even_when_target_consumes() {
    let log: Log = Rc::default();
    let mut root = tree(&log, true);
    let r = dispatch_event(
        &mut root,
        &[0, 0],
        &down(30.0, 40.0),
        Point::new(30.0, 40.0),
    );
    assert!(r.is_consumed());
    assert_eq!(
        *log.borrow(),
        vec![
            "root preview (30,40)",
            "mid preview (20,20)",
            "leaf event (15,15)",
        ]
    );
}

#[test]
fn consuming_preview_intercepts_the_event() {
    let log: Log = Rc::default();
    let mut leaf = Node::new("leaf", Rect::new(5.0, 5.0, 50.0, 50.0), &log);
    leaf.consume_event = true;
    let mut mid = Node::new("mid", Rect::new(10.0, 20.0, 100.0, 100.0), &log).with_child(leaf);
    mid.consume_preview = true;
    let mut root: Box<dyn Widget> =
        Box::new(Node::new("root", Rect::new(0.0, 0.0, 200.0, 200.0), &log).with_child(mid));
    let r = dispatch_event(
        &mut root,
        &[0, 0],
        &down(30.0, 40.0),
        Point::new(30.0, 40.0),
    );
    assert!(r.is_consumed());
    assert_eq!(
        *log.borrow(),
        vec!["root preview (30,40)", "mid preview (20,20)"]
    );
}

#[test]
fn target_does_not_preview_its_own_event() {
    let log: Log = Rc::default();
    let mut root = tree(&log, false);
    dispatch_event(&mut root, &[], &down(30.0, 40.0), Point::new(30.0, 40.0));
    assert_eq!(*log.borrow(), vec!["root event (30,40)"]);
}

#[test]
fn app_press_previews_ancestors_of_the_hit_widget() {
    let log: Log = Rc::default();
    let mut app = App::new(tree(&log, true));
    app.layout(Size::new(200.0, 200.0));
    log.borrow_mut().clear();
    // App input is Y-down; the root is 200 tall, so y 160 down = 40 up.
    app.on_mouse_down(30.0, 160.0, MouseButton::Left, Modifiers::default());
    // Only the pointer events (focus bookkeeping carries no position).
    let got: Vec<String> = log
        .borrow()
        .iter()
        .filter(|e| e.ends_with(')'))
        .cloned()
        .collect();
    assert_eq!(
        got,
        vec![
            "root preview (30,40)",
            "mid preview (20,20)",
            "leaf event (15,15)",
        ]
    );
}
