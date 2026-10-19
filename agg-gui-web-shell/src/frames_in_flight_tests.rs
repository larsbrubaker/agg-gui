//! Tests for `frames_in_flight.rs`: painting waits while the GPU holds the
//! frames allowed in flight, the first tick after a completion paints, a
//! redraw requested while waiting is still painted, and a device reset never
//! leaves painting stalled on frames the lost device will not finish.

use super::*;

use std::sync::OnceLock;

/// One fixed instant the tests count from.
fn t0() -> Instant {
    static T0: OnceLock<Instant> = OnceLock::new();
    *T0.get_or_init(Instant::now)
}

/// A redraw request the way the frame loop holds it: a flag a paint clears.
struct Pending {
    dirty: bool,
}

impl Pending {
    /// One tick of the loop: decide, and on `Paint` submit a frame and clear
    /// the request (as `paint` clears `DIRTY` and the app's draw flags).
    fn tick(&mut self, gate: &mut FramesInFlight) -> (TickPaint, Option<FrameTicket>) {
        let decision = tick_paint(gate, || self.dirty);
        let ticket = match decision {
            TickPaint::Paint => {
                self.dirty = false;
                Some(gate.submitted(t0()))
            }
            TickPaint::WaitForGpu | TickPaint::Idle => None,
        };
        (decision, ticket)
    }
}

#[test]
fn an_idle_gpu_paints_a_pending_redraw() {
    let mut gate = FramesInFlight::new(1);
    let mut p = Pending { dirty: true };
    assert_eq!(p.tick(&mut gate).0, TickPaint::Paint);
    assert_eq!(p.tick(&mut gate).0, TickPaint::WaitForGpu);
}

#[test]
fn nothing_pending_paints_nothing() {
    let mut gate = FramesInFlight::new(MAX_FRAMES_IN_FLIGHT);
    let mut p = Pending { dirty: false };
    assert_eq!(p.tick(&mut gate), (TickPaint::Idle, None));
    assert_eq!(gate.in_flight(), 0);
}

#[test]
fn painting_waits_while_the_limit_is_on_the_gpu() {
    let mut gate = FramesInFlight::new(2);
    assert!(gate.can_submit());
    let _a = gate.submitted(t0());
    assert!(gate.can_submit(), "one of two in flight");
    let _b = gate.submitted(t0());
    assert!(!gate.can_submit(), "two of two in flight");
    let mut p = Pending { dirty: true };
    for _ in 0..10 {
        assert_eq!(p.tick(&mut gate), (TickPaint::WaitForGpu, None));
    }
    assert_eq!(gate.in_flight(), 2, "waiting ticks submit nothing");
}

#[test]
fn the_wait_never_reads_the_paint_request() {
    let mut gate = FramesInFlight::new(1);
    gate.submitted(t0());
    let mut asked = false;
    let decision = tick_paint(&gate, || {
        asked = true;
        true
    });
    assert_eq!(decision, TickPaint::WaitForGpu);
    assert!(
        !asked,
        "a due deadline is not promoted by a tick that cannot paint"
    );
}

#[test]
fn a_redraw_requested_while_waiting_paints_on_the_first_tick_after_the_gpu_finishes() {
    let mut gate = FramesInFlight::new(1);
    let mut p = Pending { dirty: true };
    let (first, ticket) = p.tick(&mut gate);
    assert_eq!(first, TickPaint::Paint);
    // Input arrives while that frame is still on the GPU.
    p.dirty = true;
    for _ in 0..20 {
        assert_eq!(p.tick(&mut gate).0, TickPaint::WaitForGpu);
        assert!(p.dirty, "the request is kept while waiting");
    }
    gate.finished(ticket.expect("the painted frame has a ticket"), t0());
    assert_eq!(
        p.tick(&mut gate).0,
        TickPaint::Paint,
        "first tick after done"
    );
    assert!(!p.dirty);
    assert_eq!(gate.in_flight(), 1);
}

#[test]
fn completions_out_of_order_free_one_slot_each() {
    let mut gate = FramesInFlight::new(2);
    let a = gate.submitted(t0());
    let b = gate.submitted(t0());
    gate.finished(b, t0());
    assert!(gate.can_submit());
    gate.finished(a, t0());
    assert_eq!(gate.in_flight(), 0);
    // A stray extra completion never underflows the count.
    gate.finished(a, t0());
    assert_eq!(gate.in_flight(), 0);
}

