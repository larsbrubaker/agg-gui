//! `Tooltip` — a wrapper widget that shows a hover tooltip.
//!
//! Wraps any child widget and renders a small info panel near the cursor when
//! the user hovers over the child.  The panel appears after a short delay
//! (`HOVER_DELAY_FRAMES`) and is rendered inline within the widget's own `paint()`
//! call, which means it renders within the widget's local clip region.
//!
//! # Limitations
//!
//! Because the tooltip is painted in local coordinate space it will be clipped by
//! the parent `ScrollView` if the child is near the edge.  True floating tooltips
//! require a global overlay layer.  This implementation covers the common case
//! where the tooltip fits within the visible widget area.
//!
//! # Usage
//!
//! ```ignore
//! Tooltip::new(
//!     Box::new(Button::new("Hover me", font.clone()).on_click(|| {})),
//!     "This is a tooltip",
//!     font.clone(),
//! )
//! ```

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{Widget, paint_subtree};
use crate::widgets::label::Label;

/// Number of consecutive hovered frames before the tooltip appears.
/// At ~60 fps this gives a ~0.5 second delay.
const HOVER_DELAY_FRAMES: u32 = 30;

/// A wrapper widget that shows a text tooltip on hover.
///
/// The tooltip panel is drawn above the child widget (Y-up: higher Y = visually above).
/// The text is rendered through a backbuffered [`Label`] child.
pub struct Tooltip {
    bounds:   Rect,
    /// The wrapped child widget is stored in `children[0]`.
    children: Vec<Box<dyn Widget>>,
    base:     WidgetBase,

    /// Hover-frame counter: increments while cursor is over the child.
    hover_frames: u32,
    /// Whether the cursor is currently inside the widget bounds.
    hovered: bool,
    /// Last known cursor position in local coordinates.
    cursor: Point,

    /// Backbuffered label for the tooltip text.
    tip_label: Label,
}

impl Tooltip {
    /// Create a new `Tooltip` wrapping `child` with `text` as the tip message.
    pub fn new(child: Box<dyn Widget>, text: impl Into<String>, font: Arc<Font>) -> Self {
        Self {
            bounds:       Rect::default(),
            children:     vec![child],
            base:         WidgetBase::new(),
            hover_frames: 0,
            hovered:      false,
            cursor:       Point::ORIGIN,
            tip_label:    Label::new(text, font).with_font_size(11.5),
        }
    }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }

    fn show_tip(&self) -> bool {
        self.hovered && self.hover_frames >= HOVER_DELAY_FRAMES
    }
}

impl Widget for Tooltip {
    fn type_name(&self) -> &'static str { "Tooltip" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }

    fn is_focusable(&self) -> bool {
        self.children.first().map(|c| c.is_focusable()).unwrap_or(false)
    }

    fn layout(&mut self, available: Size) -> Size {
        let s = if let Some(child) = self.children.first_mut() {
            let cs = child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, cs.width, cs.height));
            cs
        } else {
            available
        };
        self.bounds = Rect::new(0.0, 0.0, s.width, s.height);
        s
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        // Increment hover counter each paint frame while hovered.
        if self.hovered {
            self.hover_frames = self.hover_frames.saturating_add(1);
        }

        // Paint the wrapped child widget.
        if let Some(child) = self.children.first_mut() {
            paint_subtree(child.as_mut(), ctx);
        }

        // Draw tooltip panel if hovered long enough.
        if !self.show_tip() { return; }

        let v = ctx.visuals();
        let pad_x = 8.0_f64;
        let pad_y = 5.0_f64;

        // Layout the label.
        let max_tip_w = self.bounds.width.max(120.0).min(260.0);
        let ls = self.tip_label.layout(Size::new(max_tip_w, 100.0));

        let panel_w = ls.width  + pad_x * 2.0;
        let panel_h = ls.height + pad_y * 2.0;

        // Position panel above the cursor (Y-up: above = larger Y).
        let cursor_y = self.cursor.y;
        let cursor_x = self.cursor.x;
        let panel_x = (cursor_x - panel_w * 0.5).max(0.0).min(self.bounds.width - panel_w);
        let panel_y = cursor_y + 10.0; // 10px above cursor in Y-up space

        // Draw tooltip shadow.
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.20));
        ctx.begin_path();
        ctx.rounded_rect(panel_x + 1.0, panel_y - 1.0, panel_w, panel_h, 4.0);
        ctx.fill();

        // Draw tooltip panel background.
        ctx.set_fill_color(v.window_fill);
        ctx.begin_path();
        ctx.rounded_rect(panel_x, panel_y, panel_w, panel_h, 4.0);
        ctx.fill();

        // Border.
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(panel_x, panel_y, panel_w, panel_h, 4.0);
        ctx.stroke();

        // Paint the label.
        self.tip_label.set_color(v.text_color);
        self.tip_label.set_bounds(Rect::new(0.0, 0.0, ls.width, ls.height));
        let lx = panel_x + pad_x;
        let ly = panel_y + pad_y;
        ctx.save();
        ctx.translate(lx, ly);
        paint_subtree(&mut self.tip_label, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                self.cursor = *pos;
                if !self.hovered {
                    self.hover_frames = 0;
                }
                // Forward to child.
                let result = if let Some(child) = self.children.first_mut() {
                    child.on_event(event)
                } else {
                    EventResult::Ignored
                };
                // If hover state changed, consume to trigger a repaint.
                if self.hovered != was { EventResult::Consumed } else { result }
            }
            _ => {
                if let Some(child) = self.children.first_mut() {
                    child.on_event(event)
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0 && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0 && local_pos.y <= self.bounds.height
    }
}
