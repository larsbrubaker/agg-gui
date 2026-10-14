//! Hover order on a move that swaps the hovered widget: the old leaf hears
//! its own `MouseOut`, `MouseLeave` and `(-1, -1)` hover-clear move before
//! its ancestors hear their `MouseLeave`, and every leave comes before any
//! widget the pointer reached hears `MouseEnter` / `MouseOver`.
//!
//! The bug this pins: `App::dispatch_pointer_move` sent the old leaf its
//! hover-clear move after the whole hover chain was updated, so a leaf that
//! clears a shared value when it loses hover (MatterCAD's Undo button
//! clearing the status-bar hint) wiped what another widget had just set on
//! the same move: on an enter (a sibling), or on an ancestor's leave
//! (MatterCAD's tab strip, which shows its drop hint when the pointer leaves
//! the tab content for the tabs). `hover_enter_leave.rs` covers the chain
//! diff itself.

use std::cell::RefCell;
use std::rc::Rc;

use crate::{App, DrawCtx, Event, EventResult, Rect, Size, Widget};

const VIEW: Size = Size {
    width: 300.0,
    height: 100.0,
};

type Log = Rc<RefCell<Vec<String>>>;
type Hint = Rc<RefCell<Option<&'static str>>>;

/// What a node does with the shared hint.
#[derive(Clone, Copy, PartialEq)]
enum HintRole {
    /// Leaves the hint alone.
    None,
    /// Clears the hint whenever it loses hover: on `MouseOut`, `MouseLeave`
    /// and the hover-clear move.
    ClearsOnLeave,
    /// Sets the hint to its name when the pointer enters it.
    SetsOnEnter,
    /// Sets the hint to its name when the pointer leaves it.
    SetsOnLeave,
}

/// A rectangle at `rect` in its parent that logs `"<name> <event>"` for the
/// hover events and acts on the shared hint per its role. Leaves consume
/// moves; containers let them bubble.
struct Node {
    name: &'static str,
    rect: Rect,
    bounds: Rect,
    leaf: bool,
    role: HintRole,
    hint: Hint,
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
            let rect = child_rect(child.as_ref());
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
            Event::MouseOver => "over",
            Event::MouseOut => "out",
            Event::MouseMove { pos } if pos.x < 0.0 => "clear",
            Event::MouseMove { .. } if self.leaf => "move",
            _ => return EventResult::Ignored,
        };
        self.log.borrow_mut().push(format!("{} {kind}", self.name));
        match (self.role, kind) {
            (HintRole::ClearsOnLeave, "out" | "leave" | "clear") => {
                *self.hint.borrow_mut() = None;
            }
            (HintRole::SetsOnEnter, "enter") | (HintRole::SetsOnLeave, "leave") => {
                *self.hint.borrow_mut() = Some(self.name);
            }
            _ => {}
        }
        if self.leaf || !kind.ends_with("move") && kind != "clear" {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }
}

fn child_rect(child: &dyn Widget) -> Rect {
    let props = child.properties();
    let v = &props
        .iter()
        .find(|(k, _)| *k == "rect")
        .expect("every child is a Node")
        .1;
    let n: Vec<f64> = v.split(',').map(|s| s.parse().unwrap_or(0.0)).collect();
    Rect::new(n[0], n[1], n[2], n[3])
}

struct Shared {
    log: Log,
    hint: Hint,
}

impl Shared {
    fn node(
        &self,
        name: &'static str,
        rect: Rect,
        leaf: bool,
        role: HintRole,
        children: Vec<Box<dyn Widget>>,
    ) -> Box<dyn Widget> {
        Box::new(Node {
            name,
            rect,
            bounds: Rect::default(),
            leaf,
            role,
            hint: Rc::clone(&self.hint),
            log: Rc::clone(&self.log),
            children,
        })
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.borrow_mut())
    }
}

/// root (0..300) ─┬─ a (0..200, a toolbar) ── b (100..200, its button leaf)
///                └─ c (200..300, a tab strip leaf)
#[test]
fn every_leave_and_the_old_leaf_clear_come_before_any_enter() {
    let s = Shared {
        log: Rc::new(RefCell::new(Vec::new())),
        hint: Rc::new(RefCell::new(None)),
    };
    let b = s.node(
        "b",
        Rect::new(100.0, 0.0, 100.0, 100.0),
        true,
        HintRole::ClearsOnLeave,
        vec![],
    );
    let a = s.node(
        "a",
        Rect::new(0.0, 0.0, 200.0, 100.0),
        false,
        HintRole::None,
        vec![b],
    );
    let c = s.node(
        "c",
        Rect::new(200.0, 0.0, 100.0, 100.0),
        true,
        HintRole::SetsOnEnter,
        vec![],
    );
    let root = s.node(
        "root",
        Rect::new(0.0, 0.0, 300.0, 100.0),
        false,
        HintRole::None,
        vec![a, c],
    );
    let mut app = App::new(root);
    app.layout(VIEW);

    // Over a, then over its leaf b.
    app.on_mouse_move(50.0, 50.0);
    app.on_mouse_move(150.0, 50.0);
    s.take();
    assert_eq!(*s.hint.borrow(), None);

    // One step from b onto c: b is left and gets its hover-clear move, then
    // its ancestor a is left, and only then does c enter.
    app.on_mouse_move(250.0, 50.0);
    assert_eq!(
        s.take(),
        ["b out", "b leave", "b clear", "a leave", "c enter", "c over", "c move"]
    );
    assert_eq!(*s.hint.borrow(), Some("c"), "b's clear wiped c's hint");
}

/// root (0..300) ── strip (0..300) ─┬─ content (0..200) ── b (100..200, leaf)
///                                   └─ tab (200..300, leaf)
///
/// The strip's hint shows while the pointer is on the strip but not on its
/// content, so it is set when the content is left (`SetsOnLeave`): the old
/// leaf's clear must come before that ancestor's leave.
#[test]
fn the_old_leaf_clear_comes_before_its_ancestors_leave() {
    let s = Shared {
        log: Rc::new(RefCell::new(Vec::new())),
        hint: Rc::new(RefCell::new(None)),
    };
    let b = s.node(
        "b",
        Rect::new(100.0, 0.0, 100.0, 100.0),
        true,
        HintRole::ClearsOnLeave,
        vec![],
    );
    let content = s.node(
        "content",
        Rect::new(0.0, 0.0, 200.0, 100.0),
        false,
        HintRole::SetsOnLeave,
        vec![b],
    );
    let tab = s.node(
        "tab",
        Rect::new(200.0, 0.0, 100.0, 100.0),
        true,
        HintRole::None,
        vec![],
    );
    let strip = s.node(
        "strip",
        Rect::new(0.0, 0.0, 300.0, 100.0),
        false,
        HintRole::None,
        vec![content, tab],
    );
    let root = s.node(
        "root",
        Rect::new(0.0, 0.0, 300.0, 100.0),
        false,
        HintRole::None,
        vec![strip],
    );
    let mut app = App::new(root);
    app.layout(VIEW);

    app.on_mouse_move(150.0, 50.0);
    s.take();

    app.on_mouse_move(250.0, 50.0);
    assert_eq!(
        s.take(),
        [
            "b out",
            "b leave",
            "b clear",
            "content leave",
            "tab enter",
            "tab over",
            "tab move"
        ]
    );
    assert_eq!(
        *s.hint.borrow(),
        Some("content"),
        "b's clear wiped the hint its ancestor's leave set"
    );
}
