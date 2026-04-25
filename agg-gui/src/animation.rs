//! Thread-local repaint-request signals.
//!
//! Two independent channels feed the host's event loop:
//!
//! 1. **Immediate** — [`request_tick`] / [`wants_tick`].  Any widget whose
//!    state just changed calls `request_tick()`; the next iteration of the
//!    host loop paints a frame and clears the flag.  This is the "mark
//!    dirty" path: input handlers, hover transitions, tweens mid-animation,
//!    drag movement, continuous capture widgets.
//!
//! 2. **Scheduled** — [`request_repaint_after`] / [`next_repaint_at`].  A
//!    widget that needs a redraw *at a future time* (text-cursor blink,
//!    tooltip delay) calls `request_repaint_after(Duration)`; the host's
//!    loop goes to sleep with `ControlFlow::WaitUntil(that_instant)` and
//!    paints when the deadline fires.  Successive calls keep the EARLIEST
//!    deadline.
//!
//! The host loop paints iff `wants_tick() || now >= next_repaint_at()`.
//! Between paints it idles; no frames are drawn while nothing has changed.

use std::cell::Cell;
use std::time::Duration;
use web_time::Instant;

std::thread_local! {
    static NEEDS_TICK:      Cell<bool>            = Cell::new(false);
    static NEXT_REPAINT_AT: Cell<Option<Instant>> = Cell::new(None);
}

/// Request that the host schedule another paint as soon as possible.  Safe to
/// call any number of times in a frame.  Typically called from `Widget::paint`
/// while a time-based animation is in progress, from input handlers whose
/// widget state changed, or from anywhere that mutates visual state.
pub fn request_tick() {
    NEEDS_TICK.with(|c| c.set(true));
}

/// Non-destructive read.  Hosts call this after painting to decide control-flow
/// for the next loop iteration.
pub fn wants_tick() -> bool {
    NEEDS_TICK.with(|c| c.get())
}

/// Reset the per-frame repaint flags.  The `App::paint` entry point calls
/// this before delegating to the root widget so each frame starts fresh —
/// widgets that still need a redraw (animation in flight, focus blink, etc.)
/// must re-arm during their paint, otherwise the loop goes idle.
pub fn clear_tick() {
    NEEDS_TICK.with(|c| c.set(false));
    NEXT_REPAINT_AT.with(|c| c.set(None));
}

/// Schedule a future paint.  Keeps the EARLIEST pending deadline, so multiple
/// widgets asking for different delays will all be served by the soonest one
/// (each widget re-arms its own deadline on the next paint anyway).
pub fn request_repaint_after(delay: Duration) {
    let when = Instant::now() + delay;
    NEXT_REPAINT_AT.with(|c| match c.get() {
        Some(existing) if existing <= when => {}
        _ => c.set(Some(when)),
    });
}

/// Read-and-clear the scheduled repaint deadline.  The host reads this after
/// painting so the next frame's scheduled wake is determined entirely by what
/// the fresh paint registered (e.g. a text field re-arms the 500 ms blink
/// each frame while it remains focused; losing focus means no re-arm and the
/// loop goes idle).
pub fn take_next_repaint() -> Option<Instant> {
    NEXT_REPAINT_AT.with(|c| c.replace(None))
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
    current: f64,
    start_value: f64,
    target: f64,
    start_time: Option<Instant>,
    duration: f64,
}

impl Tween {
    /// New tween that starts at `initial` with the same value as its target
    /// (no animation in flight).
    pub const fn new(initial: f64, duration_secs: f64) -> Self {
        Self {
            current: initial,
            start_value: initial,
            target: initial,
            start_time: None,
            duration: duration_secs,
        }
    }

    /// Update the target.  If it differs from the current target, re-anchors
    /// the animation at the current interpolated value so reversals are smooth.
    pub fn set_target(&mut self, new_target: f64) {
        if (self.target - new_target).abs() > 1e-9 {
            self.start_value = self.current;
            self.target = new_target;
            self.start_time = Some(Instant::now());
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
                self.current = self.target;
                self.start_time = None;
            } else {
                request_tick();
            }
        }
        self.current
    }

    /// Current interpolated value without advancing.
    pub fn value(&self) -> f64 {
        self.current
    }
}

impl Default for Tween {
    fn default() -> Self {
        Self::new(0.0, 0.12)
    }
}
