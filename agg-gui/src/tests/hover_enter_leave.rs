//! `Event::MouseEnter` / `Event::MouseLeave` along the hovered chain, through
//! the real [`App`] pointer entry points.
//!
//! `widget/app/hover_chain.rs` sends enter/leave to every widget that joins or
//! drops off the hovered chain (agg-sharp's `MouseEnterBounds` /
//! `MouseLeaveBounds`), before the `MouseMove` is dispatched, so a composite
//! learns the pointer arrived even when a child handles the moves. These
//! tests cover a wrapper around a real `TextField`, gain/lose sets in nested
//! chains, pointer capture (agg-sharp's `OnMouseMoveWhenCaptured`), and a
//! hovered widget that is dropped from the tree.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::text::Font;
use crate::{
    App, DrawCtx, Event, EventResult, Modifiers, MouseButton, Rect, Size, TextField, Widget,
};

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
const VIEW: Size = Size {
    width: 400.0,
    height: 100.0,
};

type Log = Rc<RefCell<Vec<String>>>;

/// A widget that places itself at `rect` in its parent and logs pointer
/// enter/leave as `"<name> enter"` / `"<name> leave"`. A leaf (`consumes`)
/// consumes moves and presses; an inner node ignores them, so they bubble
/// (logged as `"bubbled move"`). Every node logs the hover-clear sentinel
/// (`MouseMove` at `(-1, -1)`, sent to the previously hovered path) as
/// `"clear"`.
struct Node {
    name: &'static str,
    rect: Rect,
    bounds: Rect,
    consumes: bool,
    log: Log,
    children: Vec<Box<dyn Widget>>,
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
    fn layout(&mut self, available: Size) -> Size {
        for child in &mut self.children {
            // Children are `Node`s placed at their own rect, or a single
            // `TextField` filling this node.
            let rect = child_rect(child.as_ref(), self.bounds);
            child.set_bounds(rect);
            child.layout(Size::new(rect.width, rect.height));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    /// Exposes `rect` so a parent `Node` can place it without downcasting.
    fn properties(&self) -> Vec<(&'static str, String)> {
        let r = self.rect;
        vec![("rect", format!("{},{},{},{}", r.x, r.y, r.width, r.height))]
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        let kind = match event {
            Event::MouseEnter => "enter",
            Event::MouseLeave => "leave",
            Event::MouseMove { pos } if pos.x < 0.0 => "clear",
            Event::MouseMove { .. } if self.consumes => "move",
            Event::MouseMove { .. } => "bubbled move",
            Event::MouseDown { .. } if self.consumes => "down",
            Event::MouseUp { .. } if self.consumes => "up",
            _ => return EventResult::Ignored,
        };
        self.log.borrow_mut().push(format!("{} {kind}", self.name));
        if self.consumes || matches!(event, Event::MouseEnter | Event::MouseLeave) {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }
}

/// Where `child` sits inside a parent with `parent_bounds`: a `Node` at the
/// rect it was built with, anything else filling the parent.
fn child_rect(child: &dyn Widget, parent_bounds: Rect) -> Rect {
    match child.properties().iter().find(|(k, _)| *k == "rect") {
        Some((_, v)) => {
            let n: Vec<f64> = v.split(',').map(|s| s.parse().unwrap_or(0.0)).collect();
            Rect::new(n[0], n[1], n[2], n[3])
        }
        None => Rect::new(0.0, 0.0, parent_bounds.width, parent_bounds.height),
    }
}

impl Node {
    fn boxed(
        name: &'static str,
        rect: Rect,
        consumes: bool,
        log: &Log,
        children: Vec<Box<dyn Widget>>,
    ) -> Box<dyn Widget> {
        Box::new(Node {
            name,
            rect,
            bounds: Rect::default(),
            consumes,
            log: Rc::clone(log),
            children,
        })
    }
}

fn new_log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
}

fn app_with(root: Box<dyn Widget>) -> App {
    let mut app = App::new(root);
    app.layout(VIEW);
    app
}

/// Drain the log.
fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

fn press(app: &mut App, x: f64) {
    app.on_mouse_down(x, 50.0, MouseButton::Left, Modifiers::default());
}

fn release(app: &mut App, x: f64) {
    app.on_mouse_up(x, 50.0, MouseButton::Left, Modifiers::default());
}

/// root (0..400) ─┬─ a (0..200)   ── a1 (0..100 inside a)
///                └─ b (200..400) ── b1 (100..200 inside b, so 300..400)
fn nested_app(log: &Log) -> App {
    let full = Rect::new(0.0, 0.0, 200.0, 100.0);
    let a1 = Node::boxed("a1", Rect::new(0.0, 0.0, 100.0, 100.0), true, log, vec![]);
    let b1 = Node::boxed("b1", Rect::new(100.0, 0.0, 100.0, 100.0), true, log, vec![]);
    let a = Node::boxed("a", full, false, log, vec![a1]);
    let b = Node::boxed(
        "b",
        Rect::new(200.0, 0.0, 200.0, 100.0),
        false,
        log,
        vec![b1],
    );
    let root = Node::boxed(
        "root",
        Rect::new(0.0, 0.0, 400.0, 100.0),
        false,
        log,
        vec![a, b],
    );
    app_with(root)
}

#[test]
fn wrapper_around_text_field_gets_enter_and_leave() {
    let log = new_log();
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let field: Box<dyn Widget> = Box::new(TextField::new(font));
    let wrap = Node::boxed(
        "wrap",
        Rect::new(100.0, 0.0, 200.0, 100.0),
        false,
        &log,
        vec![field],
    );
    let root = Node::boxed(
        "root",
        Rect::new(0.0, 0.0, 400.0, 100.0),
        false,
        &log,
        vec![wrap],
    );
    let mut app = app_with(root);

    app.on_mouse_move(10.0, 50.0);
    assert_eq!(take(&log), ["root enter", "root bubbled move"]);

    // Onto the field: the field takes the move, yet its wrapper is told the
    // pointer arrived.
    // (The root gets the hover-clear sentinel: it was the hovered widget.)
    app.on_mouse_move(150.0, 50.0);
    assert_eq!(take(&log), ["wrap enter", "root clear"]);

    // Moving within the field announces nothing new. (The field consumes
    // only the move on which its hover flips; later moves bubble, which is
    // why the wrapper cannot rely on seeing moves.)
    app.on_mouse_move(250.0, 50.0);
    assert_eq!(take(&log), ["wrap bubbled move", "root bubbled move"]);

    // Off the field, back onto the bare root.
    app.on_mouse_move(350.0, 50.0);
    assert_eq!(take(&log), ["wrap leave", "root bubbled move"]);
}

#[test]
fn nested_chains_gain_and_lose_only_the_changed_widgets() {
    let log = new_log();
    let mut app = nested_app(&log);

    app.on_mouse_move(50.0, 50.0);
    assert_eq!(take(&log), ["root enter", "a enter", "a1 enter", "a1 move"]);

    // a1 → a (outside a1): only a1 leaves.
    app.on_mouse_move(150.0, 50.0);
    assert_eq!(
        take(&log),
        [
            "a1 leave",
            "a1 clear",
            "a bubbled move",
            "root bubbled move"
        ]
    );

    // a → b1: a leaves, b and b1 enter (shallowest first), root stays.
    app.on_mouse_move(350.0, 50.0);
    assert_eq!(
        take(&log),
        [
            "a leave",
            "b enter",
            "b1 enter",
            "a clear",
            "root clear",
            "b1 move"
        ]
    );

    // b1 → a1 directly: the deepest leaves first, then the new branch enters.
    app.on_mouse_move(50.0, 50.0);
    assert_eq!(
        take(&log),
        ["b1 leave", "b leave", "a enter", "a1 enter", "b1 clear", "a1 move"]
    );

    // The pointer leaves the window: everything leaves, root last.
    app.on_mouse_leave();
    assert_eq!(
        take(&log),
        ["a1 leave", "a leave", "root leave", "a1 clear"]
    );
}

#[test]
fn capture_keeps_ancestors_and_toggles_only_the_captured_widget() {
    let log = new_log();
    let mut app = nested_app(&log);
    app.on_mouse_move(50.0, 50.0);
    press(&mut app, 50.0);
    assert!(app.has_captured_pointer());
    take(&log);

    // Dragging over b1: the captured a1 leaves its own area; its ancestor a
    // stays entered and b / b1 hear nothing while capture holds.
    app.on_mouse_move(350.0, 50.0);
    assert_eq!(take(&log), ["a1 leave", "a1 move"]);

    // Back over a1: it re-enters.
    app.on_mouse_move(60.0, 50.0);
    // (b1, hit-tested as hovered during the drag, gets the hover-clear.)
    assert_eq!(take(&log), ["a1 enter", "b1 clear", "a1 move"]);

    // Out again, then release over b1: capture ends and the chain catches up.
    app.on_mouse_move(350.0, 50.0);
    take(&log);
    release(&mut app, 350.0);
    assert_eq!(
        take(&log),
        ["a1 up", "a leave", "b enter", "b1 enter", "b1 move"]
    );
}

#[test]
fn dropped_hovered_widget_gets_no_leave_and_its_replacement_enters() {
    let log = new_log();
    let full = Rect::new(0.0, 0.0, 400.0, 100.0);
    let a = Node::boxed("a", full, true, &log, vec![]);
    let root = Node::boxed("root", full, false, &log, vec![a]);
    let mut app = app_with(root);
    app.on_mouse_move(50.0, 50.0);
    assert_eq!(take(&log), ["root enter", "a enter", "a move"]);

    // Replace the hovered child with a new widget at the same index.
    app.root_mut().children_mut()[0] = Node::boxed("c", full, true, &log, vec![]);
    app.layout(VIEW);
    app.on_mouse_move(60.0, 50.0);
    assert_eq!(take(&log), ["c enter", "c move"]);
}
