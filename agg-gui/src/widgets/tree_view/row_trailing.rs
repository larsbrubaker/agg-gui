//! `FractionBar` — the small horizontal share bar a `TreeRow` can show at
//! its trailing edge (`TreeNode::fraction`), e.g. a folder's share of its
//! parent's size.  Display-only; sized by `TreeRow::layout` in `row.rs`.

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::widget::Widget;

/// Width of the bar (logical pixels).
pub const FRACTION_BAR_W: f64 = 40.0;
/// Height of the bar.
const FRACTION_BAR_H: f64 = 6.0;

/// A track filled to `fraction` (clamped to `0..=1`) with the accent colour.
pub struct FractionBar {
    bounds: Rect,
    fraction: f64,
    children: Vec<Box<dyn Widget>>,
}

impl FractionBar {
    pub fn new(fraction: f32) -> Self {
        let f = f64::from(fraction);
        Self {
            bounds: Rect::default(),
            fraction: if f.is_finite() {
                f.clamp(0.0, 1.0)
            } else {
                0.0
            },
            children: Vec::new(),
        }
    }
}

impl Widget for FractionBar {
    fn type_name(&self) -> &'static str {
        "FractionBar"
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
    fn hit_test(&self, _: crate::geometry::Point) -> bool {
        false
    }

    fn layout(&mut self, available: Size) -> Size {
        Size::new(FRACTION_BAR_W.min(available.width), available.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let y = ((self.bounds.height - FRACTION_BAR_H) * 0.5).max(0.0);
        let v = ctx.visuals();
        let track = Color::rgba(v.text_color.r, v.text_color.g, v.text_color.b, 0.12);
        let accent = v.accent;
        ctx.set_fill_color(track);
        ctx.begin_path();
        ctx.rounded_rect(0.0, y, w, FRACTION_BAR_H, 2.0);
        ctx.fill();
        let fill_w = w * self.fraction;
        if fill_w > 0.5 {
            ctx.set_fill_color(accent);
            ctx.begin_path();
            ctx.rounded_rect(0.0, y, fill_w, FRACTION_BAR_H, 2.0);
            ctx.fill();
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
