//! Thread-local signal: "please paint another frame soon".
//!
//! Widgets that are mid-animation (e.g. a scroll bar expanding on hover) call
//! [`request_tick`] during their `paint`.  After the widget tree has painted,
//! the host main loop reads [`wants_tick`] to decide whether to request
//! continuous redraws (Poll) or revert to event-driven waiting.  The flag is
//! cleared once per frame at the start of [`crate::widget::App::paint`] so
//! each paint starts from a clean slate and the flag accurately reflects
//! "some widget still wants more frames after this paint finished".

use std::cell::Cell;
use std::time::Instant;

std::thread_local! {
    static NEEDS_TICK: Cell<bool> = Cell::new(false);
}

/// Request that the host schedule another paint as soon as possible.  Safe to
/// call any number of times in a frame.  Typically called from `Widget::paint`
/// while a time-based animation is in progress.
pub fn request_tick() {
    NEEDS_TICK.with(|c| c.set(true));
}

/// Non-destructive read.  Hosts call this after painting to decide control-flow
/// for the next loop iteration.
pub fn wants_tick() -> bool {
    NEEDS_TICK.with(|c| c.get())
}

/// Reset the flag.  The `App::paint` entry point calls this before delegating
/// to the root widget so each frame starts fresh.
pub fn clear_tick() {
    NEEDS_TICK.with(|c| c.set(false));
}

// ── Tween ────────────────────────────────────────────────────────────────────
//
// Small reusable time-based interpolator for widgets that want a smooth
// transition between two scalar states (hover ↔ dormant, off ↔ on, etc.).
// Ease-out cubic; reversal preserves the current value so rapid toggles
// don't snap.  Requests an animation tick automatically while in flight.

/// Smooth scalar tween between `0.0` and `1.0` (or any pair of values the
/// caller interprets).  Drives animations such as the scroll-bar hover
/// expansion and toggle-switch on/off slide.
#[derive(Clone, Copy)]
pub struct Tween {
    current:     f64,
    start_value: f64,
    target:      f64,
    start_time:  Option<Instant>,
    duration:    f64,
}

impl Tween {
    /// New tween that starts at `initial` with the same value as its target
    /// (no animation in flight).
    pub const fn new(initial: f64, duration_secs: f64) -> Self {
        Self {
            current:     initial,
            start_value: initial,
            target:      initial,
            start_time:  None,
            duration:    duration_secs,
        }
    }

    /// Update the target.  If it differs from the current target, re-anchors
    /// the animation at the current interpolated value so reversals are smooth.
    pub fn set_target(&mut self, new_target: f64) {
        if (self.target - new_target).abs() > 1e-9 {
            self.start_value = self.current;
            self.target      = new_target;
            self.start_time  = Some(Instant::now());
        }
    }

    /// Advance the animation based on elapsed wall time and return the new
    /// interpolated value.  Ease-out cubic.  While in flight this also calls
    /// [`request_tick`] so the host keeps painting frames until completion.
    pub fn tick(&mut self) -> f64 {
        if let Some(start) = self.start_time {
            let elapsed = start.elapsed().as_secs_f64();
            let p = (elapsed / self.duration).min(1.0);
            let eased = 1.0 - (1.0 - p).powi(3);
            self.current = self.start_value + (self.target - self.start_value) * eased;
            if p >= 1.0 {
                self.current    = self.target;
                self.start_time = None;
            } else {
                request_tick();
            }
        }
        self.current
    }

    /// Current interpolated value without advancing.
    pub fn value(&self) -> f64 { self.current }
}

impl Default for Tween {
    fn default() -> Self { Self::new(0.0, 0.12) }
}
