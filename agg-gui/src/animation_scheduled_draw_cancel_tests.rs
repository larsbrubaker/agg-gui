//! Tests for `animation_scheduled_draw.rs`: cancellable scheduled draws.
//! A cancelled draw never asks for a frame, several pending draws each keep
//! their deadline with the earliest in charge, cancelling the earliest hands
//! over to the next (and tells a sleeping host to look again), and the
//! fire-and-forget `request_draw_after` channel is untouched by any of it.
//! Every test runs on the virtual clock and starts from a clean slate, since
//! the draw signals are per thread and test threads are reused.

use std::cell::Cell;
use std::time::Duration;

use crate::animation::{
    clear_draw_request, peek_next_draw_deadline, request_draw_after, reset_draw_signals,
    schedule_draw_after, wants_draw,
};
use crate::clock;

std::thread_local! {
    static NOTIFIES: Cell<u32> = const { Cell::new(0) };
}

fn count_notify() {
    NOTIFIES.with(|c| c.set(c.get() + 1));
}

/// Frame-demand notifications made while `f` ran.
fn notifies_during(f: impl FnOnce()) -> u32 {
    NOTIFIES.with(|c| c.set(0));
    crate::frame_demand::set_hook(Some(count_notify));
    f();
    crate::frame_demand::set_hook(None);
    NOTIFIES.with(|c| c.get())
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn a_cancelled_draw_never_asks_for_a_frame() {
    let _clock = clock::scoped_virtual(None);
    reset_draw_signals();
    let watchdog = schedule_draw_after(ms(100));
    assert!(watchdog.is_pending());
    assert_eq!(peek_next_draw_deadline(), Some(watchdog.deadline()));
    watchdog.cancel();
    assert_eq!(peek_next_draw_deadline(), None, "nothing left to wake for");
    clock::advance(ms(250));
    assert!(!wants_draw(), "the cancelled deadline never comes due");
}

#[test]
fn dropping_the_handle_cancels_the_draw() {
    let _clock = clock::scoped_virtual(None);
    reset_draw_signals();
    {
        let _watchdog = schedule_draw_after(ms(100));
        assert!(peek_next_draw_deadline().is_some());
    }
    assert_eq!(peek_next_draw_deadline(), None);
    clock::advance(ms(250));
    assert!(!wants_draw());
}

#[test]
fn cancelling_the_earliest_leaves_the_next_in_charge() {
    let _clock = clock::scoped_virtual(None);
    reset_draw_signals();
    let late = schedule_draw_after(ms(900));
    let early = schedule_draw_after(ms(100));
    let middle = schedule_draw_after(ms(400));
    assert_eq!(peek_next_draw_deadline(), Some(early.deadline()));
    early.cancel();
    assert_eq!(peek_next_draw_deadline(), Some(middle.deadline()));

    clock::advance(ms(150));
    assert!(!wants_draw(), "the cancelled 100 ms draw stays withdrawn");
    clock::advance(ms(250));
    assert!(
        wants_draw(),
        "the 400 ms draw comes due and asks for a frame"
    );
    assert!(!middle.is_pending(), "a served draw leaves the pending set");
    clear_draw_request();
    assert_eq!(peek_next_draw_deadline(), Some(late.deadline()));

    late.cancel();
    assert_eq!(peek_next_draw_deadline(), None);
    drop(middle);
    assert_eq!(
        peek_next_draw_deadline(),
        None,
        "cancelling a served draw is a no-op"
    );
}

#[test]
fn scheduling_and_cancelling_the_earliest_notify_the_host() {
    let _clock = clock::scoped_virtual(None);
    reset_draw_signals();
    let mut kept = None;
    assert_eq!(
        notifies_during(|| kept = Some(schedule_draw_after(ms(500)))),
        1,
        "scheduling wakes a sleeping host"
    );
    let first = kept.take().expect("scheduled");
    let later = schedule_draw_after(ms(800));
    assert_eq!(
        notifies_during(|| later.cancel()),
        0,
        "withdrawing a draw behind the earliest changes no wake time"
    );
    assert_eq!(
        notifies_during(|| first.cancel()),
        1,
        "withdrawing the earliest lets the host re-arm or sleep"
    );
    let served = schedule_draw_after(ms(10));
    clock::advance(ms(20));
    assert!(wants_draw());
    clear_draw_request();
    assert_eq!(
        notifies_during(|| served.cancel()),
        0,
        "a served draw has nothing to withdraw"
    );
}

#[test]
fn request_draw_after_is_untouched_by_cancellation() {
    let _clock = clock::scoped_virtual(None);
    reset_draw_signals();
    let watchdog = schedule_draw_after(ms(100));
    request_draw_after(ms(300));
    watchdog.cancel();
    let at = peek_next_draw_deadline().expect("the fire-and-forget deadline stays");
    assert_eq!(at, clock::now() + ms(300));
    clock::advance(ms(300));
    assert!(wants_draw(), "it still fires");
    clear_draw_request();
}

#[test]
fn a_paint_keeps_a_future_cancellable_deadline() {
    let _clock = clock::scoped_virtual(None);
    reset_draw_signals();
    let blink = schedule_draw_after(ms(300));
    clear_draw_request();
    assert!(blink.is_pending(), "a paint before the deadline keeps it");
    clock::advance(ms(300));
    clear_draw_request();
    assert!(!blink.is_pending(), "the paint at the deadline serves it");
}

#[test]
fn reset_withdraws_every_scheduled_draw() {
    let _clock = clock::scoped_virtual(None);
    reset_draw_signals();
    let a = schedule_draw_after(ms(100));
    let b = schedule_draw_after(ms(200));
    reset_draw_signals();
    assert!(!a.is_pending() && !b.is_pending());
    assert_eq!(peek_next_draw_deadline(), None);
}

/// Holds a pending draw until the thread exits.
struct DrawHolder(std::cell::RefCell<Option<crate::animation::ScheduledDraw>>);

impl Drop for DrawHolder {
    fn drop(&mut self) {
        // dropping the handle withdraws it, here after the thread's own
        // thread-locals may already be gone
        drop(self.0.borrow_mut().take());
    }
}

std::thread_local! {
    static FIRST_HOLDER: DrawHolder = const { DrawHolder(std::cell::RefCell::new(None)) };
    static LAST_HOLDER: DrawHolder = const { DrawHolder(std::cell::RefCell::new(None)) };
}

#[test]
fn a_pending_draw_dropped_while_its_thread_exits_does_not_panic() {
    // One holder is registered before the pending set and one after, so
    // whichever order the platform tears thread-locals down in, one of them
    // drops its pending handle after the pending set is gone. A panic in a
    // thread-local destructor aborts the whole test process.
    let thread = std::thread::spawn(|| {
        FIRST_HOLDER.with(|h| {
            *h.0.borrow_mut() = Some(schedule_draw_after(ms(1000)));
        });
        LAST_HOLDER.with(|h| {
            *h.0.borrow_mut() = Some(schedule_draw_after(ms(2000)));
        });
    });
    assert!(thread.join().is_ok(), "the thread exits cleanly");
}
