//! Rebuilder — regenerate a child subtree when an external version
//! changes.
//!
//! agg-gui trees are built once; widgets whose *content set* is dynamic
//! (a picker whose options change, a list whose row count changes) can't
//! express that with setters alone. `Rebuilder` closes the gap: give it
//! a cheap `version_fn` and a `builder`; each layout pass it compares the
//! version and rebuilds the child when it moved. The declarative-UI
//! equivalent of keying a subtree by its data identity.
//!
//! ```ignore
//! let list = Rebuilder::new(
//!     move || items.borrow().len() as u64,
//!     move || build_rows(&items.borrow(), &font),
//! );
//! ```
//!
//! Keep `version_fn` cheap (it runs every layout) and derive it from the
//! data the builder reads — hash names, count rows, bump a counter.

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

pub struct Rebuilder {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // exactly 1 once built
    base: WidgetBase,
    version_fn: Box<dyn Fn() -> u64>,
    builder: Box<dyn Fn() -> Box<dyn Widget>>,
    version: Option<u64>,
}

impl Rebuilder {
    pub fn new(
        version_fn: impl Fn() -> u64 + 'static,
        builder: impl Fn() -> Box<dyn Widget> + 'static,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            version_fn: Box::new(version_fn),
            builder: Box::new(builder),
            version: None,
        }
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }
}

impl Widget for Rebuilder {
    fn type_name(&self) -> &'static str {
        "Rebuilder"
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
        let version = (self.version_fn)();
        if self.version != Some(version) {
            self.children.clear();
            self.children.push((self.builder)());
            self.version = Some(version);
        }
        let Some(child) = self.children.first_mut() else {
            return Size::new(0.0, 0.0);
        };
        let s = child.layout(available);
        child.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        self.bounds = Rect::new(0.0, 0.0, s.width, s.height);
        s
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}
