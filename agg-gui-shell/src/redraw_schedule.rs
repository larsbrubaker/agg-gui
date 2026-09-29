//! Pure scheduling decisions for the shell's event loop: whether a paint may
//! run at all, and which `ControlFlow` to park in afterwards.
//!
//! Split out of `shell_loop.rs` so the decisions are unit-testable without a
//! window or a GPU. `ShellLoop::paint` consults [`paint_blocked`];
//! `ShellLoop::about_to_wait` builds a [`LoopState`] and applies
//! [`next_control_flow`]. The surface-retry deadline it reads comes from
//! `paint::PaintOutcome::retry_at` (a `RetryWake::After` from agg-gui-wgpu).

use std::time::Instant;

use winit::event_loop::ControlFlow;

/// Should the paint be skipped outright, without touching the surface?
///
/// Only while BOTH hold: the window is minimized, and its surface is
/// unconfigured (a configure-failure run is in progress —
/// `Gpu::surface_configured` is false). Then every retry against the
/// minimized window (a minimized HWND on Windows) would only fail and spend
/// the surface retry budget, with nothing to present anyway. A minimized
/// window whose surface is configured paints exactly as before — a
/// continuous-mode host keeps ticking `on_frame`.
///
/// Restoring the window delivers `WindowEvent::Resized` with a nonzero size
/// (on Windows, `WM_SIZE`; minimize itself delivers `Resized(0, 0)`), and the
/// shell's `Resized` arm requests a redraw — that is the paint that resumes
/// the retries. `minimized == None` (the platform cannot tell) paints as
/// usual.
pub(crate) fn paint_blocked(minimized: Option<bool>, surface_configured: bool) -> bool {
    minimized == Some(true) && !surface_configured
}

/// Everything [`next_control_flow`] decides from, sampled in `about_to_wait`
/// after this iteration's paint.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LoopState {
    /// A deterministic capture is still waiting for its frame.
    pub(crate) capture_pending: bool,
    /// `RedrawPolicy::Continuous`.
    pub(crate) continuous: bool,
    /// `App::wants_draw()` (immediate request or a due deadline).
    pub(crate) app_wants_draw: bool,
    /// [`paint_blocked`]: minimized with an unconfigured surface, so the
    /// paint is skipped.
    pub(crate) paint_blocked: bool,
    /// When a backed-off surface configure retry is due, from the last
    /// paint's `RetryWake::After`.
    pub(crate) surface_retry_at: Option<Instant>,
    /// `App::next_draw_deadline()`.
    pub(crate) app_deadline: Option<Instant>,
    pub(crate) now: Instant,
}

