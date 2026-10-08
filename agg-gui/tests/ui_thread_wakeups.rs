//! Rust-only checks that frame-loop wakeups are per UI thread: queuing work
//! (`ui_thread::run_on_idle` and friends) or `signal_async_state_change` wakes
//! the thread that drains the queue it lands on, and no other UI thread. That
//! is what lets parallel headless tests assert "idle" and "no relayout"
//! without seeing each other's posts. Each test binds its threads to queues of
//! their own; the main queue's worker wakeups are covered in
//! `ui_thread_main_queue.rs`.

use std::sync::mpsc;

use agg_gui::animation::{
    async_state_epoch, clear_draw_request, invalidation_epoch, signal_async_state_change,
    wants_draw,
};
use agg_gui::ui_thread::{self, UiQueue};

/// Binds the calling thread to a fresh queue and starts it idle.
fn idle_ui_thread() -> UiQueue {
    let queue = UiQueue::new();
    queue.attach_current_thread();
    ui_thread::mark_current_thread_as_ui_thread();
    clear_draw_request();
    queue
}

#[test]
fn rust_only_another_ui_threads_posts_and_signals_do_not_wake_this_one() {
    let _mine = idle_ui_thread();
    let epoch = invalidation_epoch();
    let async_epoch = async_state_epoch();
    assert!(!wants_draw(), "idle before the other thread runs");

    std::thread::spawn(|| {
        let _theirs = idle_ui_thread();
        ui_thread::run_on_idle(|| {});
        ui_thread::run_on_idle_after(std::time::Duration::from_millis(5), || {});
        signal_async_state_change();
        assert!(wants_draw(), "the posting UI thread wakes itself");
    })
    .join()
    .expect("other UI thread");

    assert!(!wants_draw(), "another UI thread's wakeups stay on it");
    assert_eq!(invalidation_epoch(), epoch, "no relayout from elsewhere");
    assert_eq!(async_state_epoch(), async_epoch);
}

#[test]
fn rust_only_a_worker_posting_to_a_queue_wakes_the_thread_that_drains_it() {
    let mine = idle_ui_thread();
    let async_epoch = async_state_epoch();
    std::thread::spawn(move || mine.run_on_idle(|| {}))
        .join()
        .expect("worker");
    assert!(wants_draw(), "a post to this thread's queue wakes it");
    assert_ne!(async_state_epoch(), async_epoch);
}

#[test]
fn rust_only_a_worker_signalling_for_a_ui_thread_wakes_only_that_thread() {
    let mine = idle_ui_thread();
    let (to_other, other_rx) = mpsc::channel::<()>();
    let (other_ready, ready_rx) = mpsc::channel::<UiQueue>();
    let (other_done, done_rx) = mpsc::channel::<bool>();
    let other = std::thread::spawn(move || {
        let theirs = idle_ui_thread();
        other_ready.send(theirs).expect("ready");
        other_rx.recv().expect("go");
        other_done.send(wants_draw()).expect("done");
    });
    let theirs = ready_rx.recv().expect("other queue");

    // A worker serving this thread (attached to its queue) signals an async
    // change: this thread wakes, the other does not.
    std::thread::spawn(move || {
        mine.attach_current_thread();
        signal_async_state_change();
    })
    .join()
    .expect("worker");
    assert!(wants_draw(), "the served UI thread wakes");
    to_other.send(()).expect("go");
    assert!(
        !done_rx.recv().expect("other result"),
        "a UI thread the worker does not serve stays idle"
    );
    other.join().expect("other UI thread");
    drop(theirs);
}
