//! Momentum ("kinetic") scrolling for touch pans in a [`ScrollView`].
//!
//! A finger that flicks the content and lifts while still moving throws it:
//! the content keeps going at the finger's speed and slows to a stop, the way
//! lists scroll on every phone. This file holds the three parts:
//!
//! - [`VelocityTracker`]: the finger's recent positions during a touch pan
//!   (`widget_impl.rs` feeds it from the pan's moves) and the velocity they
//!   give at the lift — a least-squares fit over the last
//!   [`VELOCITY_WINDOW`], and none at all when the finger stood still (within
//!   [`STILL_SLOP`]) for [`PAUSE_BEFORE_LIFT`] before lifting, so
//!   stop-then-lift stays put.
//! - [`Fling`]: the throw itself, an exponential decay with time constant
//!   [`FLING_TIME_CONSTANT`], evaluated in closed form from the lift on the UI
//!   clock ([`crate::clock`]), so where the content is depends only on the
//!   time since the lift, never on how often frames were drawn. Each axis
//!   stops on its own at the content's end (no rubber band).
//! - the `ScrollView` side: starting a fling at the lift, stepping it at each
//!   layout (and asking for the next layout only while it moves, so an idle
//!   app sleeps once it stops), and stopping it for anything else that
//!   scrolls the view — a finger landing on it (`crate::touch_emulation::
//!   fling`), a press, the wheel, or a programmatic scroll.
//!
//! Units are the view's logical pixels and seconds. Only touch pans fling:
//! a mouse middle-drag and the wheel behave as they always did.

use std::collections::VecDeque;
use std::time::Duration;

use web_time::Instant;

use crate::geometry::{Point, Rect};
use crate::touch_emulation::fling::FlingRegistration;

use super::ScrollView;

/// How far back the velocity at the lift looks. Long enough to span several
/// touch events at 60 Hz, short enough to read the flick, not the whole drag.
pub const VELOCITY_WINDOW: Duration = Duration::from_millis(100);

/// A finger that sent no move for this long before lifting had stopped:
/// lifting then does not fling.
pub const PAUSE_BEFORE_LIFT: Duration = Duration::from_millis(40);

/// A finger that stays within this many logical pixels is at rest (the
/// jitter of a held finger), for [`PAUSE_BEFORE_LIFT`].
pub const STILL_SLOP: f64 = 2.0;

/// The shortest span of moves a velocity is read from. Two moves closer in
/// time than any touch screen samples are one event replayed, not motion.
pub const MIN_SAMPLE_SPAN: Duration = Duration::from_millis(4);

/// Slower than this (logical pixels per second) at the lift is a placed
/// finger, not a flick: the content stops where the finger left it.
pub const MIN_FLING_SPEED: f64 = 50.0;

/// Fastest throw, logical pixels per second; a faster flick is capped here
/// so one wild event cannot throw the content to the far end at once.
pub const MAX_FLING_SPEED: f64 = 8000.0;

/// Time constant of the slow-down, in seconds: the speed falls to 1/e of the
/// throw every 325 ms, and the content travels `speed x 0.325 s` in all. That
/// sits between Android's default fling and iOS's normal deceleration (0.998
/// per millisecond, a 500 ms constant), the feel most kinetic scrollers use.
pub const FLING_TIME_CONSTANT: f64 = 0.325;

/// A fling stops once what is left of its travel is under this many logical
/// pixels: the rest of the decay would only redraw the same pixel row.
pub const FLING_STOP_DISTANCE: f64 = 0.5;

/// Most samples kept: a 1 kHz digitiser fills the window with 100; anything
/// older than the window is dropped first anyway.
const MAX_SAMPLES: usize = 128;

/// The finger's recent positions (logical, Y-up world space) during a pan.
#[derive(Default)]
pub struct VelocityTracker {
    samples: VecDeque<(Instant, Point)>,
}

