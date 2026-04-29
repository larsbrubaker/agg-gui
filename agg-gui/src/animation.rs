//! Thread-local draw-request and invalidation signals.
//!
//! Two independent channels feed the host's event loop:
//!
//! 1. **Immediate draw request** — [`request_draw`] / [`wants_draw`].  Any
//!    widget whose visual output just changed calls `request_draw()`; the next
//!    iteration of the host loop draws a frame and clears the flag.  The same
//!    call advances [`invalidation_epoch`], letting event dispatch dirty the
//!    affected retained ancestor path even when the event bubbles as ignored.
//!
//! 2. **Scheduled draw** — [`request_draw_after`] /
//!    [`take_next_draw_deadline`].  A
//!    widget that needs a draw *at a future time* (text-cursor blink,
//!    tooltip delay) calls `request_draw_after(Duration)`; the host's
//!    loop goes to sleep with `ControlFlow::WaitUntil(that_instant)` and
//!    draws when the deadline fires.  Successive calls keep the EARLIEST
//!    deadline.
//!
//! The host loop draws iff `wants_draw() || now >= take_next_draw_deadline()`.
//! Between draws it idles; no frames are drawn while nothing has changed.

use std::cell::Cell;
use std::time::Duration;
use web_time::Instant;

std::thread_local! {
    static NEEDS_DRAW:        Cell<bool>            = Cell::new(false);
    static NEXT_DRAW_AT:      Cell<Option<Instant>> = Cell::new(None);
    static INVALIDATION_EPOCH: Cell<u64>             = Cell::new(0);
}

/// Request that the host schedule another draw as soon as possible.
///
/// This is also the canonical visual invalidation hook: event dispatch compares
/// [`invalidation_epoch`] before/after delivery and dirties the affected
/// retained ancestor path when a widget requested a draw.
pub fn request_draw() {
    NEEDS_DRAW.with(|c| c.set(true));
    INVALIDATION_EPOCH.with(|c| c.set(c.get().wrapping_add(1)));
}

/// Request a frame without dirtying retained widget backbuffers.
///
/// Use this for app-level overlays whose source state changed outside a
/// retained subtree. The inspector hover rectangle is the canonical case:
/// it must redraw, but the inspected/inspector windows do not need their FBOs
/// rebuilt just because the overlay target moved.
pub fn request_draw_without_invalidation() {
    NEEDS_DRAW.with(|c| c.set(true));
}

/// Non-destructive read.  Hosts call this after drawing to decide control-flow
/// for the next loop iteration.
pub fn wants_draw() -> bool {
    NEEDS_DRAW.with(|c| c.get())
}

/// Monotonic draw-request epoch used to detect visual changes during dispatch.
pub fn invalidation_epoch() -> u64 {
    INVALIDATION_EPOCH.with(|c| c.get())
}

/// Reset the per-frame draw flags.  The `App::paint` entry point calls
/// this before delegating to the root widget so each frame starts fresh —
/// widgets that still need a draw (animation in flight, focus blink, etc.)
/// must re-arm during their draw, otherwise the loop goes idle.
pub fn clear_draw_request() {
    NEEDS_DRAW.with(|c| c.set(false));
    NEXT_DRAW_AT.with(|c| c.set(None));
}

/// Schedule a future draw.  Keeps the EARLIEST pending deadline, so multiple
/// widgets asking for different delays will all be served by the soonest one
/// (each widget re-arms its own deadline on the next draw anyway).
pub fn request_draw_after(delay: Duration) {
    let when = Instant::now() + delay;
    NEXT_DRAW_AT.with(|c| match c.get() {
        Some(existing) if existing <= when => {}
        _ => c.set(Some(when)),
    });
}

/// Read-and-clear the scheduled draw deadline.  The host reads this after
/// drawing so the next frame's scheduled wake is determined entirely by what
/// the fresh draw registered (e.g. a text field re-arms the 500 ms blink
/// each frame while it remains focused; losing focus means no re-arm and the
/// loop goes idle).
pub fn take_next_draw_deadline() -> Option<Instant> {
    NEXT_DRAW_AT.with(|c| c.replace(None))
}

// ── Tween ────────────────────────────────────────────────────────────────────
//
// Small reusable time-based interpolator for widgets that want a smooth
// transition between two scalar states (hover ↔ dormant, off ↔ on, etc.).
// Ease-out cubic; reversal preserves the current value so rapid toggles
// don't snap.  Requests a draw automatically while in flight.

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
    /// the animation at the current interpolated value so reversals are smooth
    /// and requests the first frame of the transition.
    ///
    /// Widgets that own a `Tween` must also report `tween.is_animating()` from
    /// `Widget::needs_draw()` so retained parents repaint every frame until
    /// the tween settles.
    pub fn set_target(&mut self, new_target: f64) {
        if (self.target - new_target).abs() > 1e-9 {
            self.start_value = self.current;
            self.target = new_target;
            self.start_time = Some(Instant::now());
            request_draw();
        }
    }

    /// Advance the animation based on elapsed wall time and return the new
    /// interpolated value.  Ease-out cubic.  While in flight this also calls
    /// [`request_draw`] so the host keeps drawing frames until completion.
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
                request_draw();
            }
        }
        self.current
    }

    /// Current interpolated value without advancing.
    pub fn value(&self) -> f64 {
        self.current
    }

    /// Whether the tween still needs frames to reach its target.
    pub fn is_animating(&self) -> bool {
        self.start_time.is_some()
    }
}

impl Default for Tween {
    fn default() -> Self {
        Self::new(0.0, 0.12)
    }
}
