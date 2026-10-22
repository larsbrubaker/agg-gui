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
//! A browser can report a finished frame late. Firefox's WebGPU resolves
//! `onSubmittedWorkDone` when it next presents a frame or when its own device
//! poll comes round, every 100 ms: a frame with another presented after it is
//! reported promptly, but a lone frame (the last before a pause, or every
//! frame once the gate allows only one) is reported up to 100 ms after it
//! finished, however fast the GPU. Timed as GPU work, those reports made a
//! fast GPU look slow, the gate allowed one frame at a time, and with nothing
//! presented behind it every frame then waited for the poll: about ten frames
//! a second for good. So a lone frame that looks slow raises a question
//! before it is learned ([`FramesInFlight::wants_report_probe`]): the frame
//! loop asks the browser for an empty `onSubmittedWorkDone`, with nothing on
//! the GPU and painting held so no present hurries the answer. Two empty
//! waits in a row answered late show the delay is the browser's
//! ([`LATE_ANSWERS`]): from then on lone frames are not timed, and the gate
//! keeps two frames in flight, the browser pacing its animation frames to a
//! slow GPU itself (measured in Firefox: a 100 ms-a-frame shader slowed its
//! animation frames to 140 ms, with at most three frames on the GPU). An
//! empty wait answered promptly shows the frame really was slow, and it is
//! learned as any other.
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

/// Empty waits that must come back slower than [`SLOW_FRAME`] in a row
/// before the browser is taken to report finished frames late. An empty wait
/// has no GPU work, so in Chrome it resolves in under 2 ms, and in Firefox
/// after its 100 ms poll every time; one alone can still be held up by a busy
/// browser (Chrome once took 174 ms over an empty submit while its GPU
/// process was starting), two in a row on an empty queue are not.
pub(crate) const LATE_ANSWERS: u32 = 2;

/// Identifies one submitted frame: the device it belongs to and its number
/// on that device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameTicket {
    generation: u64,
    frame: u64,
}

/// Identifies one empty wait sent to ask how late the browser reports: the
/// device it was sent on and its number there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProbeTicket {
    generation: u64,
    probe: u64,
}

