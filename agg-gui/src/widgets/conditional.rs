//! Conditional — show/hide a single child based on an external Cell<bool>.

use std::cell::Cell;
use std::rc::Rc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

/// Wrap a single child widget; show or hide it based on a shared boolean.
///
/// Mirrors the React-style `{cond && <Child/>}` pattern but without
/// rebuilding the child on every toggle — the child keeps its state
/// across show/hide cycles.  When hidden, layout returns `(0, 0)` and
/// `is_visible` returns `false`, so paint / event dispatch skip the
/// subtree.  Containers like `FlexColumn` honour `is_visible` to skip
/// gap/margin allotted to hidden slots.
pub struct Conditional {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // 0 or 1 element
    base: WidgetBase,
    visible: Rc<Cell<bool>>,
}

impl Conditional {
    pub fn new(visible: Rc<Cell<bool>>, child: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![child],
            base: WidgetBase::new(),
            visible,
        }
    }

    pub fn visibility_cell(&self) -> Rc<Cell<bool>> {
        Rc::clone(&self.visible)
    }
}

impl Widget for Conditional {
    fn type_name(&self) -> &'static str {
        "Conditional"
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

    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }

    fn layout(&mut self, available: Size) -> Size {
        if !self.visible.get() {
            self.bounds = Rect::new(0.0, 0.0, 0.0, 0.0);
            return Size::new(0.0, 0.0);
        }
        if let Some(child) = self.children.first_mut() {
            let s = child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
            self.bounds = Rect::new(0.0, 0.0, s.width, s.height);
            return s;
        }
        Size::new(0.0, 0.0)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn is_visible(&self) -> bool {
        self.visible.get()
    }

    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}
