//! Per-tick paint decisions: the first-paint guarantee, the reactive /
//! continuous predicate, and the layout-skip key.
//!
//! Platform-neutral on purpose — the rAF loop in [`crate::web`] is wasm-only
//! and cannot be unit tested natively, so every *decision* it makes lives here
//! where `cargo test` reaches the production code.
//!
//! # The first-paint bug this guards against
//!
//! Ported from AtomArtist (`atomartist-ui/src/first_paint.rs`, regression
//! tests in `atomartist-ui-test/tests/first_paint.rs`). A reactive web host
//! paints only when `wants_draw()` says so. Native winit hands the shell an
//! initial `RedrawRequested`; the browser has no equivalent, and nothing
//! guarantees a draw request is still pending when async GPU init resolves.
//! The page could stay blank until the first resize. [`FirstPaintGate`] forces
//! painting until one frame has actually been *presented*, so the first frame
//! is unconditional and self-healing (a tick that bails before presenting —
//! no surface texture yet — leaves the gate open).

use std::cell::Cell;

use crate::config::RedrawPolicy;

/// One-shot latch: forces painting until the first frame is presented.
#[derive(Debug, Default)]
pub struct FirstPaintGate {
    painted: Cell<bool>,
}

impl FirstPaintGate {
    /// A gate that has not yet seen a presented frame.
    pub const fn new() -> Self {
        Self {
            painted: Cell::new(false),
        }
    }

    /// True once [`Self::mark_painted`] has run.
    pub fn has_painted(&self) -> bool {
        self.painted.get()
    }

    /// Record that a frame was acquired, painted and presented. Call only
    /// after `present` — never on a tick that bailed early.
    pub fn mark_painted(&self) {
        self.painted.set(true);
    }

    /// Reopen the gate — after a GPU rebuild, the next frame must paint
    /// whatever the reactive predicate says.
    pub fn reset(&self) {
        self.painted.set(false);
    }

    /// Whether this tick should paint. `host_wants_draw` is evaluated lazily
    /// and skipped entirely while the first paint is being forced, so a
    /// forced tick never promotes a due `request_draw_after` deadline early.
    pub fn should_paint_tick(&self, resized: bool, host_wants_draw: impl FnOnce() -> bool) -> bool {
        !self.painted.get() || resized || host_wants_draw()
    }
}

/// The reactive/continuous predicate — the web port of the native shell's
/// `AboutToWait` redraw decision.
///
/// `dirty` is the shell's own flag (input arrived, [`crate::mark_dirty`]),
/// `app_wants_draw` is `App::wants_draw()` (which includes
/// `animation::wants_draw()`), and `next_deadline` is
/// `App::next_draw_deadline()` — a cursor blink or delayed tooltip paints once
/// its instant has passed.
pub fn wants_paint(
    policy: RedrawPolicy,
    dirty: bool,
    app_wants_draw: bool,
    next_deadline: Option<web_time::Instant>,
    now: web_time::Instant,
) -> bool {
    policy == RedrawPolicy::Continuous
        || dirty
        || app_wants_draw
        || next_deadline.is_some_and(|d| now >= d)
}

/// Everything that feeds layout: surface size, device scale bits, and the
/// agg-gui invalidation epoch. Layout is skipped while it is unchanged.
pub type LayoutKey = (u32, u32, u64, u64);

/// The layout key for a `width × height` frame at the current device scale
/// and invalidation epoch.
pub fn layout_key(width: u32, height: u32) -> LayoutKey {
    (
        width,
        height,
        agg_gui::device_scale().to_bits(),
        agg_gui::animation::invalidation_epoch(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn gate_forces_paint_until_a_frame_is_presented() {
        let gate = FirstPaintGate::new();
        assert!(gate.should_paint_tick(false, || false));
        // Not latched by asking — the tick may have bailed before present.
        assert!(gate.should_paint_tick(false, || false));
        gate.mark_painted();
        assert!(!gate.should_paint_tick(false, || false));
        assert!(gate.should_paint_tick(true, || false), "resize paints");
        assert!(gate.should_paint_tick(false, || true), "request paints");
    }

    #[test]
    fn forced_first_paint_does_not_evaluate_the_predicate() {
        let gate = FirstPaintGate::new();
        let mut evaluated = false;
        assert!(gate.should_paint_tick(false, || {
            evaluated = true;
            false
        }));
        assert!(!evaluated);
    }

    #[test]
    fn reset_reopens_the_gate() {
        let gate = FirstPaintGate::new();
        gate.mark_painted();
        gate.reset();
        assert!(!gate.has_painted());
        assert!(gate.should_paint_tick(false, || false));
    }

    #[test]
    fn reactive_idle_does_not_paint() {
        let now = web_time::Instant::now();
        assert!(!wants_paint(
            RedrawPolicy::Reactive,
            false,
            false,
            None,
            now
        ));
    }

    #[test]
    fn continuous_always_paints() {
        let now = web_time::Instant::now();
        assert!(wants_paint(
            RedrawPolicy::Continuous,
            false,
            false,
            None,
            now
        ));
    }

    #[test]
    fn dirty_or_app_request_paints() {
        let now = web_time::Instant::now();
        assert!(wants_paint(RedrawPolicy::Reactive, true, false, None, now));
        assert!(wants_paint(RedrawPolicy::Reactive, false, true, None, now));
    }

    #[test]
    fn deadline_paints_only_once_due() {
        let now = web_time::Instant::now();
        let later = now + Duration::from_millis(500);
        assert!(!wants_paint(
            RedrawPolicy::Reactive,
            false,
            false,
            Some(later),
            now
        ));
        assert!(wants_paint(
            RedrawPolicy::Reactive,
            false,
            false,
            Some(now),
            now
        ));
        assert!(wants_paint(
            RedrawPolicy::Reactive,
            false,
            false,
            Some(now),
            later
        ));
    }

    #[test]
    fn layout_key_tracks_size() {
        assert_ne!(layout_key(100, 100), layout_key(101, 100));
        assert_eq!(layout_key(100, 100), layout_key(100, 100));
    }
}
