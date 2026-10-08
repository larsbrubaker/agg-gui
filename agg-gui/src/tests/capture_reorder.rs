//! Pointer capture, hover and focus survive a parent reordering its children.
//!
//! `App` stores those targets as child-index paths; `widget/app/path_anchor.rs`
//! anchors each path to its widgets' identities and re-resolves it after
//! layout and at every input entry point. These tests drive the real `App`
//! entry points with a strip of two named leaves that swap places mid-gesture
//! (the way a tab strip reorders the tab being dragged past a neighbour) and
//! check every later event still reaches the widget it belongs to.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::{App, DrawCtx, Event, EventResult, Key, Modifiers, MouseButton, Rect, Size, Widget};

type Log = Rc<RefCell<Vec<String>>>;

/// A focusable leaf that consumes presses, releases and moves, logging each as
/// `"<name> <kind>"`. A move to `(-1, -1)` is the hover-clear sentinel.
struct Leaf {
    name: &'static str,
    bounds: Rect,
    log: Log,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Leaf {
    /// The name doubles as the type name so `Strip` can place each leaf.
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
    fn layout(&mut self, available: Size) -> Size {
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn is_focusable(&self) -> bool {
        true
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        let kind = match event {
            Event::MouseDown { .. } => "down",
            Event::MouseUp { .. } => "up",
            Event::MouseMove { pos } if pos.x < 0.0 => "clear",
            Event::MouseMove { .. } => "move",
            Event::KeyDown { .. } => "key",
            _ => return EventResult::Ignored,
        };
        self.log.borrow_mut().push(format!("{} {kind}", self.name));
        EventResult::Consumed
    }
}

/// Two leaves side by side: "a" always occupies x 0..100 and "b" x 100..200,
/// whatever their order in `children`. When `swap` is set, the next layout
/// swaps the two children (the strip adopting a new order).
struct Strip {
    bounds: Rect,
    swap: Rc<Cell<bool>>,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Strip {
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
        if self.swap.replace(false) {
            self.children.swap(0, 1);
        }
        for child in &mut self.children {
            let x = if child.type_name() == "a" { 0.0 } else { 100.0 };
            child.set_bounds(Rect::new(x, 0.0, 100.0, available.height));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn leaf(name: &'static str, log: &Log) -> Box<dyn Widget> {
    Box::new(Leaf {
        name,
        bounds: Rect::default(),
        log: Rc::clone(log),
        children: Vec::new(),
    })
}

const SIZE: Size = Size {
    width: 200.0,
    height: 100.0,
};

fn strip_app() -> (App, Log, Rc<Cell<bool>>) {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let swap = Rc::new(Cell::new(false));
    let strip = Strip {
        bounds: Rect::default(),
        swap: Rc::clone(&swap),
        children: vec![leaf("a", &log), leaf("b", &log)],
    };
    let mut app = App::new(Box::new(strip));
    app.layout(SIZE);
    (app, log, swap)
}

fn none() -> Modifiers {
    Modifiers::default()
}

#[test]
fn capture_follows_widget_when_layout_swaps_children() {
    let (mut app, log, swap) = strip_app();
    app.on_mouse_down(50.0, 50.0, MouseButton::Left, none());
    assert!(app.has_captured_pointer());
    // The strip adopts a new order mid-drag; "a" now sits at index 1.
    swap.set(true);
    app.layout(SIZE);
    log.borrow_mut().clear();
    app.on_mouse_move(60.0, 50.0);
    app.on_mouse_move(150.0, 50.0);
    app.on_mouse_up(150.0, 50.0, MouseButton::Left, none());
    let log = log.borrow();
    // The drag's moves and its release reach "a"; "b" only sees the hover
    // refresh at the release point once capture has ended.
    assert_eq!(&log[..3], ["a move", "a move", "a up"]);
    assert_eq!(&log[3..], ["b move"]);
}

#[test]
fn capture_follows_widget_when_children_reordered_between_events() {
    let (mut app, log, _swap) = strip_app();
    app.on_mouse_down(50.0, 50.0, MouseButton::Left, none());
    // Reorder through the root directly, with no layout in between.
    app.root_mut().children_mut().swap(0, 1);
    log.borrow_mut().clear();
    app.on_mouse_move(70.0, 50.0);
    app.on_mouse_up(70.0, 50.0, MouseButton::Left, none());
    // The release lands on "a" itself, which held the capture: no hover
    // refresh move follows (agg-sharp sends no move on a release).
    assert_eq!(*log.borrow(), ["a move", "a up"]);
}

#[test]
fn hover_clear_reaches_previously_hovered_widget_after_swap() {
    let (mut app, log, swap) = strip_app();
    app.on_mouse_move(50.0, 50.0);
    swap.set(true);
    app.layout(SIZE);
    log.borrow_mut().clear();
    // Moving onto "b" clears hover on "a", wherever "a" now sits.
    app.on_mouse_move(150.0, 50.0);
    assert_eq!(*log.borrow(), ["a clear", "b move"]);
}

#[test]
fn focus_follows_widget_when_layout_swaps_children() {
    let (mut app, log, swap) = strip_app();
    app.on_mouse_down(50.0, 50.0, MouseButton::Left, none());
    app.on_mouse_up(50.0, 50.0, MouseButton::Left, none());
    swap.set(true);
    app.layout(SIZE);
    log.borrow_mut().clear();
    app.on_key_down(Key::Char('x'), none());
    assert_eq!(*log.borrow(), ["a key"]);
}

#[test]
fn rust_only_a_release_over_a_replaced_capture_holder_refreshes_the_new_widget() {
    // The widget that took the press is replaced mid-press by a new one at
    // the same index (an item view rebuilt by what the press did). The
    // release lands on the newcomer, which never saw the pointer: it gets the
    // hover refresh move, unlike a capture holder released over itself.
    // (The release itself still goes down the capture path, as it always
    // has for a holder that was dropped.)
    let (mut app, log, _swap) = strip_app();
    app.on_mouse_move(50.0, 50.0);
    app.on_mouse_down(50.0, 50.0, MouseButton::Left, none());
    let fresh = leaf("a", &log);
    app.root_mut().children_mut()[0] = fresh;
    app.layout(SIZE);
    log.borrow_mut().clear();
    app.on_mouse_up(50.0, 50.0, MouseButton::Left, none());
    assert_eq!(*log.borrow(), ["a up", "a move"]);
}
