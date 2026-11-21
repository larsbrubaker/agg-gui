//! `animation::peek_next_draw_owner`: which call armed the scheduled-draw
//! deadline a sleeping host is waiting on, through the public request
//! functions and the virtual UI clock:
//!
//! 1. The call that moves the deadline earlier owns it; a later or equal
//!    request leaves the owner alone.
//! 2. The owner goes when its deadline does: served by `wants_draw`, cleared
//!    by the paint that starts once it is due, or reset.
//! 3. A `#[track_caller]` helper (the tagged variant, or an app's own)
//!    reports the helper's caller, not the helper.
//! 4. Cancellable draws (`schedule_draw_after`) carry owners too, and the
//!    owner reported is that of the earliest deadline across both channels.
//!
//! Each test runs on its own thread, so the per-thread draw signals and
//! clock start fresh.

use std::time::Duration;

use agg_gui::animation::{
    clear_draw_request, peek_next_draw_deadline, peek_next_draw_owner, request_draw_after,
    request_draw_after_tagged, reset_draw_signals, schedule_draw_after, wants_draw,
};
use agg_gui::clock;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// The owner's file and line (columns differ between the call and `line!`).
fn owner_line() -> Option<(&'static str, u32)> {
    peek_next_draw_owner().map(|(_, owner)| (owner.file(), owner.line()))
}

fn here(line: u32) -> Option<(&'static str, u32)> {
    Some((file!(), line))
}

/// An app's own helper that forwards to `request_draw_after`, the way a
/// widget's blink timer or a task runner's delay would.
#[track_caller]
fn blink_helper(delay: Duration) {
    request_draw_after(delay);
}

#[test]
fn the_call_that_moves_the_deadline_earlier_owns_it() {
    let _clock = clock::scoped_virtual(None);
    let start = clock::now();
    request_draw_after(ms(500));
    let (earlier, ()) = (line!(), request_draw_after(ms(100)));

    assert_eq!(owner_line(), here(earlier));
    assert_eq!(
        peek_next_draw_owner().map(|(at, _)| at),
        Some(start + ms(100))
    );
    assert_eq!(
        peek_next_draw_owner().map(|(at, _)| at),
        peek_next_draw_deadline(),
        "the owner's instant is the deadline the host wakes for"
    );
}

#[test]
fn a_later_or_equal_request_keeps_the_owner() {
    let _clock = clock::scoped_virtual(None);
    let (first, ()) = (line!(), request_draw_after(ms(100)));
    request_draw_after(ms(500));
    request_draw_after(ms(100));

    assert_eq!(owner_line(), here(first));
}

#[test]
fn the_owner_goes_when_wants_draw_serves_the_deadline() {
    let _clock = clock::scoped_virtual(None);
    request_draw_after(ms(100));
    clock::advance(ms(50));
    assert!(!wants_draw(), "not due yet");
    assert!(
        peek_next_draw_owner().is_some(),
        "a future deadline keeps its owner"
    );

    clock::advance(ms(50));
    assert!(wants_draw(), "the deadline is due");
    assert_eq!(peek_next_draw_owner(), None);
}

#[test]
fn the_owner_goes_when_a_paint_clears_a_due_deadline() {
    let _clock = clock::scoped_virtual(None);
    request_draw_after(ms(100));
    clear_draw_request();
    assert!(
        peek_next_draw_owner().is_some(),
        "a paint before the deadline keeps it"
    );

    clock::advance(ms(100));
    clear_draw_request();
    assert_eq!(peek_next_draw_owner(), None);

    request_draw_after(ms(100));
    reset_draw_signals();
    assert_eq!(
        peek_next_draw_owner(),
        None,
        "a reset clears a future deadline's owner"
    );

    // A new request after the old owner went is owned by the new caller.
    let (again, ()) = (line!(), request_draw_after(ms(900)));
    assert_eq!(owner_line(), here(again));
}

#[test]
fn a_track_caller_helper_reports_its_callers_line() {
    let _clock = clock::scoped_virtual(None);
    let (helper_call, ()) = (line!(), blink_helper(ms(300)));
    assert_eq!(owner_line(), here(helper_call));

    let (tagged_call, ()) = (line!(), request_draw_after_tagged(ms(200), "test.tagged"));
    assert_eq!(owner_line(), here(tagged_call));
}

#[test]
fn the_earliest_deadline_across_both_channels_names_the_owner() {
    let _clock = clock::scoped_virtual(None);
    let (fire_and_forget, ()) = (line!(), request_draw_after(ms(300)));
    let (watchdog_line, watchdog) = (line!(), schedule_draw_after(ms(100)));
    let (blink_line, blink) = (line!(), schedule_draw_after(ms(200)));
    assert_eq!(owner_line(), here(watchdog_line));

    watchdog.cancel();
    assert_eq!(owner_line(), here(blink_line), "the next handle takes over");

    drop(blink);
    assert_eq!(owner_line(), here(fire_and_forget));

    // Equal instants: the fire-and-forget owner is reported.
    let _tie = schedule_draw_after(ms(300));
    assert_eq!(owner_line(), here(fire_and_forget));
}
