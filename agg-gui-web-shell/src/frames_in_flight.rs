//! The frames-in-flight gate: how many presented frames may still be running
//! on the GPU before the frame loop stops submitting new ones.
//!
//! `requestAnimationFrame` does not wait for the GPU to finish a frame (a
//! software GPU such as SwiftShader takes hundreds of milliseconds for a full
//! 3D frame). A loop that submits on every tick with a redraw pending then
//! queues frames ahead of the GPU: what is on screen falls further behind the
//! input that caused it, the browser slows its animation frames (and with
//! them input handling) to the GPU's pace, and its own work that waits on the
//! GPU (a page's smooth scroll) stalls behind the backlog. So the frame loop in the crate's
//! `web::frame` module counts each presented frame until its
//! `queue.onSubmittedWorkDone()` resolves, and on a tick where the limit is
//! reached it skips painting and keeps the redraw pending. The tick itself
//! still runs (`on_tick`, input, the idle queue), and the first tick after
//! the GPU catches up paints the latest state: a frame is delayed, never lost.
//!
//! A device loss can leave a frame's promise unresolved for good, and a
//! promise from the old device can resolve after the new one is installed.
//! [`FramesInFlight::reset`] starts a new generation when a device is
//! (re)installed: the count starts from zero and completions carrying an
//! older generation are ignored.
//!
//! A completion that never arrives is not waited for forever: a frame left
//! unfinished well past this device's own frame times stops counting
//! ([`FramesInFlight::expire`]).
//!
//! Platform-neutral so `cargo test` reaches the production state machine.

use std::time::Duration;

use web_time::Instant;

/// Presented frames that may be on the GPU at once while it keeps up: the
/// standard pipelining, so a fast GPU whose completion reaches the page a
/// little after the next refresh (a 90 or 120 Hz phone) still paints every
/// refresh, with at most one frame waiting.
///
/// A slow GPU gets one ([`SLOW_FRAME`]): measured on SwiftShader at device
/// scale 3, two in flight let Chrome slow its animation frames to under 5 a
/// second while the camera was dragged (19 mouse moves handled in 8 s), one
/// kept them at 60 (309 moves); a real GPU (Metal) painted 57-60 frames a
/// second with either.
pub(crate) const MAX_FRAMES_IN_FLIGHT: u32 = 2;

/// A device whose frames take longer than this on the GPU is slow: one frame
/// in flight at a time, since a frame waiting behind it would cost the user
/// that long again in input lag. Below it a waiting frame costs little and
/// keeps the GPU busy.
pub(crate) const SLOW_FRAME: Duration = Duration::from_millis(50);

/// A slow device goes back to [`MAX_FRAMES_IN_FLIGHT`] only once its frames
/// take less than this: the gap keeps a GPU near [`SLOW_FRAME`] from
/// switching on every frame.
pub(crate) const FAST_AGAIN: Duration = Duration::from_millis(35);

/// The shortest wait for a frame's completion before the loop paints on
/// without it.
pub(crate) const MIN_STALL_WAIT: Duration = Duration::from_secs(1);

/// The longest wait for a frame's completion. A software GPU on a small CI
/// runner takes about 3 s for a full 3D frame, well inside it; a wedged GPU
/// holds painting back for at most this long, however slow one earlier frame
/// (a shader compile, a tab coming back) was.
pub(crate) const MAX_STALL_WAIT: Duration = Duration::from_secs(10);

/// How many times longer than the slowest recent frame a completion may
/// take before the frame counts as stalled.
pub(crate) const STALL_FACTOR: u32 = 4;

/// Identifies one submitted frame: the device it belongs to and its number
/// on that device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameTicket {
    generation: u64,
    frame: u64,
}

/// What [`FramesInFlight::expire`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Expired {
    /// Nothing was overdue, or a stall already under way went on.
    Nothing,
    /// Frames this long overdue were let go and a stall starts: the one
    /// moment to warn.
    StallStarted(Duration),
}

/// The frames submitted and not yet finished on the current device, each
/// with when it was submitted, and what this device's frames take.
///
/// A completion that never arrives (a GPU process that wedged or recovered
/// without reporting the device lost) would otherwise hold painting back for
/// good, so a frame unfinished for longer than [`Self::stall_wait`] stops
/// counting ([`Self::expire`]). The wait follows this device's own frame
/// times: [`STALL_FACTOR`] times the slowest recent frame, between
/// [`MIN_STALL_WAIT`] and [`MAX_STALL_WAIT`]. A frame let go also teaches the
/// wait: the device's frames take at least as long as that frame was waited
/// for, so the wait grows and the limit drops to one, and a GPU slower than a
/// second a frame (CI's software GPU) stops being treated as stalled after
/// its first frame instead of being sent two frames a second for good.
///
/// A frame's time is its own time on the GPU: from its submit, or from the
/// previous frame's completion when it waited behind that frame, to its
/// completion. Frames queued behind each other would otherwise measure the
/// wait too, and a GPU near [`SLOW_FRAME`] would switch limits every frame.
#[derive(Debug)]
pub(crate) struct FramesInFlight {
    pending: Vec<(u64, Instant)>,
    limit: usize,
    generation: u64,
    next_frame: u64,
    /// The slowest of the recent frames; it decays by a quarter at each
    /// completion so one slow frame does not set the wait for good.
    slowest: Duration,
    /// When the GPU last finished a frame on this device.
    last_done: Option<Instant>,
    /// One frame at a time: the device's frames have been slow
    /// ([`SLOW_FRAME`]) and not yet fast again ([`FAST_AGAIN`]).
    single: bool,
    /// A stall is under way: frames were let go and nothing has finished
    /// since.
    stalled: bool,
}

