//! Shared fixtures for the finger-panning tests (`scrollable_touch_pan`,
//! `touch_tap_tolerance`): a fixed-size board placing children at fixed
//! rects (agg-sharp's `GuiWidget` with `OriginRelativeParent`), a probe
//! standing in for a clickable `GuiWidget` (counts presses and clicks as
//! agg-sharp's `MouseDown` and `Click` events do), and touch helpers that
//! drive [`App`]'s touch entry points the way the touch shells do.
//!
//! Positions are agg-sharp's: Y-up in a window `height` tall, in the
//! physical pixels touches arrive in; [`screen`] flips them to the Y-down
//! screen space of `App::on_touch_*`.

use std::cell::Cell;
use std::rc::Rc;

use crate::touch_state::{TouchDeviceId, TouchId};
use crate::{App, DrawCtx, Event, EventResult, MouseButton, Rect, ScrollView, Size, Widget};

pub const DEV: TouchDeviceId = TouchDeviceId(0);
pub const FINGER: TouchId = TouchId(1);
pub const SECOND_FINGER: TouchId = TouchId(2);

/// A fixed-size widget whose children sit at fixed rects (Y-up, local).
pub struct Board {
    bounds: Rect,
    size: Size,
    rects: Vec<Rect>,
    children: Vec<Box<dyn Widget>>,
}

impl Board {
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            bounds: Rect::default(),
            size: Size::new(width, height),
            rects: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn place(mut self, rect: Rect, child: Box<dyn Widget>) -> Self {
        self.rects.push(rect);
        self.children.push(child);
        self
    }
}

impl Widget for Board {
    crate::widgets::widget_as_any!();

    fn type_name(&self) -> &'static str {
        "Board"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, _available: Size) -> Size {
        for (child, rect) in self.children.iter_mut().zip(&self.rects) {
            child.layout(Size::new(rect.width, rect.height));
            child.set_bounds(*rect);
        }
        self.size
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Presses and clicks a [`Probe`] has seen.
#[derive(Default)]
pub struct Counts {
    pub presses: Cell<u32>,
    pub clicks: Cell<u32>,
}

/// A clickable leaf: a left press is a press (and takes the capture), a
/// left release inside it after its press is a click, and a lost capture
/// ends the press without one.
pub struct Probe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    pressed: bool,
    counts: Rc<Counts>,
}

impl Probe {
    pub fn new(counts: Rc<Counts>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            pressed: false,
            counts,
        }
    }
}

impl Widget for Probe {
    fn type_name(&self) -> &'static str {
        "Probe"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown {
                button: MouseButton::Left,
                ..
            } => {
                self.pressed = true;
                self.counts.presses.set(self.counts.presses.get() + 1);
                EventResult::Consumed
            }
            Event::MouseUp {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                let inside = pos.x >= 0.0
                    && pos.y >= 0.0
                    && pos.x < self.bounds.width
                    && pos.y < self.bounds.height;
                if self.pressed && inside {
                    self.counts.clicks.set(self.counts.clicks.get() + 1);
                }
                self.pressed = false;
                EventResult::Consumed
            }
            Event::MouseCaptureLost => {
                self.pressed = false;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

/// Reset the thread's scale and touch state so tests start alike.
pub fn reset(device_scale: f64) {
    crate::set_device_scale(device_scale);
    crate::ux_scale::set_ux_scale(1.0);
    crate::touch_state::clear_last_touch_event_for_testing();
}

/// The `ScrollView` at `path` (child indices from the root).
pub fn scroll_at<'a>(app: &'a mut App, path: &[usize]) -> &'a mut ScrollView {
    let mut w: &mut dyn Widget = app.root_mut();
    for &i in path {
        w = w.children_mut()[i].as_mut();
    }
    w.as_any_mut()
        .and_then(|a| a.downcast_mut::<ScrollView>())
        .expect("a ScrollView at the path")
}

/// A Y-up window position `(x, y)` as Y-down screen space.
pub fn screen(window_height: f64, x: f64, y: f64) -> (f64, f64) {
    (x, window_height - y)
}

/// Touch helpers for one window `height` tall (physical pixels).
pub struct Finger {
    pub height: f64,
}

impl Finger {
    pub fn down(&self, app: &mut App, x: f64, y: f64) {
        self.down_id(app, FINGER, x, y);
    }
    pub fn down_id(&self, app: &mut App, id: TouchId, x: f64, y: f64) {
        let (sx, sy) = screen(self.height, x, y);
        app.on_touch_start(DEV, id, sx, sy, None);
    }
    pub fn move_to(&self, app: &mut App, x: f64, y: f64) {
        self.move_id(app, FINGER, x, y);
    }
    pub fn move_id(&self, app: &mut App, id: TouchId, x: f64, y: f64) {
        let (sx, sy) = screen(self.height, x, y);
        app.on_touch_move(DEV, id, sx, sy, None);
    }
    pub fn up(&self, app: &mut App) {
        app.on_touch_end(DEV, FINGER);
    }
    /// Press, one move, release: agg-sharp's `Drag` helper.
    pub fn drag(&self, app: &mut App, from: (f64, f64), to: (f64, f64)) {
        self.down(app, from.0, from.1);
        self.move_to(app, to.0, to.1);
        self.up(app);
    }
}
