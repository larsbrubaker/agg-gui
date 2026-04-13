//! `Container` — a rectangular box with optional background, border, and
//! padding that holds zero or more child widgets.
//!
//! Phase 4 child layout is a simple top-down vertical stack (bottom-most child
//! at `y = padding`, each subsequent child placed above the previous). Flex
//! layout arrives in Phase 5.

use crate::color::Color;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::widget::Widget;

/// A rectangular container widget.
///
/// Paints a background rounded-rect (optional border), then lets the framework
/// recurse into its children. Children are stacked bottom-to-top inside the
/// padding area.
pub struct Container {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    pub background: Color,
    pub border_color: Option<Color>,
    pub border_width: f64,
    pub corner_radius: f64,
    pub padding: f64,
}

impl Container {
    /// Create a transparent container with no border and default padding.
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            background: Color::rgba(0.0, 0.0, 0.0, 0.0),
            border_color: None,
            border_width: 1.0,
            corner_radius: 0.0,
            padding: 0.0,
        }
    }

    /// Append a child widget.
    pub fn add(mut self, child: Box<dyn Widget>) -> Self {
        self.children.push(child);
        self
    }

    pub fn with_background(mut self, color: Color) -> Self {
        self.background = color;
        self
    }

    pub fn with_border(mut self, color: Color, width: f64) -> Self {
        self.border_color = Some(color);
        self.border_width = width;
        self
    }

    pub fn with_corner_radius(mut self, r: f64) -> Self {
        self.corner_radius = r;
        self
    }

    pub fn with_padding(mut self, p: f64) -> Self {
        self.padding = p;
        self
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Container {
    fn type_name(&self) -> &'static str { "Container" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, bounds: Rect) { self.bounds = bounds; }

    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let pad = self.padding;
        let inner_w = (available.width - pad * 2.0).max(0.0);

        // Stack children top-to-bottom (first child = visually highest).
        // In Y-up coordinates, "top" = higher Y values.
        // Start cursor at the top of the inner area; move it downward each step.
        let mut cursor_y = available.height - pad;

        for child in self.children.iter_mut() {
            let child_avail = Size::new(inner_w, cursor_y - pad);
            let desired = child.layout(child_avail);
            // Place child: its top edge is at cursor_y, bottom is cursor_y - height.
            let child_y = cursor_y - desired.height;
            let child_bounds = Rect::new(pad, child_y, desired.width.min(inner_w), desired.height);
            child.set_bounds(child_bounds);
            cursor_y = child_y - pad; // gap below this child
        }

        // Container fills all available space.
        Size::new(available.width, available.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let r = self.corner_radius;

        // Background
        if self.background.a > 0.001 {
            ctx.set_fill_color(self.background);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, w, h, r);
            ctx.fill();
        }

        // Border
        if let Some(bc) = self.border_color {
            ctx.set_stroke_color(bc);
            ctx.set_line_width(self.border_width);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, w, h, r);
            ctx.stroke();
        }
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