impl VelocityTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget every sample (a new pan began).
    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// The finger was at `pos` at `at`.
    pub fn add(&mut self, at: Instant, pos: Point) {
        while self.samples.len() >= MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back((at, pos));
    }

    /// The finger's velocity (logical pixels per second, Y-up) when it lifts
    /// at `lift`: the least-squares slope of the samples in the
    /// [`VELOCITY_WINDOW`] before the lift. Zero when the finger paused for
    /// [`PAUSE_BEFORE_LIFT`] first, or when too little motion was seen to
    /// tell (fewer than two samples, or spanning under [`MIN_SAMPLE_SPAN`]).
    pub fn velocity_at(&self, lift: Instant) -> (f64, f64) {
        let Some(&(newest, last_pos)) = self.samples.back() else {
            return (0.0, 0.0);
        };
        // When the finger came to rest: the first of the trailing samples
        // that stay within `STILL_SLOP` of where it lifted. A platform that
        // keeps sending moves of a resting finger, and one that sends none,
        // both read as a pause.
        let resting_since = self
            .samples
            .iter()
            .rev()
            .take_while(|(_, pos)| {
                let (dx, dy) = (pos.x - last_pos.x, pos.y - last_pos.y);
                (dx * dx + dy * dy).sqrt() <= STILL_SLOP
            })
            .last()
            .map_or(newest, |(at, _)| *at);
        if lift.saturating_duration_since(resting_since) > PAUSE_BEFORE_LIFT {
            return (0.0, 0.0);
        }
        let mut window = self
            .samples
            .iter()
            .filter(|(at, _)| lift.saturating_duration_since(*at) <= VELOCITY_WINDOW)
            .peekable();
        let Some(&&(oldest, _)) = window.peek() else {
            return (0.0, 0.0);
        };
        // Seconds since the window's oldest sample, with the position then.
        let recent: Vec<(f64, Point)> = window
            .map(|(at, pos)| (at.saturating_duration_since(oldest).as_secs_f64(), *pos))
            .collect();
        if recent.len() < 2 || newest.saturating_duration_since(oldest) < MIN_SAMPLE_SPAN {
            return (0.0, 0.0);
        }
        let n = recent.len() as f64;
        let mean_t = recent.iter().map(|(t, _)| t).sum::<f64>() / n;
        let mean_x = recent.iter().map(|(_, p)| p.x).sum::<f64>() / n;
        let mean_y = recent.iter().map(|(_, p)| p.y).sum::<f64>() / n;
        let mut var_t = 0.0;
        let mut cov_x = 0.0;
        let mut cov_y = 0.0;
        for (t, p) in &recent {
            let dt = t - mean_t;
            var_t += dt * dt;
            cov_x += dt * (p.x - mean_x);
            cov_y += dt * (p.y - mean_y);
        }
        if var_t <= 0.0 {
            return (0.0, 0.0);
        }
        (cov_x / var_t, cov_y / var_t)
    }
}

/// One axis of a throw: where it started and how fast (offset units per
/// second, in the direction the offset grows).
#[derive(Clone, Copy, Debug)]
struct AxisThrow {
    origin: f64,
    speed: f64,
}

impl AxisThrow {
    /// The offset `t` seconds after the lift, and the travel still to come.
    fn at(&self, t: f64) -> (f64, f64) {
        let decay = (-t / FLING_TIME_CONSTANT).exp();
        let total = self.speed * FLING_TIME_CONSTANT;
        (self.origin + total * (1.0 - decay), total * decay)
    }
}

/// A running throw of a [`ScrollView`]'s content.
pub struct Fling {
    start: Instant,
    v: Option<AxisThrow>,
    h: Option<AxisThrow>,
    registration: FlingRegistration,
}

/// What one step of a fling did to an axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlingStep {
    /// The new vertical offset, if the vertical axis moved.
    pub v: Option<f64>,
    /// The new horizontal offset, if the horizontal axis moved.
    pub h: Option<f64>,
    /// The fling is over: no axis has anything left to travel.
    pub done: bool,
}

impl Fling {
    /// A throw from the offsets `(v, h)` at `(v_speed, h_speed)` offset units
    /// per second, lifted at `start`, of a view that shows at `screen_rect`.
    /// `None` when the throw is slower than [`MIN_FLING_SPEED`]; a faster one
    /// than [`MAX_FLING_SPEED`] is capped there, keeping its direction.
    pub fn new(
        start: Instant,
        offsets: (Option<f64>, Option<f64>),
        speed: (f64, f64),
        screen_rect: Rect,
    ) -> Option<Self> {
        let v_speed = if offsets.0.is_some() { speed.0 } else { 0.0 };
        let h_speed = if offsets.1.is_some() { speed.1 } else { 0.0 };
        let magnitude = (v_speed * v_speed + h_speed * h_speed).sqrt();
        if !magnitude.is_finite() || magnitude < MIN_FLING_SPEED {
            return None;
        }
        let cap = (MAX_FLING_SPEED / magnitude).min(1.0);
        let axis = |origin: Option<f64>, speed: f64| {
            origin.filter(|_| speed != 0.0).map(|origin| AxisThrow {
                origin,
                speed: speed * cap,
            })
        };
        Some(Self {
            start,
            v: axis(offsets.0, v_speed),
            h: axis(offsets.1, h_speed),
            registration: FlingRegistration::new(screen_rect),
        })
    }

