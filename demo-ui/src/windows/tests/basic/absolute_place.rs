//! `AbsolutePlace` — a single-child container that positions its child at an
//! arbitrary rect inside a bounded canvas.
//!
//! Used by the Manual Layout Test (`layout.rs`) to demonstrate absolute
//! placement, mirroring egui's `ui.put(rect, widget)` from
//! `tests/manual_layout_test.rs`. The child is a real, interactive widget
//! (button / label / text edit); the framework routes pointer events to it by
//! its bounds, so it stays clickable / editable wherever it is placed.
//!
//! Placement is expressed with a top-left-origin offset (matching egui's
//! `widget_offset`) but resolved into agg-gui's Y-up coordinate space by
//! [`placed_child_rect`], which is unit-tested below.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::{Color, DrawCtx, Event, EventResult, Rect, Size, Widget};

/// Resolve a child rect inside a container of height `container_h`.
///
/// `x` / `y_from_top` are a top-left-origin offset (as a user reading the
/// sliders expects: y grows downward from the canvas top). agg-gui is Y-up, so
/// the container's top edge is at `container_h`; a child `y_from_top` pixels
/// below the top has its top edge at `container_h - y_from_top` and its
/// bottom-left corner (the `Rect` origin) at `container_h - y_from_top - h`.
pub(crate) fn placed_child_rect(container_h: f64, x: f64, y_from_top: f64, w: f64, h: f64) -> Rect {
    Rect::new(x, container_h - y_from_top - h, w, h)
}

/// A bounded canvas that lays out its single child at a live rect read from
/// shared cells. Sliders drive the cells, so dragging them re-places the child
/// each frame without rebuilding it.
pub(crate) struct AbsolutePlace {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    x: Rc<Cell<f64>>,
    y: Rc<Cell<f64>>,
    w: Rc<Cell<f64>>,
    h: Rc<Cell<f64>>,
}

impl AbsolutePlace {
    pub(crate) fn new(
        child: Box<dyn Widget>,
        x: Rc<Cell<f64>>,
        y: Rc<Cell<f64>>,
        w: Rc<Cell<f64>>,
        h: Rc<Cell<f64>>,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![child],
            x,
            y,
            w,
            h,
        }
    }
}

impl Widget for AbsolutePlace {
    fn type_name(&self) -> &'static str {
        "AbsolutePlace"
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
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        let (x, y, w, h) = (self.x.get(), self.y.get(), self.w.get(), self.h.get());
        if let Some(child) = self.children.first_mut() {
            child.layout(Size::new(w, h));
            child.set_bounds(placed_child_rect(available.height, x, y, w, h));
        }
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        // Canvas background + border so the placement area is visible.
        ctx.set_fill_color(v.bg_color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.stroke();
        // Silence unused-import warning when no branch constructs a Color.
        let _ = Color::white();
        // The child paints itself via the framework tree walk.
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        // Events are routed to the child by the framework based on its bounds.
        EventResult::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::placed_child_rect;

    #[test]
    fn child_is_placed_from_the_top_left_in_y_up_space() {
        // Container 300 tall. Offset 20 from the top, size 50×40.
        // Top edge sits at 300 - 20 = 280; bottom-left origin at 280 - 40 = 240.
        let r = placed_child_rect(300.0, 10.0, 20.0, 50.0, 40.0);
        assert_eq!(r.x, 10.0);
        assert_eq!(r.y, 240.0);
        assert_eq!(r.width, 50.0);
        assert_eq!(r.height, 40.0);
    }

    #[test]
    fn zero_offset_pins_child_to_the_top_edge() {
        // y_from_top = 0 → child top flush with canvas top (Y-up: highest y).
        let r = placed_child_rect(200.0, 0.0, 0.0, 30.0, 30.0);
        // Bottom-left origin is container_h - h.
        assert_eq!(r.y, 170.0);
        // Child top edge = origin.y + height = 200 (the canvas top).
        assert_eq!(r.y + r.height, 200.0);
    }
}