#[test]
fn a_device_reset_unblocks_frames_the_lost_device_never_finishes() {
    let mut gate = FramesInFlight::new(1);
    let lost = gate.submitted(t0());
    assert!(!gate.can_submit());
    // The device is lost; its frame's promise never resolves. The rebuilt
    // device is installed.
    gate.reset();
    let mut p = Pending { dirty: true };
    let (decision, fresh) = p.tick(&mut gate);
    assert_eq!(decision, TickPaint::Paint, "the new device paints at once");
    // The old device's completion arrives late: it must not free the new
    // device's slot.
    gate.finished(lost, t0());
    assert_eq!(gate.in_flight(), 1);
    assert!(!gate.can_submit());
    gate.finished(fresh.expect("painted"), t0());
    assert!(gate.can_submit());
}

#[test]
fn a_zero_limit_still_paints_once_the_gpu_is_idle() {
    let mut gate = FramesInFlight::new(0);
    assert!(gate.can_submit());
    let t = gate.submitted(t0());
    assert!(!gate.can_submit());
    gate.finished(t, t0());
    assert!(gate.can_submit());
}

#[test]
fn a_frame_that_never_finishes_stops_blocking_after_the_stall_wait() {
    let mut gate = FramesInFlight::new(1);
    let stuck = gate.submitted(t0());
    let mut p = Pending { dirty: true };
    let wait = gate.stall_wait();
    assert_eq!(wait, MIN_STALL_WAIT, "no frame has finished yet");
    assert_eq!(
        gate.expire(t0() + wait),
        Expired::Nothing,
        "not stalled at the bound"
    );
    assert_eq!(p.tick(&mut gate).0, TickPaint::WaitForGpu);
    let late = t0() + wait + Duration::from_millis(1);
    assert_eq!(gate.expire(late), Expired::StallStarted(wait));
    assert_eq!(p.tick(&mut gate).0, TickPaint::Paint, "painting goes on");
    // The stuck frame's completion arrives after all: it frees nothing the
    // frame painted since holds, and ends the stall.
    assert!(gate.finished(stuck, t0() + wait * 2), "the stall ended");
    assert_eq!(gate.in_flight(), 1);
    assert!(!gate.can_submit());
}

/// The GPU as the gate sees it: frames run one after another, each taking
/// `service`, and each completion reaches the page when its frame is done.
struct GpuModel {
    service: Duration,
    queue: Vec<(FrameTicket, Instant)>,
    busy_until: Instant,
}

impl GpuModel {
    fn new(service: Duration) -> Self {
        Self {
            service,
            queue: Vec::new(),
            busy_until: t0(),
        }
    }

    fn submit(&mut self, gate: &mut FramesInFlight, now: Instant) {
        let ticket = gate.submitted(now);
        let done = self.busy_until.max(now) + self.service;
        self.busy_until = done;
        self.queue.push((ticket, done));
    }

    /// Delivers the completions due by `now`; returns how many ended a stall.
    fn deliver(&mut self, gate: &mut FramesInFlight, now: Instant) -> usize {
        let mut recovered = 0;
        while self.queue.first().is_some_and(|(_, done)| *done <= now) {
            let (ticket, done) = self.queue.remove(0);
            recovered += usize::from(gate.finished(ticket, done));
        }
        recovered
    }

    /// Frames submitted the GPU has not finished.
    fn backlog(&self) -> usize {
        self.queue.len()
    }
}

/// The frame loop with a redraw always pending: every `tick`, completions
/// arrive, overdue frames are let go, and a frame is submitted if the gate
/// allows. Returns the stall warnings, and the deepest GPU backlog after
/// `settle`.
fn run_loop(
    gate: &mut FramesInFlight,
    gpu: &mut GpuModel,
    tick: Duration,
    run: Duration,
    settle: Duration,
) -> (usize, usize) {
    let mut now = t0();
    let mut warnings = 0;
    let mut deepest = 0;
    while now < t0() + run {
        gpu.deliver(gate, now);
        if let Expired::StallStarted(_) = gate.expire(now) {
            warnings += 1;
        }
        if gate.can_submit() {
            gpu.submit(gate, now);
        }
        if now >= t0() + settle {
            deepest = deepest.max(gpu.backlog());
        }
        now += tick;
    }
    (warnings, deepest)
}

