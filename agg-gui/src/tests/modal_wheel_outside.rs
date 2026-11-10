//! The mouse wheel beside a modal that lets the pointer pass outside it
//! ([`Widget::modal_hover_passes_outside`]): it reaches the widget under the
//! pointer (a 3D view's zoom, a list beside a light-dismiss panel), while the
//! wheel inside the modal still reaches the modal's own scroll list. A modal
//! that keeps the default (a dialog over a backdrop) takes every wheel.

use super::*;
use crate::{DrawCtx, Event, EventResult, Rect, Widget};
use std::cell::Cell;
use std::rc::Rc;

const ROOT: f64 = 200.0;

/// A widget at a fixed rectangle (Y-up) that counts the wheel events it
/// receives and consumes them, as a scroll list does. As a modal it holds a
/// child list of its own.
struct WheelWidget {
    bounds: Rect,
    modal: bool,
    passes: bool,
    wheels: Rc<Cell<u32>>,
    children: Vec<Box<dyn Widget>>,
}

impl WheelWidget {
    fn leaf(bounds: Rect, wheels: &Rc<Cell<u32>>) -> Self {
        Self {
            bounds,
            modal: false,
            passes: false,
            wheels: Rc::clone(wheels),
            children: Vec::new(),
        }
    }
}

impl Widget for WheelWidget {
    fn type_name(&self) -> &'static str {
        "WheelWidget"
    }
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
    fn has_active_modal(&self) -> bool {
        self.modal
    }
    fn modal_hover_passes_outside(&self) -> bool {
        self.passes
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseWheel { .. } => {
                self.wheels.set(self.wheels.get() + 1);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

/// The wheel counts of the widget beside the modal (x 120..200), the modal
/// panel itself (x 0..100) and the list inside the modal (x 10..90).
struct Scene {
    app: App,
    side: Rc<Cell<u32>>,
    modal: Rc<Cell<u32>>,
    list: Rc<Cell<u32>>,
}

impl Scene {
    fn counts(&self) -> (u32, u32, u32) {
        (self.side.get(), self.modal.get(), self.list.get())
    }
}

fn scene(passes: bool) -> Scene {
    let counter = || Rc::new(Cell::new(0));
    let (side, modal, list) = (counter(), counter(), counter());
    let side_widget = WheelWidget::leaf(Rect::new(120.0, 0.0, 80.0, ROOT), &side);
    let mut modal_widget = WheelWidget::leaf(Rect::new(0.0, 0.0, 100.0, ROOT), &modal);
    modal_widget.modal = true;
    modal_widget.passes = passes;
    modal_widget.children.push(Box::new(WheelWidget::leaf(
        Rect::new(10.0, 10.0, 80.0, 180.0),
        &list,
    )));
    let root = WheelWidget {
        bounds: Rect::new(0.0, 0.0, ROOT, ROOT),
        modal: false,
        passes: false,
        wheels: counter(),
        children: vec![Box::new(side_widget), Box::new(modal_widget)],
    };
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(ROOT, ROOT));
    Scene {
        app,
        side,
        modal,
        list,
    }
}

#[test]
fn the_wheel_beside_a_pass_through_modal_reaches_the_widget_under_the_pointer() {
    let mut s = scene(true);
    s.app.on_mouse_wheel(150.0, ROOT / 2.0, 1.0);
    assert_eq!(s.counts(), (1, 0, 0), "the widget beside the modal");
}

#[test]
fn the_wheel_inside_a_pass_through_modal_reaches_its_own_list() {
    let mut s = scene(true);
    s.app.on_mouse_wheel(50.0, ROOT / 2.0, 1.0);
    assert_eq!(s.counts(), (0, 0, 1), "the modal's list");
}

#[test]
fn a_modal_that_keeps_the_pointer_takes_the_wheel_outside_it() {
    let mut s = scene(false);
    s.app.on_mouse_wheel(150.0, ROOT / 2.0, 1.0);
    assert_eq!(s.counts(), (0, 1, 0), "the modal, never the widget beneath");
    s.app.on_mouse_wheel(50.0, ROOT / 2.0, 1.0);
    assert_eq!(s.counts(), (0, 1, 1), "the modal's list");
}
