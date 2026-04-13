//! `Splitter` — draggable divider between two side-by-side children.
//!
//! Phase 5: horizontal split only (left panel | right panel).

use crate::color::Color;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::widget::Widget;

/// A draggable divider that splits its two children horizontally.
///
/// `children[0]` = left panel, `children[1]` = right panel.
pub struct Splitter {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,  // exactly 2
    /// Split position as a fraction of total width. Clamped to [0.05, 0.95].
    pub ratio: f64,
    /// Width of the draggable divider strip.
    pub divider_width: f64,

    hovered: bool,
    dragging: bool,
}

impl Splitter {
    pub fn new(left: Box<dyn Widget>, right: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![left, right],
            ratio: 0.5,
            divider_width: 6.0,
            hovered: false,
            dragging: false,
        }
    }

    pub fn with_ratio(mut self, ratio: f64) -> Self {
        self.ratio = ratio.clamp(0.05, 0.95);
        self
    }

    pub fn with_divider_width(mut self, w: f64) -> Self {
        self.divider_width = w;
        self
    }

    fn divider_x(&self) -> f64 {
        (self.bounds.width - self.divider_width) * self.ratio
    }
}

impl Widget for Splitter {
    fn type_name(&self) -> &'static str { "Splitter" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn hit_test(&self, local_pos: Point) -> bool {
        // Capture all events during drag, even if cursor leaves bounds.
        if self.dragging { return true; }
        let b = self.bounds();
        local_pos.x >= 0.0 && local_pos.x <= b.width
            && local_pos.y >= 0.0 && local_pos.y <= b.height
    }

    fn layout(&mut self, available: Size) -> Size {
        let div = self.divider_width;
        let left_w = ((available.width - div) * self.ratio).max(0.0);
        let right_w = (available.width - div - left_w).max(0.0);
        let h = available.height;

        if self.children.len() >= 2 {
            self.children[0].layout(Size::new(left_w, h));
            self.children[0].set_bounds(Rect::new(0.0, 0.0, left_w, h));

            let right_x = left_w + div;
            self.children[1].layout(Size::new(right_w, h));
            self.children[1].set_bounds(Rect::new(right_x, 0.0, right_w, h));
        }

        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let div_x = self.divider_x();
        let h = self.bounds.height;

        let color = if self.dragging {
            Color::rgba(0.22, 0.45, 0.88, 0.6)
        } else if self.hovered {
            Color::rgba(0.0, 0.0, 0.0, 0.15)
        } else {
            Color::rgba(0.0, 0.0, 0.0, 0.08)
        };
        ctx.set_fill_color(color);
        ctx.begin_path();
        ctx.rect(div_x, 0.0, self.divider_width, h);
        ctx.fill();

        // Grip dots in the center of the divider
        if h > 30.0 {
            let grip_color = if self.hovered || self.dragging {
                Color::rgba(0.22, 0.45, 0.88, 0.7)
            } else {
                Color::rgba(0.0, 0.0, 0.0, 0.25)
            };
            ctx.set_fill_color(grip_color);
            let cx = div_x + self.divider_width * 0.5;
            let cy = h * 0.5;
            for i in -1i32..=1 {
                ctx.begin_path();
                ctx.circle(cx, cy + i as f64 * 5.0, 1.5);
                ctx.fill();
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        let div_x = self.divider_x();
        let div_end = div_x + self.divider_width;

        match event {
            Event::MouseMove { pos } => {
                let over_div = pos.x >= div_x - 2.0 && pos.x <= div_end + 2.0;
                self.hovered = over_div;
                if self.dragging {
                    let total = self.bounds.width;
                    if total > self.divider_width {
                        self.ratio = (pos.x / total).clamp(0.05, 0.95);
                    }
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                if pos.x >= div_x - 2.0 && pos.x <= div_end + 2.0 {
                    self.dragging = true;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
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
