//! `ui_thread::next_due_owner`: which call queued the timed work a sleeping
//! loop's `next_due` timer will wake for, through the public queue functions
//! and the virtual UI clock:
//!
//! 1. The earliest delayed action's queuing line is reported, with the same
//!    instant `next_due` returns, and reading it changes nothing.
//! 2. An interval's setup line is its owner, and stays so after it runs.
//! 3. Work queued from another thread names that thread's call site.
//! 4. A `#[track_caller]` helper reports its own caller, not itself.
//! 5. An immediate action has no owner.
//!
//! Every test binds its thread to a queue of its own, so they run in
//! parallel without seeing each other's work.

use std::sync::Arc;
use std::time::Duration;

use agg_gui::clock;
use agg_gui::ui_thread::{self, RunningInterval, UiQueue};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Binds the calling test thread to a fresh queue of its own.
fn own_queue() -> UiQueue {
    let queue = UiQueue::new();
    queue.attach_current_thread();
    queue
}

/// The owner's file and line (columns differ between the call and `line!`).
fn owner_line() -> Option<(&'static str, u32)> {
    ui_thread::next_due_owner().map(|(_, owner)| (owner.file(), owner.line()))
}

fn here(line: u32) -> Option<(&'static str, u32)> {
    Some((file!(), line))
}

/// An app's own helper that forwards to the free function, the way a
/// debounce or a fade timer would.
#[track_caller]
fn wake_after(delay: Duration) {
    ui_thread::run_on_idle_after(delay, || {});
}

/// An app's own helper that forwards to the queue method.
#[track_caller]
fn tick_every(queue: &UiQueue, interval: Duration) -> Arc<RunningInterval> {
    queue.set_interval(|| {}, interval)
}

#[test]
fn the_earliest_delayed_action_names_the_line_that_queued_it() {
    let _clock = clock::scoped_virtual(None);
    let queue = own_queue();
    let start = clock::now();
    ui_thread::run_on_idle_after(ms(500), || {});
    let (earlier, ()) = (line!(), ui_thread::run_on_idle_after(ms(100), || {}));
    ui_thread::run_on_idle_after(ms(100), || {});

    assert_eq!(owner_line(), here(earlier), "the first of the earliest");
    let owned_at = ui_thread::next_due_owner().map(|(at, _)| at);
    assert_eq!(owned_at, Some(start + ms(100)));
    assert_eq!(owned_at, ui_thread::next_due(), "the time the loop arms");
    assert_eq!(owner_line(), here(earlier), "reading changes nothing");
    assert_eq!(ui_thread::count(), 3, "nothing ran or was dropped");

    // Once the two earliest have run, the remaining one owns the wake.
    clock::advance(ms(100));
    ui_thread::invoke_pending_actions();
    let later = earlier - 1;
    assert_eq!(owner_line(), here(later));
    assert_eq!(
        ui_thread::next_due_owner().map(|(at, _)| at),
        Some(start + ms(500))
    );
    queue.clear();
    assert_eq!(ui_thread::next_due_owner(), None, "nothing queued");
}

#[test]
fn an_interval_names_its_setup_line_and_keeps_it_after_a_run() {
    let _clock = clock::scoped_virtual(None);
    let queue = own_queue();
    let start = clock::now();
    let (setup, interval) = (line!(), ui_thread::set_interval(|| {}, ms(1000)));
    assert_eq!(owner_line(), here(setup));
    assert_eq!(
        ui_thread::next_due_owner().map(|(at, _)| at),
        Some(start + ms(1000))
    );

    clock::advance(ms(1000));
    ui_thread::invoke_pending_actions();
    assert_eq!(owner_line(), here(setup), "the same owner after a run");
    assert_eq!(
        ui_thread::next_due_owner().map(|(at, _)| at),
        Some(start + ms(2000)),
        "at its next run"
    );

    ui_thread::clear_interval(&interval);
    assert_eq!(ui_thread::next_due_owner(), None, "a cleared interval");
    queue.clear();
}

#[test]
fn work_queued_from_another_thread_names_that_threads_call_site() {
    let _clock = clock::scoped_virtual(None);
    let queue = own_queue();
    let remote = queue.clone();
    let posted_at = std::thread::spawn(move || {
        let (line, ()) = (line!(), remote.run_on_idle_after(ms(5000), || {}));
        line
    })
    .join()
    .expect("poster thread");

    // Not stamped yet: due at once, for the next drain to stamp it.
    assert_eq!(owner_line(), here(posted_at));
    assert_eq!(
        ui_thread::next_due_owner().map(|(at, _)| at),
        Some(clock::now())
    );
    ui_thread::invoke_pending_actions();
    assert_eq!(
        owner_line(),
        here(posted_at),
        "still its owner once stamped"
    );
    assert_eq!(
        ui_thread::next_due_owner().map(|(at, _)| at),
        Some(clock::now() + ms(5000))
    );
    queue.clear();
}

#[test]
fn a_track_caller_helper_reports_its_caller() {
    let _clock = clock::scoped_virtual(None);
    let queue = own_queue();
    let (delayed, ()) = (line!(), wake_after(ms(100)));
    assert_eq!(owner_line(), here(delayed), "through the free function");
    queue.clear();

    let (ticking, interval) = (line!(), tick_every(&queue, ms(100)));
    assert_eq!(owner_line(), here(ticking), "through the queue method");
    ui_thread::clear_interval(&interval);
    queue.clear();
}

#[test]
fn an_immediate_action_has_no_owner() {
    let _clock = clock::scoped_virtual(None);
    let queue = own_queue();
    ui_thread::run_on_idle_after(ms(100), || {});
    ui_thread::run_on_idle(|| {});
    assert_eq!(ui_thread::next_due(), Some(clock::now()), "due at once");
    assert_eq!(ui_thread::next_due_owner(), None, "an immediate action");
    ui_thread::invoke_pending_actions();
    assert!(
        ui_thread::next_due_owner().is_some(),
        "the delayed one owns the wake once the immediate one has run"
    );
    queue.clear();
}
