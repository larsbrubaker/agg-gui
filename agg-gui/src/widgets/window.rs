//! `Window` — a floating, draggable panel with a title bar.
//!
//! # Usage
//!
//! Create a `Window` and place it as the **last** child of a [`Stack`] so it
//! paints on top of everything and receives hit-test priority.
//!
//! ```ignore
//! let win = Window::new("Inspector", font, Box::new(my_content));
//! Stack::new()
//!     .add(Box::new(main_ui))
//!     .add(Box::new(win))
//! ```
//!
//! # Coordinate notes (Y-up)
//!
//! `bounds` stores the window's position in its **parent's** coordinate space.
//! The title bar is at the **top** of the window, i.e. local Y ∈
//! `[height − TITLE_H .. height]`. The content area fills local Y ∈ `[0 .. height − TITLE_H]`.
//!
//! Drag uses world-space anchoring: `drag_start_world = bounds.xy + click_local`,
//! `drag_start_bounds = bounds at click time`. Every subsequent MouseMove
//! re-derives world pos (`pos + current_bounds.xy`) and applies the offset, so
//! the dragged point stays exactly under the cursor even as the window moves.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::text::Font;
use crate::widget::Widget;

const TITLE_H: f64 = 28.0;
const CORNER_R: f64 = 8.0;
const SHADOW_BLUR: f64 = 6.0; // extra size of shadow rect on each side
const CLOSE_R: f64 = 6.0;     // radius of close button circle
const CLOSE_PAD: f64 = 10.0;  // padding from right edge to close button center

/// A floating panel with a draggable title bar and a single content child.
pub struct Window {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always exactly 1: the content

    title: String,
    font: Arc<Font>,
    font_size: f64,

    visible: bool,

    dragging: bool,
    /// Cursor world position when drag started.
    drag_start_world: Point,
    /// Window bounds when drag started.
    drag_start_bounds: Rect,

    close_hovered: bool,
}

impl Window {
    /// Create a new window with the given title, font, and content widget.
    ///
    /// Default position: `(60, 60)` with `size = (360, 280)`. Call
    /// [`with_bounds`] to override.
    pub fn new(title: impl Into<String>, font: Arc<Font>, content: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::new(60.0, 60.0, 360.0, 280.0),
            children: vec![content],
            title: title.into(),
            font,
            font_size: 13.0,
            visible: true,
            dragging: false,
            drag_start_world: Point::ORIGIN,
            drag_start_bounds: Rect::default(),
            close_hovered: false,
        }
    }

    pub fn with_bounds(mut self, b: Rect) -> Self { self.bounds = b; self }
    pub fn with_font_size(mut self, size: f64) -> Self { self.font_size = size; self }

    pub fn show(&mut self) { self.visible = true; }
    pub fn hide(&mut self) { self.visible = false; }
    pub fn toggle(&mut self) { self.visible = !self.visible; }
    pub fn is_visible(&self) -> bool { self.visible }

    // Local Y of the title bar bottom edge (Y-up: title bar top = height).
    fn title_bar_bottom(&self) -> f64 {
        self.bounds.height - TITLE_H
    }

    fn in_title_bar(&self, local: Point) -> bool {
        local.y >= self.title_bar_bottom() && local.y <= self.bounds.height
            && local.x >= 0.0 && local.x <= self.bounds.width
    }

    // Center of the close button in local coords.
    fn close_center(&self) -> Point {
        Point::new(
            self.bounds.width - CLOSE_PAD,
            self.bounds.height - TITLE_H * 0.5,
        )
    }

    fn in_close_button(&self, local: Point) -> bool {
        let c = self.close_center();
        let dx = local.x - c.x;
        let dy = local.y - c.y;
        dx * dx + dy * dy <= (CLOSE_R + 3.0) * (CLOSE_R + 3.0)
    }
}

impl Widget for Window {
    fn type_name(&self) -> &'static str { "Window" }
    fn bounds(&self) -> Rect { self.bounds }

    fn set_bounds(&mut self, b: Rect) {
        // Preserve our position — only update size if zero (first call from Stack layout).
        if self.bounds.width == 0.0 || self.bounds.height == 0.0 {
            self.bounds = b;
        }
        // Otherwise keep our self-managed position unchanged.
    }

    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn hit_test(&self, local_pos: Point) -> bool {
        if !self.visible { return false; }
        // Keep capturing during drag even when cursor leaves.
        if self.dragging { return true; }
        let b = self.bounds();
        local_pos.x >= 0.0 && local_pos.x <= b.width
            && local_pos.y >= 0.0 && local_pos.y <= b.height
    }

