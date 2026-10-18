//! Tests for `frames_in_flight.rs`: painting waits while the GPU holds the
//! frames allowed in flight, the first tick after a completion paints, a
//! redraw requested while waiting is still painted, and a device reset never
//! leaves painting stalled on frames the lost device will not finish.

use super::*;

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
                Some(gate.submitted())
            }
            TickPaint::WaitForGpu | TickPaint::Idle => None,
        };
        (decision, ticket)
    }
}

#[test]
fn an_idle_gpu_paints_a_pending_redraw() {
    let mut gate = FramesInFlight::new(MAX_FRAMES_IN_FLIGHT);
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
    let _a = gate.submitted();
    assert!(gate.can_submit(), "one of two in flight");
    let _b = gate.submitted();
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
    gate.submitted();
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
    gate.finished(ticket.expect("the painted frame has a ticket"));
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
    let a = gate.submitted();
    let b = gate.submitted();
    gate.finished(b);
    assert!(gate.can_submit());
    gate.finished(a);
    assert_eq!(gate.in_flight(), 0);
    // A stray extra completion never underflows the count.
    gate.finished(a);
    assert_eq!(gate.in_flight(), 0);
}

#[test]
fn a_device_reset_unblocks_frames_the_lost_device_never_finishes() {
    let mut gate = FramesInFlight::new(1);
    let lost = gate.submitted();
    assert!(!gate.can_submit());
    // The device is lost; its frame's promise never resolves. The rebuilt
    // device is installed.
    gate.reset();
    let mut p = Pending { dirty: true };
    let (decision, fresh) = p.tick(&mut gate);
    assert_eq!(decision, TickPaint::Paint, "the new device paints at once");
    // The old device's completion arrives late: it must not free the new
    // device's slot.
    gate.finished(lost);
    assert_eq!(gate.in_flight(), 1);
    assert!(!gate.can_submit());
    gate.finished(fresh.expect("painted"));
    assert!(gate.can_submit());
}

#[test]
fn a_zero_limit_still_paints_once_the_gpu_is_idle() {
    let mut gate = FramesInFlight::new(0);
    assert!(gate.can_submit());
    let t = gate.submitted();
    assert!(!gate.can_submit());
    gate.finished(t);
    assert!(gate.can_submit());
}
