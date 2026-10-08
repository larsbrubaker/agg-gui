//! First-under-mouse tracking (`Event::MouseOver` / `Event::MouseOut`), the
//! `UnderMouseState` queries on [`App`] and the thread snapshot
//! (`under_mouse_state_of`), through the real pointer entry points.
//!
//! `widget/app/hover_chain.rs` sends over/out beside the bounds enter/leave
//! that `hover_enter_leave.rs` covers; `widget/app/under_mouse.rs` holds the
//! queries. Each `Node` logs over/out/enter/leave plus the state it reads for
//! itself from the snapshot while handling the event.

use std::cell::RefCell;
use std::rc::Rc;

use crate::{
    under_mouse_state_of, App, DrawCtx, Event, EventResult, Modifiers, MouseButton, Rect, Size,
    UnderMouseState, Widget, WidgetId,
};

const VIEW: Size = Size {
    width: 200.0,
    height: 200.0,
};

type Log = Rc<RefCell<Vec<String>>>;

/// A rectangle at `rect` in its parent that logs `"<name> <event> <state>"`
/// for the hover events, reading its own state from the snapshot. It takes
/// presses when `captures`.
struct Node {
    name: &'static str,
    rect: Rect,
    captures: bool,
    log: Log,
    children: Vec<Box<dyn Widget>>,
}

fn short(state: UnderMouseState) -> &'static str {
    match state {
        UnderMouseState::NotUnderMouse => "not",
        UnderMouseState::UnderMouseNotFirst => "under",
        UnderMouseState::FirstUnderMouse => "first",
    }
}

impl Widget for Node {
    fn type_name(&self) -> &'static str {
        self.name
    }
    fn bounds(&self) -> Rect {
        self.rect
    }
    fn set_bounds(&mut self, b: Rect) {
        self.rect = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.rect.width, self.rect.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        let kind = match event {
            Event::MouseOver => "over",
            Event::MouseOut => "out",
            Event::MouseEnter => "enter",
            Event::MouseLeave => "leave",
            Event::MouseDown { .. } if self.captures => return EventResult::Consumed,
            Event::MouseUp { .. } if self.captures => return EventResult::Consumed,
            _ => return EventResult::Ignored,
        };
        let state = under_mouse_state_of(WidgetId::of(&*self));
        self.log
            .borrow_mut()
            .push(format!("{} {kind} {}", self.name, short(state)));
        EventResult::Ignored
    }
}

fn node(name: &'static str, rect: Rect, captures: bool, log: &Log) -> Node {
    Node {
        name,
        rect,
        captures,
        log: Rc::clone(log),
        children: Vec::new(),
    }
}

/// root (0..200) ── outer (10..190) ── inner (20..60 inside outer)
fn nested(log: &Log, captures: bool) -> App {
    let inner = node("inner", Rect::new(20.0, 20.0, 40.0, 40.0), captures, log);
    let mut outer = node("outer", Rect::new(10.0, 10.0, 180.0, 180.0), captures, log);
    outer.children.push(Box::new(inner));
    let mut root = node("root", Rect::new(0.0, 0.0, 200.0, 200.0), false, log);
    root.children.push(Box::new(outer));
    let mut app = App::new(Box::new(root));
    app.layout(VIEW);
    app
}

fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

/// Y-up window point to the App's Y-down input.
fn at(y_up: f64) -> f64 {
    VIEW.height - y_up
}

#[test]
fn over_and_out_follow_the_first_widget_under_the_mouse() {
    let log: Log = Rc::default();
    let mut app = nested(&log, false);

    app.on_mouse_move(5.0, at(5.0));
    assert_eq!(take(&log), ["root enter first", "root over first"]);
    assert_eq!(app.first_under_mouse(), Some(&[][..]));

    // Onto outer's own surface: the root stays under the mouse but is no
    // longer first; every handler reads the settled states.
    app.on_mouse_move(15.0, at(15.0));
    assert_eq!(
        take(&log),
        ["root out under", "outer enter first", "outer over first"]
    );
    assert_eq!(
        app.under_mouse_state(&[]),
        UnderMouseState::UnderMouseNotFirst
    );
    assert_eq!(
        app.under_mouse_state(&[0]),
        UnderMouseState::FirstUnderMouse
    );
    assert_eq!(
        app.under_mouse_state(&[0, 0]),
        UnderMouseState::NotUnderMouse
    );

    // Within outer: nothing changes.
    app.on_mouse_move(16.0, at(15.0));
    assert!(take(&log).is_empty());

    // Into inner, then out of the window altogether.
    app.on_mouse_move(40.0, at(40.0));
    assert_eq!(
        take(&log),
        ["outer out under", "inner enter first", "inner over first"]
    );
    assert_eq!(app.hovered_chain(), Some(&[0, 0][..]));
    app.on_mouse_move(-5.0, at(-5.0));
    assert_eq!(
        take(&log),
        [
            "inner out not",
            "inner leave not",
            "outer leave not",
            "root leave not"
        ]
    );
    assert_eq!(app.hovered_chain(), None);
    assert_eq!(app.first_under_mouse(), None);
}

#[test]
fn a_press_with_no_move_before_it_announces_where_it_landed() {
    let log: Log = Rc::default();
    let mut app = nested(&log, false);
    app.on_mouse_move(5.0, at(5.0));
    take(&log);

    app.on_mouse_down(40.0, at(40.0), MouseButton::Left, Modifiers::default());
    assert_eq!(
        take(&log),
        [
            "root out under",
            "outer enter under",
            "inner enter first",
            "inner over first"
        ]
    );
    app.on_mouse_up(40.0, at(40.0), MouseButton::Left, Modifiers::default());
    assert!(take(&log).is_empty());

    app.on_mouse_down(5.0, at(5.0), MouseButton::Left, Modifiers::default());
    assert_eq!(
        take(&log),
        [
            "inner out not",
            "inner leave not",
            "outer leave not",
            "root over first"
        ]
    );
}

#[test]
fn capture_off_the_captured_widget_leaves_nothing_first() {
    let log: Log = Rc::default();
    let mut app = nested(&log, true);
    app.on_mouse_move(40.0, at(40.0));
    take(&log);

    app.on_mouse_down(40.0, at(40.0), MouseButton::Left, Modifiers::default());
    assert_eq!(app.captured_path(), Some(&[0, 0][..]));
    assert!(take(&log).is_empty());

    // Dragged off inner onto outer: inner leaves, its ancestors stay under
    // the mouse and none of them becomes first (agg-sharp's
    // `OnMouseMoveWhenCaptured`).
    app.on_mouse_move(15.0, at(15.0));
    assert_eq!(take(&log), ["inner out not", "inner leave not"]);
    assert_eq!(app.first_under_mouse(), None);
    assert_eq!(
        app.under_mouse_state(&[0]),
        UnderMouseState::UnderMouseNotFirst
    );

    // Back over inner: first again.
    app.on_mouse_move(40.0, at(40.0));
    assert_eq!(take(&log), ["inner enter first", "inner over first"]);

    // Released off it: capture ends and the chain catches up.
    app.on_mouse_move(15.0, at(15.0));
    take(&log);
    app.on_mouse_up(15.0, at(15.0), MouseButton::Left, Modifiers::default());
    assert_eq!(app.captured_path(), None);
    assert_eq!(take(&log), ["outer over first"]);
}