    fn layout(&mut self, _available: Size) -> Size {
        if !self.visible {
            return Size::new(self.bounds.width, self.bounds.height);
        }
        let content_h = (self.bounds.height - TITLE_H).max(0.0);
        if let Some(child) = self.children.first_mut() {
            child.layout(Size::new(self.bounds.width, content_h));
            // Content sits at the bottom of the window (Y-up: y=0).
            child.set_bounds(Rect::new(0.0, 0.0, self.bounds.width, content_h));
        }
        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.visible { return; }

        let w = self.bounds.width;
        let h = self.bounds.height;
        let tb = self.title_bar_bottom();

        // Shadow (painted slightly offset and larger, semi-transparent).
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.18));
        ctx.begin_path();
        ctx.rounded_rect(
            SHADOW_BLUR, -SHADOW_BLUR,
            w + SHADOW_BLUR, h + SHADOW_BLUR,
            CORNER_R,
        );
        ctx.fill();

        // Window body background (content area).
        ctx.set_fill_color(Color::rgb(0.97, 0.97, 0.98));
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, CORNER_R);
        ctx.fill();

        // Title bar.
        let bar_color = if self.dragging {
            Color::rgb(0.22, 0.22, 0.26)
        } else {
            Color::rgb(0.28, 0.28, 0.32)
        };
        ctx.set_fill_color(bar_color);
        ctx.begin_path();
        // Draw only the top-rounded portion for the title bar.
        // We paint a full rounded rect then cover the bottom corners with a plain rect.
        ctx.rounded_rect(0.0, tb, w, TITLE_H, CORNER_R);
        ctx.fill();
        // Square off the bottom edge of the title bar.
        ctx.set_fill_color(bar_color);
        ctx.begin_path();
        ctx.rect(0.0, tb, w, CORNER_R);
        ctx.fill();

        // Thin separator line between title bar and content.
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.15));
        ctx.begin_path();
        ctx.rect(0.0, tb - 1.0, w, 1.0);
        ctx.fill();

        // Title text.
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        ctx.set_fill_color(Color::rgba(1.0, 1.0, 1.0, 0.90));
        let title_cy = tb + TITLE_H * 0.5;
        if let Some(m) = ctx.measure_text(&self.title) {
            let tx = 12.0;
            let ty = title_cy - (m.ascent - m.descent) * 0.5 + m.descent;
            ctx.fill_text(&self.title, tx, ty);
        }

        // Close button.
        let cc = self.close_center();
        let close_bg = if self.close_hovered {
            Color::rgba(1.0, 1.0, 1.0, 0.25)
        } else {
            Color::rgba(1.0, 1.0, 1.0, 0.12)
        };
        ctx.set_fill_color(close_bg);
        ctx.begin_path();
        ctx.circle(cc.x, cc.y, CLOSE_R);
        ctx.fill();

        // × glyph on close button.
        let arm = CLOSE_R * 0.5;
        ctx.set_stroke_color(Color::rgba(1.0, 1.0, 1.0, 0.80));
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.move_to(cc.x - arm, cc.y - arm);
        ctx.line_to(cc.x + arm, cc.y + arm);
        ctx.stroke();
        ctx.begin_path();
        ctx.move_to(cc.x + arm, cc.y - arm);
        ctx.line_to(cc.x - arm, cc.y + arm);
        ctx.stroke();

        // Thin border around the whole window.
        ctx.set_stroke_color(Color::rgba(0.0, 0.0, 0.0, 0.15));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, CORNER_R);
        ctx.stroke();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if !self.visible { return EventResult::Ignored; }

        match event {
            Event::MouseMove { pos } => {
                self.close_hovered = self.in_close_button(*pos);

                if self.dragging {
                    // Derive world position from local pos + current bounds.
                    let world = Point::new(pos.x + self.bounds.x, pos.y + self.bounds.y);
                    let dx = world.x - self.drag_start_world.x;
                    let dy = world.y - self.drag_start_world.y;
                    self.bounds.x = self.drag_start_bounds.x + dx;
                    self.bounds.y = self.drag_start_bounds.y + dy;
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }

            Event::MouseDown { button: MouseButton::Left, pos, .. } => {
                if self.in_close_button(*pos) {
                    // Close button: hide the window.
                    self.visible = false;
                    return EventResult::Consumed;
                }
                if self.in_title_bar(*pos) {
                    self.dragging = true;
                    self.drag_start_world = Point::new(
                        pos.x + self.bounds.x,
                        pos.y + self.bounds.y,
                    );
                    self.drag_start_bounds = self.bounds;
                    return EventResult::Consumed;
                }
                // Click on content area: consume so it doesn't fall through.
                EventResult::Consumed
            }

            Event::MouseUp { button: MouseButton::Left, .. } => {
                let was_dragging = self.dragging;
                self.dragging = false;
                if was_dragging { EventResult::Consumed } else { EventResult::Ignored }
            }

            _ => EventResult::Ignored,
        }
    }
}
