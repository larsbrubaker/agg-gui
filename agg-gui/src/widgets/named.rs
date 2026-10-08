//! `Named` — gives a name (C# `GuiWidget.Name`) to a widget that has no
//! [`WidgetBase`](crate::WidgetBase) of its own, so `find_widget_by_id` and
//! GUI automation can find it.
//!
//! Widgets that embed a `WidgetBase` take a name directly with
//! [`Widget::with_name`]; this wrapper is for the rest (custom widgets, or a
//! boxed child whose concrete type the caller no longer has).  It lays its
//! single child out at the child's natural size at its own origin, paints
//! nothing itself, and can report extra inspector `properties()` (tests use
//! them to read a child's live state).

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::widget::Widget;

/// Reads the wrapper's reflection properties on demand.
type PropertiesFn = Box<dyn Fn() -> Vec<(&'static str, String)>>;

/// A name around exactly one child.
pub struct Named {
    name: String,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    properties: Option<PropertiesFn>,
}

impl Named {
    pub fn new(name: impl Into<String>, child: Box<dyn Widget>) -> Self {
        Self {
            name: name.into(),
            bounds: Rect::default(),
            children: vec![child],
            properties: None,
        }
    }

    /// Report `properties()` from `read` (the child's live state, say).
    pub fn with_properties(
        mut self,
        read: impl Fn() -> Vec<(&'static str, String)> + 'static,
    ) -> Self {
        self.properties = Some(Box::new(read));
        self
    }
}

impl Widget for Named {
    fn type_name(&self) -> &'static str {
        "Named"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.name)
    }
    fn set_name(&mut self, name: Option<String>) {
        // The wrapper exists to carry a name; clearing it leaves an empty one.
        self.name = name.unwrap_or_default();
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
    fn layout(&mut self, available: Size) -> Size {
        // `children_mut` lets a caller take the child away; an empty wrapper
        // is then simply empty rather than a panic.
        let Some(child) = self.children.first_mut() else {
            return Size::ZERO;
        };
        let size = child.layout(available);
        child.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
        size
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        self.properties
            .as_ref()
            .map_or_else(Vec::new, |read| read())
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}
