//! Tests for `frames_in_flight.rs`: painting waits while the GPU holds the
//! frames allowed in flight, the first tick after a completion paints, a
//! redraw requested while waiting is still painted, a device reset never
//! leaves painting stalled on frames the lost device will not finish, and a
//! browser that reports lone frames late (Firefox's 100 ms poll) is told from
//! a slow GPU by asking it an empty wait.

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

    /// Delivers the completions due by `now`, answering at once any empty
    /// wait one of them raises (a browser that reports promptly, as Chrome
    /// does); returns how many ended a stall.
    fn deliver(&mut self, gate: &mut FramesInFlight, now: Instant) -> usize {
        let mut recovered = 0;
        while self.queue.first().is_some_and(|(_, done)| *done <= now) {
            let (ticket, done) = self.queue.remove(0);
            recovered += usize::from(gate.finished(ticket, done));
            while gate.wants_report_probe() {
                let probe = gate.report_probe_sent(done);
                gate.report_probe_answered(probe, done + Duration::from_millis(1));
            }
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

/// A browser that reports a finished frame only when it next presents a
/// frame or when its own device poll comes round, every 100 ms (Firefox's
/// WebGPU, measured: a lone 5 ms frame resolves `onSubmittedWorkDone` up to
/// 100 ms after its submit, while a frame followed by another resolves when
/// that one is presented). The GPU itself is fast: each frame takes 5 ms.
struct PollingBrowser {
    queue: Vec<(FrameTicket, Instant)>,
    busy_until: Instant,
    /// When the browser last presented a frame, which reports every frame
    /// finished by then.
    last_present: Option<Instant>,
    /// The empty wait out, and when the browser answers it: its next poll
    /// (nothing is presented while the gate waits for it).
    probe: Option<(ProbeTicket, Instant)>,
    /// Empty waits asked so far.
    probes: usize,
}

/// How often the polling browser reports finished work on its own.
const POLL: Duration = Duration::from_millis(100);
/// The polling browser's GPU time for a frame.
const FAST_GPU_FRAME: Duration = Duration::from_millis(5);

impl PollingBrowser {
    fn new() -> Self {
        Self {
            queue: Vec::new(),
            busy_until: t0(),
            last_present: None,
            probe: None,
            probes: 0,
        }
    }

    /// The browser's first poll after `at`.
    fn next_poll_after(at: Instant) -> Instant {
        let since = at.saturating_duration_since(t0()).as_nanos();
        let poll = POLL.as_nanos();
        t0() + Duration::from_nanos((since / poll + 1) as u64 * poll as u64)
    }

    /// Sends an empty wait at `at` if the gate asks for one, as the frame
    /// loop does from the completion that raised the question.
    fn ask_if_wanted(&mut self, gate: &mut FramesInFlight, at: Instant) {
        if gate.wants_report_probe() {
            self.probes += 1;
            self.probe = Some((gate.report_probe_sent(at), Self::next_poll_after(at)));
        }
    }

    fn submit(&mut self, gate: &mut FramesInFlight, now: Instant) {
        let ticket = gate.submitted(now);
        let done = self.busy_until.max(now) + FAST_GPU_FRAME;
        self.busy_until = done;
        self.queue.push((ticket, done));
        self.last_present = Some(now);
    }

    /// When a frame finished at `done` reaches the page: the first present
    /// at or after `done`, else the first poll at or after it.
    fn reported(&self, done: Instant, now: Instant) -> Option<Instant> {
        if self.last_present.is_some_and(|p| p >= done && p <= now) {
            return self.last_present;
        }
        let since = done.saturating_duration_since(t0()).as_nanos();
        let poll = POLL.as_nanos();
        let next_poll = t0() + Duration::from_nanos(since.div_ceil(poll) as u64 * poll as u64);
        (next_poll <= now).then_some(next_poll)
    }

    /// Delivers the completions and empty-wait answers the browser has
    /// reported by `now`, in order.
    fn deliver(&mut self, gate: &mut FramesInFlight, now: Instant) {
        loop {
            let frame = self
                .queue
                .first()
                .and_then(|&(ticket, done)| Some((ticket, self.reported(done, now)?)));
            let answer = self.probe.filter(|&(_, at)| at <= now);
            match (frame, answer) {
                (Some((_, at)), Some((probe, answered))) if answered < at => {
                    self.probe = None;
                    gate.report_probe_answered(probe, answered);
                    self.ask_if_wanted(gate, answered);
                }
                (Some((ticket, at)), _) => {
                    self.queue.remove(0);
                    gate.finished(ticket, at);
                    self.ask_if_wanted(gate, at);
                }
                (None, Some((probe, answered))) => {
                    self.probe = None;
                    gate.report_probe_answered(probe, answered);
                    self.ask_if_wanted(gate, answered);
                }
                (None, None) => break,
            }
        }
    }
}

/// The frame loop at 60 Hz on the polling browser: an animation (a redraw
/// every tick) until `pause`, nothing to draw until `resume`, then another
/// animation until `end`. Returns the frames painted after `resume`.
fn animate_with_pause(gate: &mut FramesInFlight, browser: &mut PollingBrowser) -> usize {
    let tick = Duration::from_micros(16_667);
    let (pause, resume, end) = (
        t0() + Duration::from_millis(1_010),
        t0() + Duration::from_millis(1_300),
        t0() + Duration::from_millis(2_300),
    );
    let mut now = t0();
    let mut painted = 0;
    while now < end {
        browser.deliver(gate, now);
        gate.expire(now);
        let animating = now < pause || now >= resume;
        if animating && gate.can_submit() {
            browser.submit(gate, now);
            if now >= resume {
                painted += 1;
            }
        }
        now += tick;
    }
    painted
}

#[test]
fn a_fast_gpu_on_a_browser_that_reports_lone_frames_late_paints_every_tick_after_a_pause() {
    // The editor's slide on Firefox: the last frame before the pause had no
    // frame after it, so it was reported at the browser's next poll.
    let mut gate = FramesInFlight::new(MAX_FRAMES_IN_FLIGHT);
    gate.enable_report_probes(true);
    let mut browser = PollingBrowser::new();
    let painted = animate_with_pause(&mut gate, &mut browser);
    assert!(
        painted >= 55,
        "a second of animation at 60 Hz on a 5 ms GPU paints about 60 frames: {painted}"
    );
    assert!(gate.reports_late());
    assert_eq!(
        browser.probes, LATE_ANSWERS as usize,
        "asked once per answer needed, during the pause"
    );
}

#[test]
fn a_slow_gpu_on_a_browser_that_answers_promptly_still_runs_one_frame_at_a_time() {
    // CI's software GPU in Chrome, with the empty waits on: every lone frame
    // is slow, every empty wait is answered at once.
    let mut gate = FramesInFlight::new(MAX_FRAMES_IN_FLIGHT);
    gate.enable_report_probes(true);
    let mut gpu = GpuModel::new(Duration::from_secs(3));
    let (warnings, deepest) = run_loop(
        &mut gate,
        &mut gpu,
        Duration::from_millis(16),
        Duration::from_secs(120),
        Duration::from_secs(15),
    );
    assert_eq!(warnings, 1);
    assert!(deepest <= 1, "one frame on the GPU at a time: {deepest}");
    assert!(gate.is_single());
    assert!(!gate.reports_late());
}

/// A lone frame finished `took` after its submit, on a gate asking empty
/// waits; returns the gate with the question open.
fn lone_slow_frame(took: Duration) -> FramesInFlight {
    let mut gate = FramesInFlight::new(MAX_FRAMES_IN_FLIGHT);
    gate.enable_report_probes(true);
    let t = gate.submitted(t0());
    gate.finished(t, t0() + took);
    gate
}

#[test]
fn a_lone_slow_frame_holds_painting_until_the_browser_answers() {
    let mut gate = lone_slow_frame(Duration::from_millis(100));
    assert!(gate.wants_report_probe());
    assert!(!gate.can_submit(), "a present now would hurry the answer");
    let mut p = Pending { dirty: true };
    assert_eq!(p.tick(&mut gate).0, TickPaint::WaitForGpu);
    let sent = t0() + Duration::from_millis(100);
    let probe = gate.report_probe_sent(sent);
    assert!(!gate.wants_report_probe(), "one wait out at a time");
    gate.report_probe_answered(probe, sent + Duration::from_millis(1));
    assert!(gate.can_submit());
    assert!(gate.is_single(), "answered promptly: the frame was slow");
    assert!(!gate.reports_late());
}

#[test]
fn one_late_answer_is_not_enough() {
    let mut gate = lone_slow_frame(Duration::from_millis(100));
    let mut at = t0() + Duration::from_millis(100);
    let first = gate.report_probe_sent(at);
    at += Duration::from_millis(174);
    gate.report_probe_answered(first, at);
    assert!(gate.wants_report_probe(), "asked again");
    let second = gate.report_probe_sent(at);
    gate.report_probe_answered(second, at + Duration::from_millis(2));
    assert!(!gate.reports_late());
    assert!(
        gate.is_single(),
        "the second answer was prompt: the frame was slow"
    );
}

#[test]
fn a_browser_found_late_never_drops_to_one_frame() {
    let mut gate = lone_slow_frame(Duration::from_millis(100));
    let mut at = t0() + Duration::from_millis(100);
    for _ in 0..LATE_ANSWERS {
        let probe = gate.report_probe_sent(at);
        at += POLL;
        gate.report_probe_answered(probe, at);
    }
    assert!(gate.reports_late());
    assert!(gate.can_submit());
    // Lone frames are passed over without asking again, and a followed frame
    // slower than SLOW_FRAME does not drop the limit either.
    let lone = gate.submitted(at);
    at += POLL;
    gate.finished(lone, at);
    assert!(!gate.wants_report_probe());
    let followed = gate.submitted(at);
    let _next = gate.submitted(at);
    at += Duration::from_millis(80);
    gate.finished(followed, at);
    assert!(!gate.is_single());
    assert!(gate.can_submit(), "one of two in flight");
}

#[test]
fn an_empty_wait_never_answered_is_given_up_after_the_stall_wait() {
    let mut gate = lone_slow_frame(Duration::from_millis(100));
    let sent = t0() + Duration::from_millis(100);
    let probe = gate.report_probe_sent(sent);
    let wait = gate.stall_wait();
    gate.expire(sent + wait);
    assert!(!gate.can_submit(), "still asking at the bound");
    gate.expire(sent + wait + Duration::from_millis(1));
    assert!(gate.can_submit(), "painting goes on");
    assert!(gate.is_single(), "the lone frame learned as measured");
    // Its answer after all changes nothing.
    gate.report_probe_answered(probe, sent + wait * 2);
    assert!(!gate.reports_late());
}

#[test]
fn a_new_device_asks_its_browser_afresh() {
    let mut gate = lone_slow_frame(Duration::from_millis(100));
    let stale = gate.report_probe_sent(t0());
    gate.reset();
    assert!(gate.can_submit(), "the question went with the old device");
    gate.report_probe_answered(stale, t0() + POLL);
    assert!(!gate.reports_late());
}

#[test]
fn without_empty_waits_a_lone_slow_frame_is_learned_at_once() {
    // wgpu's WebGL2 backend: completions come from the shell's own polling.
    let mut gate = FramesInFlight::new(MAX_FRAMES_IN_FLIGHT);
    let t = gate.submitted(t0());
    gate.finished(t, t0() + Duration::from_millis(100));
    assert!(!gate.wants_report_probe());
    assert!(gate.is_single());
}