#[test]
fn a_gpu_slower_than_the_first_wait_is_learned_and_its_backlog_stays_bounded() {
    // CI's software GPU: three seconds a frame, more than the first wait.
    let mut gate = FramesInFlight::new(MAX_FRAMES_IN_FLIGHT);
    let mut gpu = GpuModel::new(Duration::from_secs(3));
    let tick = Duration::from_millis(16);
    let (warnings, deepest) = run_loop(
        &mut gate,
        &mut gpu,
        tick,
        Duration::from_secs(120),
        Duration::from_secs(15),
    );
    assert_eq!(
        warnings, 1,
        "one warning, for the first frame, before the wait learned"
    );
    assert!(
        deepest <= 1,
        "one frame on the GPU at a time once learned: {deepest}"
    );
    assert!(
        gate.stall_wait() > Duration::from_secs(3),
        "{:?}",
        gate.stall_wait()
    );
}

#[test]
fn a_steady_thirty_millisecond_gpu_keeps_two_frames_in_flight() {
    let mut gate = FramesInFlight::new(MAX_FRAMES_IN_FLIGHT);
    let mut gpu = GpuModel::new(Duration::from_millis(30));
    let tick = Duration::from_micros(8_333);
    let mut now = t0();
    let mut switches = 0;
    let mut last = None;
    while now < t0() + Duration::from_secs(10) {
        gpu.deliver(&mut gate, now);
        gate.expire(now);
        if gate.can_submit() {
            gpu.submit(&mut gate, now);
        }
        let single = gate.is_single();
        if last.is_some_and(|l| l != single) {
            switches += 1;
        }
        last = Some(single);
        now += tick;
    }
    assert_eq!(switches, 0, "the limit never switched");
    assert_eq!(
        last,
        Some(false),
        "two in flight: each frame's own time is 30 ms"
    );
}

#[test]
fn a_gpu_near_the_slow_line_does_not_switch_back_and_forth() {
    let mut gate = FramesInFlight::new(MAX_FRAMES_IN_FLIGHT);
    let mut now = t0();
    let mut frame = |gate: &mut FramesInFlight, ms: u64| {
        let t = gate.submitted(now);
        now += Duration::from_millis(ms);
        gate.finished(t, now);
    };
    frame(&mut gate, 55);
    assert!(gate.is_single(), "slower than SLOW_FRAME: one at a time");
    for _ in 0..20 {
        frame(&mut gate, 45);
        assert!(gate.is_single(), "45 ms is not fast enough to go back");
    }
    for _ in 0..20 {
        frame(&mut gate, 10);
    }
    assert!(!gate.is_single(), "fast again: two in flight");
}

#[test]
fn one_slow_frame_does_not_set_the_wait_for_good() {
    let mut gate = FramesInFlight::new(1);
    let mut now = t0();
    let slow = gate.submitted(now);
    now += Duration::from_millis(900);
    gate.finished(slow, now);
    for _ in 0..40 {
        let t = gate.submitted(now);
        now += Duration::from_millis(5);
        gate.finished(t, now);
    }
    assert_eq!(gate.stall_wait(), MIN_STALL_WAIT);
}

#[test]
fn the_stall_wait_is_capped_after_one_very_slow_frame() {
    let mut gate = FramesInFlight::new(1);
    // One frame of a minute (a shader compile, a tab coming back), seen
    // through: the wait grows only to the cap.
    gate.slowest_for_test(Duration::from_secs(60));
    assert_eq!(gate.stall_wait(), MAX_STALL_WAIT);
    // Then the GPU wedges: painting goes on after the cap, not four minutes.
    let now = t0();
    gate.submitted(now);
    assert_eq!(gate.expire(now + MAX_STALL_WAIT), Expired::Nothing);
    assert_eq!(
        gate.expire(now + MAX_STALL_WAIT + Duration::from_millis(1)),
        Expired::StallStarted(MAX_STALL_WAIT)
    );
    assert!(gate.can_submit());
}

#[test]
fn a_wedged_gpu_warns_once_however_long_it_lasts() {
    let mut gate = FramesInFlight::new(1);
    let mut gpu = GpuModel::new(Duration::from_secs(3600));
    let (warnings, _) = run_loop(
        &mut gate,
        &mut gpu,
        Duration::from_millis(16),
        Duration::from_secs(120),
        Duration::ZERO,
    );
    assert_eq!(warnings, 1);
}

#[test]
fn a_device_reset_learns_frame_times_afresh() {
    let mut gate = FramesInFlight::new(MAX_FRAMES_IN_FLIGHT);
    let t = gate.submitted(t0());
    gate.finished(t, t0() + Duration::from_secs(3));
    assert!(gate.is_single());
    assert!(gate.stall_wait() > MIN_STALL_WAIT);
    gate.reset();
    assert!(!gate.is_single(), "a new device starts with two in flight");
    assert_eq!(gate.stall_wait(), MIN_STALL_WAIT);
}
