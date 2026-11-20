//! The global keyboard lift: how far the whole tree is shown raised to keep
//! the focused field above the on-screen keyboard, as a [`Tween`] that
//! slides with the keyboard, and the lift the current paint is under.
//!
//! `keyboard_scroll.rs` decides the lift (what the scroll chain cannot
//! absorb, or the keyboard's room when `keyboard_resize.rs` lays the tree out
//! above it), says what root logical space means while lifted, paints the
//! tree under it (`paint_lifted_tree`) and re-exports this module's API, so
//! callers keep naming it `keyboard_scroll::request_lift` and so on. This
//! file holds the lift's state alone. Setting the lift asks for a frame only
//! when its target changes: the resized layout sets it again at the end of
//! every layout, and a draw request there would make every frame lay the
//! whole tree out again.

use std::cell::{Cell, RefCell};

use crate::animation::Tween;

/// Slide duration for the global keyboard-driven lift, in seconds.
/// Matches the keyboard's own slide animation so the lift and the
/// keyboard panel travel together — no visible "the field jumps,
/// THEN the keyboard slides" stagger.
const LIFT_DURATION_SECS: f64 = 0.22;

thread_local! {
    /// Active "lift" applied to the entire widget tree (Y-up pixels)
    /// to keep the focused field above the on-screen keyboard panel.
    /// Animated via [`Tween`] so the raise / lower reads as a smooth
    /// slide rather than a snap.  Updated by [`request_lift`] when
    /// focus changes and ticked by `App::paint`.
    static LIFT: RefCell<Tween> = const { RefCell::new(Tween::new(0.0, LIFT_DURATION_SECS)) };

    /// The lift translate `keyboard_scroll::paint_lifted_tree` has applied to the paint
    /// ctx right now — 0 outside it. Lets
    /// [`logical_root_transform`](crate::widget::logical_root_transform)
    /// take it back out, so paint-time root coordinates are unlifted.
    static PAINT_LIFT: Cell<f64> = const { Cell::new(0.0) };
}

/// The keyboard lift translate currently applied to the paint ctx by
/// `keyboard_scroll::paint_lifted_tree`; 0 outside it (event time, direct
/// test paints).
pub(crate) fn paint_lift() -> f64 {
    PAINT_LIFT.with(|c| c.get())
}

/// Sets [`PAINT_LIFT`] for the duration of a lifted paint and restores the
/// previous value on drop, so a panicking paint can't leak it.
pub(crate) struct PaintLiftGuard {
    prev: f64,
}

impl PaintLiftGuard {
    pub(crate) fn set(lift: f64) -> Self {
        PaintLiftGuard {
            prev: PAINT_LIFT.with(|c| c.replace(lift)),
        }
    }
}

impl Drop for PaintLiftGuard {
    fn drop(&mut self) {
        PAINT_LIFT.with(|c| c.set(self.prev));
    }
}

/// Run `f` — a raster into a FRESH offscreen ctx (CPU backbuffer
/// `GfxCtx` / `LcdGfxCtx`) — with [`paint_lift`] reading 0, restoring it
/// afterwards (even on panic). The paint lift describes the translate on
/// App's own ctx; a fresh sub ctx's `root_transform` is bitmap-relative and
/// never carried it. Compositing layers pushed onto App's ctx must NOT use
/// this: their `root_transform` includes the lift through the layer origins.
pub(crate) fn with_paint_lift_cleared<R>(f: impl FnOnce() -> R) -> R {
    let _cleared = PaintLiftGuard::set(0.0);
    f()
}

/// Set the lift target.  No-op when the new target matches the
/// current target — no draw is requested either, since nothing moves: the
/// keyboard-resized layout sets the target again at the end of every layout
/// (`keyboard_resize::after_layout`), and a draw request there would move the
/// invalidation epoch and make every frame lay the whole tree out again.
/// When it differs the animation re-anchors at the current interpolated
/// value, so a focus change that walks the target up then back down
/// reverses smoothly, and a draw is requested to start the slide.
pub fn request_lift(target: f64) {
    let changed = LIFT.with(|c| {
        let mut lift = c.borrow_mut();
        let before = lift.target();
        lift.set_target(target.max(0.0));
        lift.target() != before
    });
    if changed {
        crate::animation::request_draw();
    }
}

/// Current interpolated lift in Y-up pixels, without advancing the
/// tween.  Mouse handlers call this to translate on-screen positions into
/// root logical (unlifted layout) coordinates; the keyboard panel itself
/// sits in screen space so its hit-tests use the on-screen position.
/// During `App::paint` it equals the lift the tree is painted under (the
/// paint ticks the tween first), so it is valid at event and paint time.
pub fn current_lift() -> f64 {
    LIFT.with(|c| c.borrow().value())
}

/// Advance the animation by one frame and return the new lift.  Call
/// exactly once per paint — Tween auto-requests another draw while
/// the animation is in flight.
pub fn tick_lift() -> f64 {
    LIFT.with(|c| c.borrow_mut().tick())
}

/// `true` while the lift tween is still travelling toward its target.
/// `App::wants_draw` honours this so the host event loop keeps
/// pumping frames until the slide completes.
pub fn is_lift_animating() -> bool {
    LIFT.with(|c| c.borrow().is_animating())
}

/// Put the lift back at rest at 0 — `keyboard_scroll::reset_lift_for_test`.
#[cfg(test)]
pub(crate) fn reset_for_test() {
    LIFT.with(|c| *c.borrow_mut() = Tween::new(0.0, LIFT_DURATION_SECS));
}

/// Pin the lift at a settled `v` (no animation in flight), so
/// [`current_lift`] and [`tick_lift`] both read exactly `v` — for tests
/// that check where things land while the tree is lifted. Pair with
/// `keyboard_scroll::reset_lift_for_test` (the thread-local outlives the
/// test).
#[cfg(test)]
pub fn set_lift_for_test(v: f64) {
    LIFT.with(|c| *c.borrow_mut() = Tween::new(v, LIFT_DURATION_SECS));
}

/// Where the lift tween is travelling toward (last value passed to
/// `request_lift`).  Useful for asserting that focus loss did in fact
/// retarget the lift back to 0 without waiting on the animation.
#[cfg(test)]
pub fn lift_target_for_test() -> f64 {
    LIFT.with(|c| c.borrow().target())
}