    /// Whether a finger landed on the view since the throw began.
    pub(crate) fn caught(&self) -> bool {
        self.registration.caught()
    }

    /// The offsets at `now`, clamped to `0..=max` per axis (`(v_max,
    /// h_max)`). An axis that reaches either end, or has under
    /// [`FLING_STOP_DISTANCE`] left to go, stops there and moves no more.
    pub fn step(&mut self, now: Instant, max: (f64, f64)) -> FlingStep {
        let t = now.saturating_duration_since(self.start).as_secs_f64();
        let v = step_axis(&mut self.v, t, max.0);
        let h = step_axis(&mut self.h, t, max.1);
        FlingStep {
            v,
            h,
            done: self.v.is_none() && self.h.is_none(),
        }
    }
}

/// Advance one axis to `t` seconds after the lift; `None` when it was
/// already still. Ends the axis at the content's ends, and where it is once
/// it has under [`FLING_STOP_DISTANCE`] left to travel (no jump to where the
/// decay would settle, so the very last frame moves least of all).
fn step_axis(axis: &mut Option<AxisThrow>, t: f64, max: f64) -> Option<f64> {
    let throw = (*axis)?;
    let (pos, left) = throw.at(t);
    let max = max.max(0.0);
    if pos <= 0.0 || pos >= max || left.abs() < FLING_STOP_DISTANCE {
        *axis = None;
        return Some(pos.clamp(0.0, max));
    }
    Some(pos)
}

impl ScrollView {
    /// Turn momentum scrolling of touch pans on or off for this view (on by
    /// default). Off, the content stops where the finger lifts.
    pub fn with_touch_fling(mut self, enabled: bool) -> Self {
        self.set_touch_fling(enabled);
        self
    }

    /// See [`Self::with_touch_fling`]. Turning it off stops a running fling.
    pub fn set_touch_fling(&mut self, enabled: bool) {
        self.fling_enabled = enabled;
        if !enabled {
            self.stop_fling();
        }
    }

    /// Whether the content is still moving from a flick.
    pub fn is_flinging(&self) -> bool {
        self.fling.is_some()
    }

    /// Stop a running fling where the content is now.
    pub fn stop_fling(&mut self) {
        if self.fling.take().is_some() && !self.middle_dragging {
            // The floating bar showed for the throw; let it fade now.
            self.v.touch_panning = false;
            self.h.touch_panning = false;
            crate::animation::request_draw();
        }
    }

    /// A touch pan's finger moved to `world` (logical, Y-up world space).
    pub(super) fn track_pan(&mut self, world: Point) {
        self.velocity.add(crate::clock::now(), world);
    }

    /// The touch pan's finger lifted while the view showed at `origin`
    /// (its local origin in world space): throw the content on at the
    /// finger's speed, if it was moving fast enough and flinging is on.
    pub(super) fn start_fling(&mut self, origin: Point) {
        self.fling = None;
        if !self.fling_enabled {
            return;
        }
        let now = crate::clock::now();
        let (fx, fy) = self.velocity.velocity_at(now);
        self.velocity.clear();
        let (vw, vh) = self.viewport();
        // A finger moving up raises the vertical offset (shows content
        // further down); one moving right lowers the horizontal one.
        let offsets = (
            self.v.can_scroll(vh).then_some(self.v.offset),
            self.h.can_scroll(vw).then_some(self.h.offset),
        );
        let screen_rect = Rect::new(origin.x, origin.y + self.bounds.height - vh, vw, vh);
        self.fling = Fling::new(now, offsets, (fy, -fx), screen_rect);
        if self.fling.is_some() {
            // The floating bar keeps showing while the content moves.
            self.v.touch_panning = true;
            self.h.touch_panning = true;
            crate::animation::request_layout();
        }
    }

    /// Advance a running fling to the UI clock's now (from `layout`, with
    /// the content measured and the viewport `(vw, vh)` known), and ask for
    /// the next layout while it still moves.
    pub(super) fn step_fling(&mut self, vw: f64, vh: f64) {
        let Some(fling) = self.fling.as_mut() else {
            return;
        };
        if fling.caught() {
            self.stop_fling();
            return;
        }
        let max = (self.v.max_scroll(vh), self.h.max_scroll(vw));
        let step = fling.step(crate::clock::now(), max);
        if let Some(v) = step.v {
            self.v.offset = v;
        }
        if let Some(h) = step.h {
            self.h.offset = h;
        }
        self.clamp_offsets();
        self.was_at_bottom = self.v.at_end(vh);
        self.publish_offsets();
        if step.done {
            self.stop_fling();
        } else {
            crate::animation::request_layout();
        }
    }
}

#[cfg(test)]
#[path = "fling_tests.rs"]
mod tests;
