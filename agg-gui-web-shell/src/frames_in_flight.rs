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

/// A device whose recent frames took longer than this is slow: one frame in
/// flight at a time, since a frame waiting behind it would cost the user
/// that long again in input lag. Below it a waiting frame costs little and
/// keeps the GPU busy.
pub(crate) const SLOW_FRAME: Duration = Duration::from_millis(50);

/// The shortest wait for a frame's completion before the loop paints on
/// without it.
pub(crate) const MIN_STALL_WAIT: Duration = Duration::from_secs(1);

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

/// The frames submitted and not yet finished on the current device, each
/// with when it was submitted.
///
/// A completion that never arrives (a GPU process that wedged or recovered
/// without reporting the device lost) would otherwise hold painting back for
/// good, so a frame unfinished for longer than [`Self::stall_wait`] stops
/// counting ([`Self::expire`]). The wait follows this device's own frame
/// times: [`STALL_FACTOR`] times the slowest of the recent frames, and at
/// least [`MIN_STALL_WAIT`]. A software GPU on a small CI runner takes
/// several seconds for a full 3D frame, and a fixed second would count every
/// one of those as stalled and bring the backlog back; a real GPU finishes in
/// milliseconds, so there a stuck frame is let go after one second.
#[derive(Debug)]
pub(crate) struct FramesInFlight {
    pending: Vec<(u64, Instant)>,
    limit: usize,
    generation: u64,
    next_frame: u64,
    /// The slowest of the recent frames; it decays by a quarter at each
    /// completion so one slow frame does not set the wait for good.
    slowest: Duration,
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
        }
    }

    /// Whether a frame may be submitted now: fewer than the limit are on the
    /// GPU, and only one at a time on a slow device ([`SLOW_FRAME`]).
    pub(crate) fn can_submit(&self) -> bool {
        let limit = if self.slowest > SLOW_FRAME {
            1
        } else {
            self.limit
        };
        self.pending.len() < limit
    }

    /// Frames submitted and not yet finished on the current device.
    #[cfg(test)]
    pub(crate) fn in_flight(&self) -> u32 {
        self.pending.len() as u32
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
    /// completion from a replaced device, or for a frame already let go as
    /// stalled, changes nothing.
    pub(crate) fn finished(&mut self, ticket: FrameTicket, now: Instant) {
        if ticket.generation != self.generation {
            return;
        }
        if let Some(at) = self.pending.iter().position(|(f, _)| *f == ticket.frame) {
            let (_, submitted) = self.pending.remove(at);
            let took = now.saturating_duration_since(submitted);
            self.slowest = took.max(self.slowest - self.slowest / 4);
        }
    }

    /// How long a frame may stay unfinished before it counts as stalled.
    pub(crate) fn stall_wait(&self) -> Duration {
        (self.slowest * STALL_FACTOR).max(MIN_STALL_WAIT)
    }

    /// Lets go of every frame unfinished for longer than
    /// [`Self::stall_wait`] at `now`, and answers how many it let go.
    pub(crate) fn expire(&mut self, now: Instant) -> usize {
        let wait = self.stall_wait();
        let before = self.pending.len();
        self.pending
            .retain(|(_, submitted)| now.saturating_duration_since(*submitted) <= wait);
        before - self.pending.len()
    }

    /// A device was installed (first start or a rebuild after loss): nothing
    /// is on it yet, and frames counted against the old device never block.
    pub(crate) fn reset(&mut self) {
        self.pending.clear();
        self.generation = self.generation.wrapping_add(1);
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
