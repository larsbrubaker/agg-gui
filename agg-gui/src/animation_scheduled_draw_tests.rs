//! Tests for the scheduled-draw channel and the draw-request clears in
//! `animation.rs`, pulled in via `#[path]` as its `scheduled_draw_tests`
//! child module so the parent stays under the 800-line limit.
//!
//! Regression coverage for the reactive-host lost-wakeup fix: the
//! scheduled-draw cell must be readable non-destructively, and a due
//! deadline must surface through `wants_draw`. Uses short real sleeps
//! (`web_time::Instant` has no injectable clock here); each test clears
//! shared thread-local state up front so it can't inherit a pending
//! deadline from a prior test on the same worker thread.

use super::*;
use std::thread::sleep;

/// (a) The lost-wakeup repro. A pending deadline read once must still be
/// visible on the SECOND read — the "intervening AboutToWait" that the
/// read-and-clear design silently dropped.
#[test]
fn peek_is_non_destructive() {
    clear_draw_request();
    request_draw_after(Duration::from_millis(50));
    let first = peek_next_draw_deadline();
    assert!(first.is_some(), "first peek sees the pending deadline");
    let second = peek_next_draw_deadline();
    assert_eq!(
        first, second,
        "second peek still sees the SAME pending deadline (lost-wakeup fix)"
    );
}

/// (b) Once due, `wants_draw` returns true; after the paint-clear cycle
/// consumes it, a subsequent `wants_draw` is false absent a re-arm.
#[test]
fn due_deadline_surfaces_then_clears() {
    clear_draw_request();
    assert!(!wants_draw(), "baseline: nothing pending after clear");
    request_draw_after(Duration::from_millis(20));
    sleep(Duration::from_millis(40));
    assert!(wants_draw(), "a due deadline makes wants_draw() true");
    // Simulate the frame that honours it: paint clears the draw flags.
    clear_draw_request();
    assert!(
        !wants_draw(),
        "without a re-arm the loop goes idle again after the draw"
    );
}

/// (c) A future (not-yet-due) deadline is peekable but does NOT make
/// `wants_draw` true — the host idles on `WaitUntil` instead of polling.
#[test]
fn future_deadline_peeks_but_does_not_want_draw() {
    clear_draw_request();
    request_draw_after(Duration::from_millis(500));
    assert!(
        peek_next_draw_deadline().is_some(),
        "future deadline is visible to the host's WaitUntil"
    );
    assert!(
        !wants_draw(),
        "a future deadline must not force continuous polling"
    );
    // It also stays pending after that wants_draw() read.
    assert!(
        peek_next_draw_deadline().is_some(),
        "a non-due wants_draw() must not consume the deadline"
    );
}

/// (d) Earliest-deadline-wins still holds regardless of arm order.
#[test]
fn earliest_deadline_wins() {
    clear_draw_request();
    request_draw_after(Duration::from_millis(400));
    let after_long = peek_next_draw_deadline().expect("long deadline armed");
    request_draw_after(Duration::from_millis(20));
    let after_short = peek_next_draw_deadline().expect("short deadline armed");
    assert!(
        after_short < after_long,
        "a nearer deadline replaces a farther one"
    );
    // Reverse order: a farther deadline does not push the nearer one out.
    request_draw_after(Duration::from_millis(400));
    assert_eq!(
        peek_next_draw_deadline(),
        Some(after_short),
        "arming a farther deadline keeps the earliest"
    );
}

/// (e) A host whose surface refused the frame drops only the immediate
/// request: left set, it would keep a reactive host polling.
#[test]
fn clear_immediate_drops_the_immediate_request() {
    clear_draw_request();
    request_draw();
    clear_immediate_draw_request();
    assert!(
        !peek_draw_signals().0,
        "the immediate draw flag must be cleared"
    );
}

/// (f) The scheduled deadline survives: it may be the wake that retries the
/// refused frame (a backed-off surface configure) or an animation's next tick.
#[test]
fn clear_immediate_keeps_the_scheduled_deadline() {
    clear_draw_request();
    request_draw();
    request_draw_after(Duration::from_secs(60));
    let deadline = peek_next_draw_deadline();
    assert!(deadline.is_some(), "deadline armed");
    clear_immediate_draw_request();
    assert_eq!(
        peek_next_draw_deadline(),
        deadline,
        "the scheduled deadline must be untouched"
    );
}

/// (g) A cross-thread async signal that landed before the clear is still
/// pending after it — the next paint must still see it (dirty walk, layout).
/// Signalled from a spawned thread: a same-thread signal also sets this
/// thread's flags directly, which would hide a clear that swallows the
/// cross-thread counter.
#[test]
fn clear_immediate_keeps_a_pending_cross_thread_signal() {
    clear_draw_request();
    let before = ASYNC_STATE_EPOCH.with(|c| c.get());
    std::thread::spawn(signal_async_state_change)
        .join()
        .expect("signal thread");
    clear_immediate_draw_request();
    assert!(
        wants_draw(),
        "the cross-thread signal must still surface as a draw request"
    );
    assert_ne!(
        async_state_epoch(),
        before,
        "the cross-thread signal must still advance the async-state epoch"
    );
}
