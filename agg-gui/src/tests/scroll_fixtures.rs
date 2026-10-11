//! Fixtures shared by the ScrollView ports of agg-sharp's Agg.UI scroll test
//! classes (`horizontal_wheel_routing`, `wheel_occlusion_routing`,
//! `scrollable_widget`): a host that gives its one child a fixed size (the
//! fixed `Width`/`Height` a C# `GuiWidget(w, h)` has), and a wrapper that
//! records the wheel deltas that reach it.
//!
//! Wheel units: agg-sharp packs a trackpad's travel as 5 wheel units per
//! logical pixel, and agg-gui's wheel carries notches of
//! [`PIXELS_PER_NOTCH`](crate::wheel::PIXELS_PER_NOTCH) pixels, so
//! [`wheel_units_to_notches`] is `units / 5 / 40`.

use std::cell::Cell;
use std::rc::Rc;

use crate::geometry::Point;
use crate::{DrawCtx, Event, EventResult, Rect, Size, Widget};

/// agg-sharp wheel units (5 per pixel of finger travel) → agg-gui notches.
pub(super) fn wheel_units_to_notches(units: f64) -> f64 {
    units / 5.0 / crate::wheel::PIXELS_PER_NOTCH
}

/// A widget of a fixed size holding at most one child laid out to fill it.
pub(super) struct Fixed {
    bounds: Rect,
    size: Size,
    children: Vec<Box<dyn Widget>>,
}

impl Fixed {
    pub(super) fn new(width: f64, height: f64, child: Option<Box<dyn Widget>>) -> Self {
        Self {
            bounds: Rect::default(),
            size: Size::new(width, height),
            children: child.into_iter().collect(),
        }
    }
}

impl Widget for Fixed {
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
    fn layout(&mut self, _available: Size) -> Size {
        let s = self.size;
        for child in &mut self.children {
            let _ = child.layout(s);
            child.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        }
        s
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn hit_test(&self, p: Point) -> bool {
        p.x >= 0.0 && p.y >= 0.0 && p.x <= self.bounds.width && p.y <= self.bounds.height
    }
}

/// What a [`Recorder`] saw of the last wheel event: `(delta_x, delta_y)`.
pub(super) type SeenWheel = Rc<Cell<Option<(f64, f64)>>>;

/// Fills its parent, holds one child placed at `child_origin` with
/// `child_size`, and records the wheel deltas that bubble to it. When
/// `take_x` is set it takes the horizontal axis, as agg-sharp's
/// `HorizontalWheelEater` zeroes `WheelDeltaX`.
pub(super) struct Recorder {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    child_rect: Rect,
    pub(super) seen: SeenWheel,
    take_x: bool,
}

impl Recorder {
    pub(super) fn new(child: Box<dyn Widget>, child_rect: Rect, take_x: bool) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![child],
            child_rect,
            seen: Rc::new(Cell::new(None)),
            take_x,
        }
    }
}

impl Widget for Recorder {
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
        let r = self.child_rect;
        for child in &mut self.children {
            let _ = child.layout(Size::new(r.width, r.height));
            child.set_bounds(r);
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, e: &Event) -> EventResult {
        if let Event::MouseWheel {
            delta_x, delta_y, ..
        } = e
        {
            self.seen.set(Some((*delta_x, *delta_y)));
            if self.take_x {
                crate::wheel_axes::take_wheel_axes(true, false);
            }
        }
        EventResult::Ignored
    }
    fn hit_test(&self, p: Point) -> bool {
        p.x >= 0.0 && p.y >= 0.0 && p.x <= self.bounds.width && p.y <= self.bounds.height
    }
}
