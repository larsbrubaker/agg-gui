//! Regression tests for pointer capture used by touch-driven scroll gestures.
//!
//! Mobile web scroll is implemented by synthesizing a middle-button drag.  Once
//! a `ScrollView` starts that drag it must own subsequent moves, even if the
//! original content child would otherwise consume hover events.

use std::cell::Cell;
use std::rc::Rc;

use crate::{
    App, DrawCtx, Event, EventResult, Modifiers, MouseButton, Rect, ScrollView, Size, Widget,
};

struct MoveConsumer {
    bounds: Rect,
    /// Always empty — this is a leaf. Kept as a real field so whole-tree walks
    /// (e.g. `App::paint`'s dirty-marking pass) can traverse through it.
    children: Vec<Box<dyn Widget>>,
}

impl MoveConsumer {
    fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
        }
    }
}

impl Widget for MoveConsumer {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, _available: Size) -> Size {
        Size::new(300.0, 300.0)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { .. } => EventResult::Consumed,
            _ => EventResult::Ignored,
        }
    }
}

#[test]
fn middle_drag_capture_moves_from_hover_consuming_child_to_scroll_view() {
    let v_offset = Rc::new(Cell::new(0.0));
    let scroll = ScrollView::new(Box::new(MoveConsumer::new()))
        .horizontal(true)
        .with_offset_cell(Rc::clone(&v_offset));
    let mut app = App::new(Box::new(scroll));
    let viewport = Size::new(100.0, 100.0);
    app.layout(viewport);

    app.on_mouse_down(50.0, 50.0, MouseButton::Middle, Modifiers::default());
    app.on_mouse_move(50.0, 40.0);
    app.on_mouse_up(50.0, 40.0, MouseButton::Middle, Modifiers::default());

    assert_eq!(
        v_offset.get(),
        10.0,
        "middle-drag scrolling should stay captured by ScrollView, not by its content child"
    );
}

#[test]
fn middle_drag_scroll_uses_mouse_down_as_stable_anchor() {
    let v_offset = Rc::new(Cell::new(40.0));
    let h_offset = Rc::new(Cell::new(30.0));
    let scroll = ScrollView::new(Box::new(MoveConsumer::new()))
        .horizontal(true)
        .with_offset_cell(Rc::clone(&v_offset))
        .with_h_offset_cell(Rc::clone(&h_offset));
    let mut app = App::new(Box::new(scroll));
    let viewport = Size::new(100.0, 100.0);
    app.layout(viewport);

    app.on_mouse_down(50.0, 92.0, MouseButton::Middle, Modifiers::default());
    assert_eq!(v_offset.get(), 40.0, "mouse-down alone must not scroll");
    assert_eq!(h_offset.get(), 30.0, "mouse-down alone must not scroll");

    // A layout pass between mouse-down and first move must not change the drag
    // anchor. This catches jumps caused by mixing viewport/header coordinates
    // into the scroll position.
    app.layout(viewport);
    assert_eq!(
        v_offset.get(),
        40.0,
        "layout after mouse-down must not scroll"
    );
    assert_eq!(
        h_offset.get(),
        30.0,
        "layout after mouse-down must not scroll"
    );

    app.on_mouse_move(40.0, 82.0);
    app.on_mouse_up(40.0, 82.0, MouseButton::Middle, Modifiers::default());

    assert_eq!(v_offset.get(), 50.0);
    assert_eq!(h_offset.get(), 40.0);
}

/// Regression: positive wheel `delta_y` DECREASES scroll offset (the
/// user wants to see content ABOVE), and negative INCREASES it.
/// Matches winit / `WheelEvent` after the OS applies natural-scroll —
/// see the `Event::MouseWheel` doc.
#[test]
fn test_scroll_view_wheel_direction_matches_system_convention() {
    use crate::Point;
    use crate::SizedBox;
    let v = Rc::new(Cell::new(200.0));
    let mut sv = ScrollView::new(Box::new(SizedBox::new().with_height(2000.0)))
        .with_offset_cell(Rc::clone(&v));
    sv.layout(Size::new(200.0, 200.0));
    let wheel = |dy: f64| Event::MouseWheel {
        pos: Point::new(100.0, 100.0),
        delta_y: dy,
        delta_x: 0.0,
        modifiers: Modifiers::default(),
    };
    let before = v.get();
    sv.on_event(&wheel(1.0));
    assert!(
        v.get() < before,
        "+y must scroll up; {before} → {}",
        v.get()
    );
    let mid = v.get();
    sv.on_event(&wheel(-1.0));
    assert!(v.get() > mid, "-y must scroll down; {mid} → {}", v.get());
}