/// The `ControlFlow` to park the loop in.
///
/// - While a failed surface configure is backing off, nothing can be
///   presented before the retry is due, so continuous mode, a pending
///   capture and app draw requests are all served by waiting for that
///   deadline (or an earlier app deadline) instead of polling — polling
///   would spin a core re-asking a surface that has said "not yet".
/// - While the paint is blocked (minimized with an unconfigured surface, see
///   [`paint_blocked`]), continuous mode and app draw requests do not poll
///   either; restore wakes the loop. A minimized window with a configured
///   surface is not blocked and polls exactly as before. A pending capture
///   still polls, so its wall-clock give-up
///   (`screenshot::capture_exhausted`) keeps being checked.
/// - Otherwise: `Poll` while anything wants frames, `WaitUntil` for an app
///   deadline, `Wait` when idle.
pub(crate) fn next_control_flow(s: &LoopState) -> ControlFlow {
    if let Some(retry) = s.surface_retry_at.filter(|&at| at > s.now) {
        let wake = s.app_deadline.map_or(retry, |app| app.min(retry));
        return ControlFlow::WaitUntil(wake);
    }
    let poll = s.capture_pending || (!s.paint_blocked && (s.continuous || s.app_wants_draw));
    if poll {
        ControlFlow::Poll
    } else if let Some(t) = s.app_deadline {
        ControlFlow::WaitUntil(t)
    } else {
        ControlFlow::Wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn idle(now: Instant) -> LoopState {
        LoopState {
            capture_pending: false,
            continuous: false,
            app_wants_draw: false,
            paint_blocked: false,
            surface_retry_at: None,
            app_deadline: None,
            now,
        }
    }

    #[test]
    fn only_a_minimized_window_with_an_unconfigured_surface_blocks_the_paint() {
        assert!(paint_blocked(Some(true), false));
        assert!(!paint_blocked(Some(false), false));
        assert!(!paint_blocked(None, false), "unknown state paints as usual");
        assert!(!paint_blocked(Some(false), true));
        assert!(!paint_blocked(None, true));
    }

    #[test]
    fn minimized_window_with_a_configured_surface_paints_and_polls_as_before() {
        // Continuous-mode hosts do work in `on_frame`; minimizing a window
        // whose surface is fine must not stop them ticking.
        assert!(!paint_blocked(Some(true), true));
        let s = LoopState {
            continuous: true,
            paint_blocked: paint_blocked(Some(true), true),
            ..idle(Instant::now())
        };
        assert_eq!(next_control_flow(&s), ControlFlow::Poll);
    }

    #[test]
    fn surface_backoff_waits_instead_of_polling() {
        // Continuous mode, a pending capture and an app draw request all
        // want frames, but the surface cannot give one before the retry.
        let now = Instant::now();
        let retry = now + Duration::from_millis(400);
        let s = LoopState {
            capture_pending: true,
            continuous: true,
            app_wants_draw: true,
            surface_retry_at: Some(retry),
            ..idle(now)
        };
        assert_eq!(next_control_flow(&s), ControlFlow::WaitUntil(retry));
    }

    #[test]
    fn earlier_app_deadline_still_wakes_during_backoff() {
        let now = Instant::now();
        let app = now + Duration::from_millis(100);
        let s = LoopState {
            continuous: true,
            surface_retry_at: Some(now + Duration::from_millis(800)),
            app_deadline: Some(app),
            ..idle(now)
        };
        assert_eq!(next_control_flow(&s), ControlFlow::WaitUntil(app));
    }

    #[test]
    fn a_due_retry_no_longer_holds_the_loop() {
        let now = Instant::now();
        let s = LoopState {
            continuous: true,
            surface_retry_at: Some(now),
            ..idle(now)
        };
        assert_eq!(next_control_flow(&s), ControlFlow::Poll);
    }

    #[test]
    fn blocked_paint_does_not_poll_for_frames_it_cannot_paint() {
        let now = Instant::now();
        let continuous = LoopState {
            continuous: true,
            app_wants_draw: true,
            paint_blocked: paint_blocked(Some(true), false),
            ..idle(now)
        };
        assert_eq!(next_control_flow(&continuous), ControlFlow::Wait);
        // A pending capture keeps polling so its give-up timer still runs.
        let capture = LoopState {
            capture_pending: true,
            paint_blocked: paint_blocked(Some(true), false),
            ..idle(now)
        };
        assert_eq!(next_control_flow(&capture), ControlFlow::Poll);
    }

    #[test]
    fn normal_ladder_is_unchanged() {
        let now = Instant::now();
        assert_eq!(next_control_flow(&idle(now)), ControlFlow::Wait);
        let wants = LoopState {
            app_wants_draw: true,
            ..idle(now)
        };
        assert_eq!(next_control_flow(&wants), ControlFlow::Poll);
        let continuous = LoopState {
            continuous: true,
            ..idle(now)
        };
        assert_eq!(next_control_flow(&continuous), ControlFlow::Poll);
        let capture = LoopState {
            capture_pending: true,
            ..idle(now)
        };
        assert_eq!(next_control_flow(&capture), ControlFlow::Poll);
        let t = now + Duration::from_secs(1);
        let deadline = LoopState {
            app_deadline: Some(t),
            ..idle(now)
        };
        assert_eq!(next_control_flow(&deadline), ControlFlow::WaitUntil(t));
    }
}
