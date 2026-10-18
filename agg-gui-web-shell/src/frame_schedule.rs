//! When the `requestAnimationFrame` loop runs again: the end-of-tick
//! decision ([`next_tick`]: another frame, a timer for the next deadline, or
//! sleep) and the wake bookkeeping ([`FrameScheduler`]) that keeps a sleeping
//! loop to exactly one requested frame per wake.
//!
//! An idle page must cost nothing: a loop that requests a frame on every
//! tick runs its per-tick work (backing-store sizing, sensor polling, the
//! idle queue, cursor and IME sync, `on_idle`) sixty times a second and
//! keeps a phone's CPU awake for a page that draws nothing. So the loop
//! requests another frame only while something needs one, arms one
//! `setTimeout` for the earliest future deadline, and otherwise stops. What
//! wakes it is listed on the crate's `web::frame` module; this module holds
//! the decisions, platform-neutral so `cargo test` reaches them.
//!
//! Erring toward waking is deliberate: a wake that finds nothing to do costs
//! one tick, a missed wake is a frozen screen.

use web_time::Instant;

/// Everything the end-of-tick decision reads, gathered by the frame loop.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LoopDemand {
    /// A frame can be painted now (the GPU is ready). Paint demand is moot
    /// without it; the GPU becoming ready wakes the loop itself.
    pub can_paint: bool,
    /// [`crate::RedrawPolicy::Continuous`].
    pub continuous: bool,
    /// No frame presented yet since boot or a GPU rebuild.
    pub first_paint_pending: bool,
    /// The shell's own repaint flag (input, `mark_dirty`, a resize).
    pub dirty: bool,
    /// `App::wants_draw()`: a draw or layout request, a running animation,
    /// queued work that ran, a deadline that came due.
    pub app_wants_draw: bool,
    /// `App::next_draw_deadline()`: the earliest scheduled draw (caret
    /// blink, tooltip delay, deferred idle work, an interval).
    pub next_draw_deadline: Option<Instant>,
    /// `agg_gui::ui_thread::next_due()`: when queued idle work (an action,
    /// a delayed action, an interval) needs a drain. Read from the queue, not
    /// from the draw flags a post raises, because a paint later in the same
    /// tick consumes those. Counts with or without a GPU: every tick drains.
    pub idle_work_at: Option<Instant>,
    /// A mouse button or touch contact is held.
    pub pointer_held: bool,
    /// A gamepad is connected; pads have no events, so they are polled.
    pub gamepad_connected: bool,
    /// Input arrived while the app was borrowed (`crate::held_input`) and
    /// waits for the next tick to deliver it; its own wake came while a tick
    /// was running, so only this decision can see it.
    pub input_held: bool,
    /// The app asked for tilt or a fullscreen toggle and the shell has not
    /// served it yet.
    pub platform_request_pending: bool,
    /// The host polls something every tick (`WebShellHost::wants_tick`) or
    /// asked for one more tick (`request_tick`).
    pub tick_requested: bool,
    /// A lost GPU device waits for a rebuild attempt at this instant.
    pub rebuild_at: Option<Instant>,
    pub now: Instant,
}

/// What the loop does after a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NextTick {
    /// Request the next animation frame.
    Frame,
    /// Sleep until this instant (one timer), then tick.
    Timer(Instant),
    /// Sleep until something wakes the loop.
    Sleep,
}

/// The end-of-tick decision.
pub(crate) fn next_tick(d: &LoopDemand) -> NextTick {
    let paint =
        d.can_paint && (d.continuous || d.first_paint_pending || d.dirty || d.app_wants_draw);
    let poll =
        d.pointer_held || d.gamepad_connected || d.input_held || d.platform_request_pending;
    if paint || poll || d.tick_requested {
        return NextTick::Frame;
    }
    let draw_at = d.next_draw_deadline.filter(|_| d.can_paint);
    let deadline = [draw_at, d.idle_work_at, d.rebuild_at]
        .into_iter()
        .flatten()
        .min();
    match deadline {
        Some(at) if at <= d.now => NextTick::Frame,
        Some(at) => NextTick::Timer(at),
        None => NextTick::Sleep,
    }
}

/// How the loop's one timer changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TimerChange {
    Keep,
    Cancel,
    /// Cancel any armed timer and arm one for this instant.
    Arm(Instant),
}

/// What the frame loop must do with the browser after a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Schedule {
    pub request_frame: bool,
    pub timer: TimerChange,
}

/// The loop's state between ticks: whether a frame is already requested, a
/// tick is running, and which deadline the armed timer serves.
#[derive(Debug, Default)]
pub(crate) struct FrameScheduler {
    frame_requested: bool,
    in_tick: bool,
    timer: Option<Instant>,
}

impl FrameScheduler {
    pub(crate) const fn new() -> Self {
        Self {
            frame_requested: false,
            in_tick: false,
            timer: None,
        }
    }

    /// Something may want a frame. Returns whether to request one now: only
    /// when the loop is asleep. A wake during a tick is left to that tick's
    /// own decision, which reads the same state the wake announced, and a
    /// wake while a frame is already requested is that frame's.
    pub(crate) fn wake(&mut self) -> bool {
        if self.in_tick || self.frame_requested {
            return false;
        }
        self.frame_requested = true;
        true
    }

    /// The requested frame arrived and its tick starts.
    pub(crate) fn begin_tick(&mut self) {
        self.frame_requested = false;
        self.in_tick = true;
    }

    /// The tick ended with decision `next`.
    pub(crate) fn end_tick(&mut self, next: NextTick) -> Schedule {
        self.in_tick = false;
        match next {
            NextTick::Frame => {
                self.frame_requested = true;
                Schedule {
                    request_frame: true,
                    timer: self.cancel_timer(),
                }
            }
            NextTick::Timer(at) if self.timer == Some(at) => Schedule {
                request_frame: false,
                timer: TimerChange::Keep,
            },
            NextTick::Timer(at) => {
                self.timer = Some(at);
                Schedule {
                    request_frame: false,
                    timer: TimerChange::Arm(at),
                }
            }
            NextTick::Sleep => Schedule {
                request_frame: false,
                timer: self.cancel_timer(),
            },
        }
    }

    /// The armed timer fired. Returns whether to request a frame now.
    pub(crate) fn timer_fired(&mut self) -> bool {
        self.timer = None;
        self.wake()
    }

    /// Whether no frame is requested and no tick is running.
    #[cfg(test)]
    pub(crate) fn is_asleep(&self) -> bool {
        !self.frame_requested && !self.in_tick
    }

    fn cancel_timer(&mut self) -> TimerChange {
        match self.timer.take() {
            Some(_) => TimerChange::Cancel,
            None => TimerChange::Keep,
        }
    }
}

/// Whole milliseconds to wait for `at`, rounded up and one past it, so the
/// timer never fires before the deadline is due on the frame clock (browsers
/// coarsen timers and `performance.now()`). Saturates at `i32::MAX` ms, the
/// longest delay `setTimeout` takes.
pub(crate) fn timer_delay_ms(at: Instant, now: Instant) -> i32 {
    let wait = at.saturating_duration_since(now);
    let ms = wait.as_micros().div_ceil(1000) + 1;
    i32::try_from(ms).unwrap_or(i32::MAX)
}

#[cfg(test)]
#[path = "frame_schedule_tests.rs"]
mod tests;
