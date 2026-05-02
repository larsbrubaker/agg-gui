//! `CollapsingHeader` — a clickable header that shows/hides child content.
//!
//! # Composition
//!
//! ```text
//! CollapsingHeader
//!   ├── Label  (children[0]: header text, framework-painted)
//!   └── child  (children[1]: shown when expanded, swapped out when collapsed)
//! ```
//!
//! The triangle indicator is drawn as a path.  Clicking anywhere on the header
//! row toggles the collapsed/expanded state.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::label::Label;

const HEADER_H: f64 = 22.0;
const TRIANGLE_SIZE: f64 = 6.0;
const INDENT: f64 = 12.0;

/// A collapsible section header.  When expanded, the child widget is visible
/// below the header row.  When collapsed, only the header row is shown.
pub struct CollapsingHeader {
    bounds: Rect,
    /// `children[0]` is the header [`Label`].  When expanded, the content lives
    /// at `children[1]`; when collapsed, it is parked in `self.content`.
    children: Vec<Box<dyn Widget>>,
    open: bool,
    hovered: bool,
    /// The content shown when expanded.  Stored here while collapsed.
    content: Option<Box<dyn Widget>>,
}

impl CollapsingHeader {
    /// Create a new header with the given text, using the provided font.
    /// Starts expanded by default.
    pub fn new(text: impl Into<String>, font: Arc<Font>) -> Self {
        let label = Label::new(text, Arc::clone(&font)).with_font_size(13.0);
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(label)],
            open: true,
            hovered: false,
            content: None,
        }
    }

    /// Set whether the section is open (expanded) by default.
    pub fn default_open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Set the child content widget shown when expanded.
    pub fn with_content(mut self, content: Box<dyn Widget>) -> Self {
        self.content = Some(content);
        self
    }
}

impl Widget for CollapsingHeader {
    fn type_name(&self) -> &'static str {
        "CollapsingHeader"
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

    fn layout(&mut self, available: Size) -> Size {
        let w = available.width;

        // Sync `children[1]` with `open` state so the framework dispatches
        // events to content only when visible.  Closed → parked in `self.content`.
        if self.open && self.children.len() == 1 {
            if let Some(c) = self.content.take() {
                self.children.push(c);
            }
        } else if !self.open && self.children.len() > 1 {
            if let Some(c) = self.children.pop() {
                self.content = Some(c);
            }
        }

        // Layout content first so we know total height before placing the label.
        let content_h = if self.open && self.children.len() > 1 {
            let inset = INDENT * 0.5;
            let avail_w = (w - inset).max(0.0);
            let child = &mut self.children[1];
            let cs = child.layout(Size::new(avail_w, available.height - HEADER_H));
            child.set_bounds(Rect::new(inset, 0.0, cs.width, cs.height));
            cs.height
        } else {
            0.0
        };
        let total_h = HEADER_H + content_h;

        // Layout label inside the header row (Y-up: header sits at the top).
        let label_avail = Size::new(w - INDENT - TRIANGLE_SIZE * 2.0, HEADER_H);
        let ls = self.children[0].layout(label_avail);
        let header_bottom = total_h - HEADER_H;
        let label_y = header_bottom + (HEADER_H - ls.height) * 0.5;
        self.children[0].set_bounds(Rect::new(
            INDENT + TRIANGLE_SIZE * 2.0 + 4.0,
            label_y,
            ls.width,
            ls.height,
        ));

        self.bounds = Rect::new(0.0, 0.0, w, total_h);
        Size::new(w, total_h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Header row background — always shown at a subtle tint so the header
        // reads as a distinct section boundary even when not hovered.  Hover
        // deepens the tint slightly as click affordance.  Sits just below the
        // top divider line so the line remains crisp.
        let alpha = if self.hovered { 0.10 } else { 0.06 };
        ctx.set_fill_color(Color::rgba(
            v.text_color.r,
            v.text_color.g,
            v.text_color.b,
            alpha,
        ));
        ctx.begin_path();
        ctx.rect(0.0, h - HEADER_H, w, HEADER_H - 1.0);
        ctx.fill();

        // Top divider line — 1px, full-width, in the shared separator colour
        // so a vertical stack of headers forms consistent section boundaries
        // matching any `Separator` widgets elsewhere in the UI.
        ctx.set_fill_color(v.separator);
        ctx.begin_path();
        ctx.rect(0.0, h - 1.0, w, 1.0);
        ctx.fill();

        // Triangle indicator (▶ collapsed, ▼ expanded).
        // In Y-up: the header row occupies y = h - HEADER_H .. h.
        let center_y = h - HEADER_H * 0.5;
        let tx = INDENT;
        let ts = TRIANGLE_SIZE * 0.5;
        ctx.set_fill_color(v.text_dim);
        ctx.begin_path();
        if self.open {
            // Pointing down (▼): triangle with point at bottom.
            ctx.move_to(tx, center_y + ts * 0.5);
            ctx.line_to(tx + ts * 2.0, center_y + ts * 0.5);
            ctx.line_to(tx + ts, center_y - ts * 0.8);
        } else {
            // Pointing right (▶): triangle with point to the right.
            ctx.move_to(tx, center_y + ts);
            ctx.line_to(tx, center_y - ts);
            ctx.line_to(tx + ts * 1.6, center_y);
        }
        ctx.fill();

        // Label colour — child paints itself via the framework's tree walk.
        self.children[0].set_label_color(v.text_color);

        // Content (children[1] when open) is painted by the framework via
        // normal child recursion.
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        let h = self.bounds.height;

        match event {
            Event::MouseMove { pos } => {
                // Header row: top portion in Y-up = y from (h - HEADER_H) to h.
                let in_header = pos.x >= 0.0
                    && pos.x <= self.bounds.width
                    && pos.y >= h - HEADER_H
                    && pos.y <= h;
                let was = self.hovered;
                self.hovered = in_header;
                if self.hovered != was {
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                let in_header = pos.x >= 0.0
                    && pos.x <= self.bounds.width
                    && pos.y >= h - HEADER_H
                    && pos.y <= h;
                if in_header {
                    self.open = !self.open;
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }
}
