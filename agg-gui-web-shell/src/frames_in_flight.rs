//! The frames-in-flight gate: how many presented frames may still be running
//! on the GPU before the frame loop stops submitting new ones.
//!
//! `requestAnimationFrame` keeps firing at the display rate even when the GPU
//! takes longer than a refresh to finish a frame (a software GPU such as
//! SwiftShader takes hundreds of milliseconds for a full 3D frame). A loop
//! that submits on every tick with a redraw pending then queues frames ahead
//! of the GPU: what is on screen falls further behind the input that caused
//! it, and the browser's own work that waits on the GPU (a page's smooth
//! scroll) stalls behind the backlog. So the frame loop in the crate's
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
//! Platform-neutral so `cargo test` reaches the production state machine.

/// Presented frames that may be on the GPU at once before the loop waits.
///
/// One is enough for a fast GPU: its frame finishes well inside a refresh,
/// so the completion resolves before the next animation frame and the loop
/// still paints every refresh (measured on Metal: 57-60 painted frames a
/// second while dragging the camera of a 3D view, the same as without the
/// limit). On a software GPU it keeps the backlog to the frame being drawn.
pub(crate) const MAX_FRAMES_IN_FLIGHT: u32 = 1;

/// Identifies the device a submitted frame belongs to; completions from an
/// older device are ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameTicket {
    generation: u64,
}

/// Submitted-but-unfinished frame count for the current device.
#[derive(Debug)]
pub(crate) struct FramesInFlight {
    in_flight: u32,
    limit: u32,
    generation: u64,
}

impl FramesInFlight {
    /// A gate allowing `limit` frames on the GPU (at least one, so the loop
    /// can always paint once the GPU is idle).
    pub(crate) const fn new(limit: u32) -> Self {
        Self {
            in_flight: 0,
            limit: if limit == 0 { 1 } else { limit },
            generation: 0,
        }
    }

    /// Whether a frame may be submitted now.
    pub(crate) fn can_submit(&self) -> bool {
        self.in_flight < self.limit
    }

    /// Frames submitted and not yet finished on the current device.
    #[cfg(test)]
    pub(crate) fn in_flight(&self) -> u32 {
        self.in_flight
    }

    /// A frame was submitted; the ticket goes to its completion callback.
    pub(crate) fn submitted(&mut self) -> FrameTicket {
        self.in_flight = self.in_flight.saturating_add(1);
        FrameTicket {
            generation: self.generation,
        }
    }

    /// The GPU finished the frame `ticket` was issued for. A completion from
    /// a device that has since been replaced changes nothing.
    pub(crate) fn finished(&mut self, ticket: FrameTicket) {
        if ticket.generation == self.generation {
            self.in_flight = self.in_flight.saturating_sub(1);
        }
    }

    /// A device was installed (first start or a rebuild after loss): nothing
    /// is on it yet, and frames counted against the old device never block.
    pub(crate) fn reset(&mut self) {
        self.in_flight = 0;
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