/// The question a lone slow frame raised: is the browser late to report, or
/// was the frame slow?
#[derive(Clone, Copy, Debug)]
struct ReportProbe {
    /// The lone frame's measured time, learned if the browser answers
    /// promptly.
    deferred: Duration,
    /// Empty waits answered late so far.
    late: u32,
    /// The number of the empty wait out now, and when it was sent; `None`
    /// until the frame loop sends the next one.
    sent: Option<(u64, Instant)>,
    /// When the question was raised or last answered, for giving up on a
    /// wait the frame loop never sent.
    since: Instant,
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
    /// Whether the frame loop can send empty waits on this device
    /// ([`Self::enable_report_probes`]).
    report_probes: bool,
    /// The question a lone slow frame raised, while it is open.
    probe: Option<ReportProbe>,
    next_probe: u64,
    /// The browser reports a lone frame late: lone frames are not timed and
    /// the gate never drops to one frame in flight.
    late_reports: bool,
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
            report_probes: false,
            probe: None,
            next_probe: 0,
            late_reports: false,
        }
    }

    /// Whether the frame loop can ask this device how late its completions
    /// arrive. Off (the default) a lone slow frame is learned at once, as on
    /// wgpu's WebGL2 backend, where completions come from the shell's own
    /// polling rather than the browser's.
    pub(crate) fn enable_report_probes(&mut self, on: bool) {
        self.report_probes = on;
        if !on {
            self.probe = None;
        }
    }

    /// Whether a frame may be submitted now: fewer than the limit are on the
    /// GPU, only one at a time on a slow device, and none while the browser
    /// is being asked how late it reports (a frame presented then would
    /// hurry its answer).
    pub(crate) fn can_submit(&self) -> bool {
        if self.probe.is_some() {
            return false;
        }
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

    /// Whether the browser has been found to report lone frames late.
    #[cfg(test)]
    pub(crate) fn reports_late(&self) -> bool {
        self.late_reports
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
            // Nothing was submitted after it, so no later present prompted
            // its report.
            let lone = ticket.frame.wrapping_add(1) == self.next_frame;
            self.learn_frame(now.saturating_duration_since(began), lone, now);
        }
        self.last_done = Some(self.last_done.map_or(now, |done| done.max(now)));
        std::mem::take(&mut self.stalled)
    }

    /// Learns a finished frame's time, unless the browser's reporting may be
    /// what made a lone frame slow: then the question is raised first, or,
    /// once the browser is known to report late, the time is passed over.
    fn learn_frame(&mut self, took: Duration, lone: bool, now: Instant) {
        if lone && self.late_reports {
            return;
        }
        if lone && took > SLOW_FRAME && self.report_probes && self.probe.is_none() {
            self.probe = Some(ReportProbe {
                deferred: took,
                late: 0,
                sent: None,
                since: now,
            });
            return;
        }
        self.learn(took);
    }

    /// Whether the frame loop should send an empty wait now
    /// ([`Self::report_probe_sent`]): a question is open and no wait is out.
    pub(crate) fn wants_report_probe(&self) -> bool {
        self.probe.is_some_and(|p| p.sent.is_none())
    }

    /// An empty wait was sent at `now`; the ticket goes to its answer.
    pub(crate) fn report_probe_sent(&mut self, now: Instant) -> ProbeTicket {
        let probe = self.next_probe;
        self.next_probe = self.next_probe.wrapping_add(1);
        if let Some(p) = &mut self.probe {
            p.sent = Some((probe, now));
        }
        ProbeTicket {
            generation: self.generation,
            probe,
        }
    }

    /// The empty wait `ticket` was issued for resolved at `now`. Late, it
    /// counts towards [`LATE_ANSWERS`] (and another is wanted until they are
    /// reached); prompt, the lone frame really was slow and is learned. An
    /// answer from a replaced device, or to a question already given up,
    /// changes nothing.
    pub(crate) fn report_probe_answered(&mut self, ticket: ProbeTicket, now: Instant) {
        if ticket.generation != self.generation {
            return;
        }
        let Some(probe) = self.probe else {
            return;
        };
        let Some((sent_probe, sent)) = probe.sent else {
            return;
        };
        if sent_probe != ticket.probe {
            return;
        }
        if now.saturating_duration_since(sent) <= SLOW_FRAME {
            self.probe = None;
            self.learn(probe.deferred);
        } else if probe.late + 1 >= LATE_ANSWERS {
            self.probe = None;
            self.late_reports = true;
            self.single = false;
        } else {
            self.probe = Some(ReportProbe {
                late: probe.late + 1,
                sent: None,
                since: now,
                ..probe
            });
        }
    }

    /// Folds one frame's time into `slowest` and the limit. Once the browser
    /// is known to report late the limit stays at two: its frame times are
    /// not the GPU's alone, and one frame in flight would leave every frame
    /// waiting for the browser's poll.
    fn learn(&mut self, took: Duration) {
        self.slowest = took.max(self.slowest - self.slowest / 4);
        if self.late_reports {
            self.single = false;
        } else if self.slowest > SLOW_FRAME {
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
    /// of the GPU's queue, and gives up on an empty wait out that long (the
    /// lone frame is then learned as measured). Run on every tick.
    pub(crate) fn expire(&mut self, now: Instant) -> Expired {
        let wait = self.stall_wait();
        if let Some(probe) = self.probe {
            let asked = probe.sent.map_or(probe.since, |(_, sent)| sent);
            if now.saturating_duration_since(asked) > wait {
                self.probe = None;
                self.learn(probe.deferred);
            }
        }
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
    /// old device never block, and its frame times, and how late its browser
    /// reports, are learned afresh.
    pub(crate) fn reset(&mut self) {
        self.pending.clear();
        self.generation = self.generation.wrapping_add(1);
        self.slowest = Duration::ZERO;
        self.last_done = None;
        self.single = false;
        self.stalled = false;
        self.probe = None;
        self.late_reports = false;
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
