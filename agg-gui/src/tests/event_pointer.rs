//! `event_pointer_local`: the pointer in the local space of the widget
//! handling the current event — agg-sharp's `MouseEventArgs.Position` on
//! events that carry no position here (`MouseEnter`/`MouseLeave`,
//! agg-sharp's `MouseEnterBounds`/`MouseLeaveBounds`, which do).

use std::cell::RefCell;
use std::rc::Rc;

use crate::{event_pointer_local, App, DrawCtx, Event, EventResult, Point, Rect, Size, Widget};

type Seen = Rc<RefCell<Vec<Option<Point>>>>;

/// A box at fixed bounds that records `event_pointer_local()` on enter.
struct Spot {
    bounds: Rect,
    seen: Seen,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Spot {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, _b: Rect) {}
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, _available: Size) -> Size {
        for c in &mut self.children {
            let b = c.bounds();
            c.layout(Size::new(b.width, b.height));
        }
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        if matches!(event, Event::MouseEnter) {
            self.seen.borrow_mut().push(event_pointer_local());
        }
        EventResult::Ignored
    }
}

#[test]
fn rust_only_an_enter_knows_where_the_pointer_is_in_local_space() {
    let seen: Seen = Rc::default();
    let inner = Spot {
        bounds: Rect::new(50.0, 20.0, 40.0, 40.0),
        seen: Rc::clone(&seen),
        children: Vec::new(),
    };
    let root = Spot {
        bounds: Rect::new(0.0, 0.0, 200.0, 100.0),
        seen: Rc::new(RefCell::new(Vec::new())),
        children: vec![Box::new(inner)],
    };
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(200.0, 100.0));
    // Y-down (60, 70) is Y-up (60, 30): (10, 10) inside the inner box.
    app.on_mouse_move(60.0, 70.0);
    assert_eq!(*seen.borrow(), vec![Some(Point::new(10.0, 10.0))]);
    assert_eq!(event_pointer_local(), None, "outside dispatch");
}
