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
}

impl MoveConsumer {
    fn new() -> Self {
        Self {
            bounds: Rect::default(),
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
        &[]
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        panic!("MoveConsumer has no children")
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
