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
    assert_eq!(gate.expire(t0() + wait), 0, "not stalled yet at the bound");
    assert_eq!(p.tick(&mut gate).0, TickPaint::WaitForGpu);
    assert_eq!(gate.expire(t0() + wait + Duration::from_millis(1)), 1);
    assert_eq!(p.tick(&mut gate).0, TickPaint::Paint, "painting goes on");
    // The stuck frame's completion arrives after all: it frees nothing the
    // frame painted since holds.
    gate.finished(stuck, t0() + wait * 2);
    assert_eq!(gate.in_flight(), 1);
    assert!(!gate.can_submit());
}

#[test]
fn the_stall_wait_follows_the_devices_own_frame_times() {
    let mut gate = FramesInFlight::new(1);
    // A software GPU: each frame takes three seconds.
    let frame = Duration::from_secs(3);
    let mut now = t0();
    for _ in 0..3 {
        let t = gate.submitted(now);
        now += frame;
        gate.finished(t, now);
    }
    assert_eq!(gate.stall_wait(), frame * STALL_FACTOR);
    // A frame as slow as the others is not let go.
    gate.submitted(now);
    assert_eq!(gate.expire(now + frame * 2), 0);
    assert_eq!(gate.expire(now + frame * STALL_FACTOR + frame), 1);
}

#[test]
fn one_slow_frame_does_not_set_the_wait_for_good() {
    let mut gate = FramesInFlight::new(1);
    let mut now = t0();
    let slow = gate.submitted(now);
    now += Duration::from_secs(10);
    gate.finished(slow, now);
    for _ in 0..40 {
        let t = gate.submitted(now);
        now += Duration::from_millis(5);
        gate.finished(t, now);
    }
    assert_eq!(gate.stall_wait(), MIN_STALL_WAIT);
}

#[test]
fn a_fast_gpu_keeps_two_frames_in_flight_and_a_slow_one_keeps_one() {
    let mut gate = FramesInFlight::new(MAX_FRAMES_IN_FLIGHT);
    assert_eq!(MAX_FRAMES_IN_FLIGHT, 2);
    let mut now = t0();
    // Fast frames: two may be on the GPU at once.
    for _ in 0..3 {
        let t = gate.submitted(now);
        now += Duration::from_millis(9);
        gate.finished(t, now);
    }
    gate.submitted(now);
    assert!(gate.can_submit(), "a second fast frame may follow");
    let a = gate.submitted(now);
    assert!(!gate.can_submit(), "but not a third");
    gate.finished(a, now + Duration::from_millis(9));
    // A frame that takes longer than SLOW_FRAME makes it one at a time.
    let mut gate = FramesInFlight::new(MAX_FRAMES_IN_FLIGHT);
    let t = gate.submitted(now);
    gate.finished(t, now + SLOW_FRAME + Duration::from_millis(1));
    gate.submitted(now);
    assert!(!gate.can_submit(), "a slow GPU gets one frame at a time");
}
