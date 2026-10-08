//! Rust-only check of `agg_gui::ui_thread::unbind_current_thread`: a UI
//! thread that is done with its UI hands the process's main queue on at once,
//! not when the thread itself ends. A headless test's UI lives for one test
//! function, but the test thread's thread-locals (and with them its claim on
//! the main queue) are torn down only after the test runner has been told the
//! test finished, so without the explicit hand-off the next test's UI thread
//! finds the main queue still owned, binds a queue of its own, and workers'
//! posts land on the main queue that nobody drains. One test in its own
//! binary, because the main queue is process state that parallel tests would
//! race for; the rest of the main-queue behaviour is in
//! `ui_thread_main_queue.rs`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;

use agg_gui::ui_thread;

fn post_from_worker(ran: &Arc<AtomicUsize>, add: usize) {
    let r = Arc::clone(ran);
    std::thread::spawn(move || {
        ui_thread::run_on_idle(move || {
            r.fetch_add(add, Ordering::SeqCst);
        })
    })
    .join()
    .expect("worker");
}

#[test]
fn rust_only_a_ui_thread_that_unbinds_hands_the_main_queue_on_while_it_still_runs() {
    let ran = Arc::new(AtomicUsize::new(0));
    let (unbound, first_unbound) = channel();
    let (finish, first_may_end) = channel::<()>();

    // The first UI thread owns the main queue, then is done with its UI but
    // keeps running (as a test thread does after its test body returns).
    let first = std::thread::spawn(move || {
        ui_thread::mark_current_thread_as_ui_thread();
        let main = ui_thread::current_queue();
        ui_thread::unbind_current_thread();
        assert!(!ui_thread::is_ui_thread(), "it is no longer a UI thread");
        assert!(
            ui_thread::current_queue().same_queue(&main),
            "unbound, it posts to the main queue as any worker does"
        );
        unbound.send(main).expect("the test waits");
        let _ = first_may_end.recv();
    });
    let main = first_unbound.recv().expect("the first UI thread unbinds");

    // A worker's post waits on the main queue...
    post_from_worker(&ran, 1);

    // ...and the next UI thread adopts the main queue, with that post on it,
    // although the first thread has not ended.
    let ran_next = Arc::clone(&ran);
    let next_had_main = std::thread::spawn(move || {
        ui_thread::invoke_pending_actions();
        assert_eq!(ran_next.load(Ordering::SeqCst), 1, "the post ran here");
        let had_main = ui_thread::current_queue().same_queue(&main);
        ui_thread::unbind_current_thread();
        had_main
    })
    .join()
    .expect("next UI thread");
    assert!(next_had_main, "the next UI thread adopted the main queue");

    // A thread that unbinds and binds again is a UI thread again, back on the
    // main queue it released when nobody else took it.
    let rebound_main = std::thread::spawn(|| {
        ui_thread::mark_current_thread_as_ui_thread();
        let main = ui_thread::current_queue();
        ui_thread::unbind_current_thread();
        ui_thread::mark_current_thread_as_ui_thread();
        ui_thread::is_ui_thread() && ui_thread::current_queue().same_queue(&main)
    })
    .join()
    .expect("rebinding UI thread");
    assert!(rebound_main);

    finish.send(()).expect("the first UI thread waits");
    first.join().expect("first UI thread");
}