impl FramesInFlight {
    /// A gate allowing `limit` frames on the GPU (at least one, so the loop
    /// can always paint once the GPU is idle).
    pub(crate) const fn new(limit: u32) -> Self {
        Self {
            pending: Vec::new(),
            limit: if limit == 0 { 1 } else { limit as usize },
            generation: 0,
            next_frame: 0,
            slowest: Duration::ZERO,
            last_done: None,
            single: false,
            stalled: false,
        }
    }

    /// Whether a frame may be submitted now: fewer than the limit are on the
    /// GPU, and only one at a time on a slow device.
    pub(crate) fn can_submit(&self) -> bool {
        let limit = if self.single { 1 } else { self.limit };
        self.pending.len() < limit
    }

    /// Frames submitted and not yet finished (or let go) on this device.
    #[cfg(test)]
    pub(crate) fn in_flight(&self) -> u32 {
        self.pending.len() as u32
    }

    /// Whether the gate allows only one frame at a time now.
    #[cfg(test)]
    pub(crate) fn is_single(&self) -> bool {
        self.single
    }

    /// Sets the slowest recent frame directly, for tests of the wait's bounds.
    #[cfg(test)]
    pub(crate) fn slowest_for_test(&mut self, slowest: Duration) {
        self.learn(slowest);
    }

    /// A frame was submitted at `now`; the ticket goes to its completion
    /// callback.
    pub(crate) fn submitted(&mut self, now: Instant) -> FrameTicket {
        let frame = self.next_frame;
        self.next_frame = self.next_frame.wrapping_add(1);
        self.pending.push((frame, now));
        FrameTicket {
            generation: self.generation,
            frame,
        }
    }

    /// The GPU finished the frame `ticket` was issued for, at `now`. A
    /// completion from a replaced device changes nothing; one for a frame
    /// already let go frees nothing, but shows the GPU is working again.
    /// Returns whether a stall ended (the moment to say painting recovered).
    pub(crate) fn finished(&mut self, ticket: FrameTicket, now: Instant) -> bool {
        if ticket.generation != self.generation {
            return false;
        }
        if let Some(at) = self.pending.iter().position(|(f, _)| *f == ticket.frame) {
            let (_, submitted) = self.pending.remove(at);
            let began = self.last_done.map_or(submitted, |done| done.max(submitted));
            self.learn(now.saturating_duration_since(began));
        }
        self.last_done = Some(self.last_done.map_or(now, |done| done.max(now)));
        std::mem::take(&mut self.stalled)
    }

    /// Folds one frame's time into `slowest` and the limit.
    fn learn(&mut self, took: Duration) {
        self.slowest = took.max(self.slowest - self.slowest / 4);
        if self.slowest > SLOW_FRAME {
            self.single = true;
        } else if self.slowest < FAST_AGAIN {
            self.single = false;
        }
    }

    /// How long a frame may stay unfinished before it counts as stalled.
    pub(crate) fn stall_wait(&self) -> Duration {
        (self.slowest * STALL_FACTOR).clamp(MIN_STALL_WAIT, MAX_STALL_WAIT)
    }

    /// Lets go of every frame unfinished for longer than
    /// [`Self::stall_wait`] at `now`, counted from when it reached the front
    /// of the GPU's queue. Run on every tick.
    pub(crate) fn expire(&mut self, now: Instant) -> Expired {
        let wait = self.stall_wait();
        let before = self.pending.len();
        // A frame queued behind another starts its own time when that one
        // finishes, as its measured time does (`finished`).
        let last_done = self.last_done;
        self.pending.retain(|(_, submitted)| {
            let began = last_done.map_or(*submitted, |done| done.max(*submitted));
            now.saturating_duration_since(began) <= wait
        });
        if self.pending.len() == before {
            return Expired::Nothing;
        }
        // The device takes at least this long a frame: the next wait is
        // longer and the limit drops to one.
        self.learn(wait);
        if std::mem::replace(&mut self.stalled, true) {
            Expired::Nothing
        } else {
            Expired::StallStarted(wait)
        }
    }

    /// A device was installed (first start or a rebuild after loss, perhaps
    /// on another adapter): nothing is on it yet, frames counted against the
    /// old device never block, and its frame times are learned afresh.
    pub(crate) fn reset(&mut self) {
        self.pending.clear();
        self.generation = self.generation.wrapping_add(1);
        self.slowest = Duration::ZERO;
        self.last_done = None;
        self.single = false;
        self.stalled = false;
    }
}

/// What a tick does about painting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TickPaint {
    /// Paint and present a frame.
    Paint,
    /// The GPU is still busy with the frames allowed in flight: paint
    /// nothing and leave every redraw request pending for a later tick.
    WaitForGpu,
    /// Nothing asked for a frame.
    Idle,
}

/// The tick's paint decision. `wants_paint` (the first-paint gate and the
/// reactive predicate) is not evaluated while the GPU is full, so a tick
/// that cannot paint never consumes or promotes a pending request.
pub(crate) fn tick_paint(gate: &FramesInFlight, wants_paint: impl FnOnce() -> bool) -> TickPaint {
    if !gate.can_submit() {
        TickPaint::WaitForGpu
    } else if wants_paint() {
        TickPaint::Paint
    } else {
        TickPaint::Idle
    }
}

#[cfg(test)]
#[path = "frames_in_flight_tests.rs"]
mod tests;
