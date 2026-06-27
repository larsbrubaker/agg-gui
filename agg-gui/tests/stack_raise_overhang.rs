//! Regression: `Stack::layout` must not panic when a child that requests a
//! raise sits at an index beyond the parallel `aligned` vec.
//!
//! `aligned` can be shorter than `children` because callers mutate the tree
//! through `children_mut()` (e.g. the demo pushes a full-canvas overlay
//! directly, bypassing `add()`). The layout read path already tolerates this
//! (`aligned.get(idx).unwrap_or(false)`); the bring-to-front pass used to
//! assume a strict 1:1 length and panicked in `Vec::remove`. Adding a new demo
//! window shifted indices enough to land a raise on the overhang slot and crash
//! the whole app on startup.

use agg_gui::{DrawCtx, Event, EventResult, Rect, Size, Stack, Widget};

/// Minimal widget that asks to be raised exactly once.
struct RaiseOnce {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    pending: bool,
}

impl RaiseOnce {
    fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            pending: true,
        }
    }
}

impl Widget for RaiseOnce {
    fn type_name(&self) -> &'static str {
        "RaiseOnce"
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
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn take_raise_request(&mut self) -> bool {
        std::mem::replace(&mut self.pending, false)
    }
}

/// Inert filler widget so the stack has stretched children before the overhang.
struct Filler {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Filler {
    fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
        }
    }
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
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}

#[test]
fn raise_on_overhang_index_does_not_panic() {
    // Two children via add() → aligned.len() == 2.
    let mut stack = Stack::new().add(Box::new(Filler::new())).add(Box::new(Filler::new()));
    // A third child pushed directly bypasses `aligned` → children.len() == 3,
    // aligned.len() == 2. This raise-requesting child sits on the overhang.
    stack.children_mut().push(Box::new(RaiseOnce::new()));

    // Must not panic.
    stack.layout(Size::new(200.0, 100.0));

    // The raised child is moved to the end (front-most) and all three survive.
    assert_eq!(stack.children().len(), 3);
    assert_eq!(stack.children()[2].type_name(), "RaiseOnce");
}
