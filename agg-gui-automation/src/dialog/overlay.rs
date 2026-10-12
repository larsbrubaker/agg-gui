//! The built-in overlay layer of an [`AutomationWindow`](crate::AutomationWindow):
//! where a [`ModalPresenter`] that is the run's own window puts its modals.
//!
//! [`AutomationModalPresenter`] is the cloneable handle (an `Rc` queue) the
//! application keeps; [`ModalLayer`] is the widget in the tree that takes
//! the queued modals into its children on its next layout, lays each over
//! the whole window and drops each once it has hidden itself. The window
//! stacks the layer above its root (`execute/window.rs`) only when a
//! presenter was asked for, so a run that never shows a modal keeps its tree
//! exactly as built.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::{Point, Rect, Size, Widget};

use super::ModalPresenter;

type Queue = Rc<RefCell<Vec<Box<dyn Widget>>>>;

/// The handle an application presents modals through when its window is an
/// [`AutomationWindow`](crate::AutomationWindow)
/// ([`AutomationWindow::modal_presenter`](crate::AutomationWindow::modal_presenter)).
#[derive(Clone, Default)]
pub struct AutomationModalPresenter {
    queue: Queue,
}

impl AutomationModalPresenter {
    pub(crate) fn layer(&self) -> ModalLayer {
        ModalLayer {
            bounds: Rect::default(),
            children: Vec::new(),
            queue: Rc::clone(&self.queue),
        }
    }
}

impl ModalPresenter for AutomationModalPresenter {
    fn present(&self, modal: Box<dyn Widget>) {
        self.queue.borrow_mut().push(modal);
        agg_gui::animation::request_layout();
        agg_gui::animation::request_draw();
    }
}

/// The overlay layer widget; see the module docs.
pub(crate) struct ModalLayer {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    queue: Queue,
}

impl ModalLayer {
    fn shows_anything(&self) -> bool {
        self.children.iter().any(|c| c.is_visible())
    }
}

impl Widget for ModalLayer {
    fn type_name(&self) -> &'static str {
        "AutomationModalLayer"
    }

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

    /// Shown while a modal shows or waits for this layer's next layout, so
    /// an idle layer neither paints nor takes a press.
    fn is_visible(&self) -> bool {
        !self.queue.borrow().is_empty() || self.shows_anything()
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        self.shows_anything()
            && local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }

    fn layout(&mut self, available: Size) -> Size {
        self.children.retain(|c| c.is_visible());
        self.children.append(&mut self.queue.borrow_mut());
        let full = Rect::new(0.0, 0.0, available.width, available.height);
        for child in &mut self.children {
            child.layout(available);
            child.set_bounds(full);
        }
        available
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
