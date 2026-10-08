//! Rust-only checks of `agg_gui::ui_thread`'s process-wide main queue: which
//! queue an unbound worker posts to, and how a UI thread claims, keeps and
//! hands on the main queue. One test in its own binary, because the main
//! queue is process state that parallel tests would race for; the per-thread
//! behaviour is in `ui_thread.rs`.

use std::sync::atomic::{AtomicUsize, Ordering};
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
fn rust_only_workers_post_to_the_main_ui_thread_which_later_threads_adopt() {
    let ran = Arc::new(AtomicUsize::new(0));
    // Work posted before any UI thread exists waits on the main queue...
    post_from_worker(&ran, 1);

    // ...and the first UI thread adopts it, and with it the workers' posts.
    let first = {
        let ran = Arc::clone(&ran);
        std::thread::spawn(move || {
            ui_thread::mark_current_thread_as_ui_thread();
            let main = ui_thread::current_queue();
            ui_thread::invoke_pending_actions();
            assert_eq!(ran.load(Ordering::SeqCst), 1);
            post_from_worker(&ran, 10);
            ui_thread::invoke_pending_actions();
            assert_eq!(ran.load(Ordering::SeqCst), 11);

            // A second UI thread while this one lives gets a queue of its own.
            let second_was_main = std::thread::spawn(move || {
                ui_thread::mark_current_thread_as_ui_thread();
                ui_thread::current_queue().same_queue(&main)
            })
            .join()
            .expect("second UI thread");
            assert!(!second_was_main);

            // Posted to the main queue while this thread is ending: nobody
            // drains it now.
            post_from_worker(&ran, 100);
        })
    };
    first.join().expect("first UI thread");
    assert_eq!(ran.load(Ordering::SeqCst), 11);

    // The owner ended, so the next UI thread adopts the main queue with the
    // work still on it.
    std::thread::spawn(move || {
        ui_thread::invoke_pending_actions();
        assert_eq!(ran.load(Ordering::SeqCst), 111);
    })
    .join()
    .expect("next UI thread");
}
