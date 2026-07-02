//! ReserveInset — transparent wrapper that marks its child as edge chrome.
//!
//! Wrap an edge-hugging overlay (a left button rail, a bottom control
//! tray) in this and the strip the child ends up occupying is
//! automatically registered with [`crate::overlay_insets`] every frame.
//! Anchored floating content ([`crate::card`], tooltips) then stays out
//! from underneath it with zero coordination from the app — which is the
//! point: a developer shouldn't have to remember every sibling overlay to
//! place a card correctly.
//!
//! The wrapper is layout-transparent: it forwards size, anchors, margin,
//! and visibility from the child unchanged, so `.with_h_anchor(...)` etc.
//! configured on the child keep working when it's used as a `Stack`
//! overlay. The reservation happens in [`Widget::set_bounds`] — the one
//! call made exactly when the parent has decided the final on-screen
//! strip — using [`crate::widget::current_viewport`] for the far-edge
//! distances. Use it for direct overlay children of the root `Stack`,
//! whose bounds are viewport-relative.

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::{current_viewport, Widget};

/// Which viewport edge the wrapped chrome hugs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ReservedEdge {
    Left,
    Right,
    Top,
    Bottom,
}

/// See module docs. Construct via [`ReserveInset::left`] etc. and use in
/// place of the bare child.
pub struct ReserveInset {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // exactly 1
    edge: ReservedEdge,
}

impl ReserveInset {
    pub fn new(edge: ReservedEdge, child: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![child],
            edge,
        }
    }
    pub fn left(child: Box<dyn Widget>) -> Self {
        Self::new(ReservedEdge::Left, child)
    }
    pub fn right(child: Box<dyn Widget>) -> Self {
        Self::new(ReservedEdge::Right, child)
    }
    pub fn top(child: Box<dyn Widget>) -> Self {
        Self::new(ReservedEdge::Top, child)
    }
    pub fn bottom(child: Box<dyn Widget>) -> Self {
        Self::new(ReservedEdge::Bottom, child)
    }

    fn child(&self) -> &dyn Widget {
        self.children[0].as_ref()
    }
}

impl Widget for ReserveInset {
    fn type_name(&self) -> &'static str {
        "ReserveInset"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
        if let Some(child) = self.children.first_mut() {
            child.set_bounds(Rect::new(0.0, 0.0, b.width, b.height));
        }
        if !self.is_visible() || b.width <= 0.0 || b.height <= 0.0 {
            return;
        }
        // Reserve the strip between the viewport edge and the child's far
        // side, so a rail floating a few px off the edge still shadows
        // everything left of it (Y-up: `y` measures from the bottom).
        let viewport = current_viewport();
        let strip = match self.edge {
            ReservedEdge::Left => Insets {
                left: b.x + b.width,
                ..Insets::default()
            },
            ReservedEdge::Right => Insets {
                right: (viewport.width - b.x).max(0.0),
                ..Insets::default()
            },
            ReservedEdge::Bottom => Insets {
                bottom: b.y + b.height,
                ..Insets::default()
            },
            ReservedEdge::Top => Insets {
                top: (viewport.height - b.y).max(0.0),
                ..Insets::default()
            },
        };
        crate::overlay_insets::reserve(strip);
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    // Layout-transparent: the child's own sizing and placement properties
    // drive the parent container exactly as if it were unwrapped.
    fn margin(&self) -> Insets {
        self.child().margin()
    }
    fn h_anchor(&self) -> HAnchor {
        self.child().h_anchor()
    }
    fn v_anchor(&self) -> VAnchor {
        self.child().v_anchor()
    }
    fn min_size(&self) -> Size {
        self.child().min_size()
    }
    fn max_size(&self) -> Size {
        self.child().max_size()
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        self.children[0].widget_base()
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        self.children[0].widget_base_mut()
    }
    fn is_visible(&self) -> bool {
        self.child().is_visible()
    }
    fn measure_min_height(&self, available_w: f64) -> f64 {
        self.child().measure_min_height(available_w)
    }

    fn layout(&mut self, available: Size) -> Size {
        self.children[0].layout(available)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}
