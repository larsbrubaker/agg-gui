//! agg-sharp `Tests/Agg.Tests/Agg.UI/UiThreadConcurrencyTests.cs`, ported
//! through MatterCAD's `mattercad-app/tests/ui_thread.rs`, plus Rust-only
//! checks of delayed actions and intervals, per-thread queues, the UI clock
//! and panic containment. Every test binds its own thread to a queue of its
//! own (or an explicit shared [`UiQueue`]), so they run in parallel without a
//! lock; the process-wide main queue is covered in `ui_thread_main_queue.rs`.
//! The other agg-sharp UiThread test classes cover the async/await pump
//! (`RunOnIdle(Func<Task>)`, `YieldToFrame`, pump identity under a
//! synchronization context), which is not ported.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agg_gui::clock;
use agg_gui::ui_thread::{self, UiQueue};
use agg_gui::unhandled::{set_unhandled_handler, UnhandledOrigin, UnhandledReport};

/// Pumping from several threads at once must never fail, lose a queued action
/// or run one twice. C# pumps its one process-wide queue; here the pumps and
/// the producer attach to one shared queue, which is what several threads
/// pumping "the" queue means with a queue per UI thread.
#[test]
fn concurrent_pumps_do_not_throw_or_lose_actions() {
    const ACTION_COUNT: usize = 20000;
    let queue = UiQueue::new();
    queue.attach_current_thread();
    let ran = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let pumps: Vec<_> = (0..3)
        .map(|i| {
            let stop = Arc::clone(&stop);
            let queue = queue.clone();
            std::thread::Builder::new()
                .name(format!("UiThread test pump {i}"))
                .spawn(move || {
                    queue.attach_current_thread();
                    while !stop.load(Ordering::Acquire) {
                        ui_thread::invoke_pending_actions();
                        std::thread::yield_now();
                    }
                    // Drain whatever the producer queued just before the stop
                    // flag was set.
                    ui_thread::invoke_pending_actions();
                })
                .expect("spawn pump")
        })
        .collect();
    for _ in 0..ACTION_COUNT {
        let ran = Arc::clone(&ran);
        ui_thread::run_on_idle(move || {
            ran.fetch_add(1, Ordering::SeqCst);
        });
    }
    // Let the pumps get through the queue before asking them to stop. The
    // deadline is only so a dropped action fails the assert below instead of
    // hanging the run.
    let deadline = Instant::now();
    while ran.load(Ordering::SeqCst) < ACTION_COUNT && deadline.elapsed() < Duration::from_secs(10)
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    stop.store(true, Ordering::Release);
    let failures = pumps.into_iter().filter_map(|p| p.join().err()).count();
    ui_thread::reset_for_tests();
    assert_eq!(failures, 0, "pumping from several threads must not fail");
    assert_eq!(
        ran.load(Ordering::SeqCst),
        ACTION_COUNT,
        "every action runs exactly once"
    );
}

/// Binds the calling test thread to a queue of its own: a fresh queue
/// attached, so no other test's thread can ever drain it.
fn own_queue() -> UiQueue {
    let queue = UiQueue::new();
    queue.attach_current_thread();
    queue
}

#[test]
fn rust_only_delayed_actions_and_intervals_wait_for_their_time() {
    own_queue();
    ui_thread::reset_for_tests();
    let delayed = Arc::new(AtomicUsize::new(0));
    let d = Arc::clone(&delayed);
    ui_thread::run_on_idle_after(Duration::from_millis(30), move || {
        d.fetch_add(1, Ordering::SeqCst);
    });
    let ticks = Arc::new(AtomicUsize::new(0));
    let t = Arc::clone(&ticks);
    let interval = ui_thread::set_interval(
        move || {
            t.fetch_add(1, Ordering::SeqCst);
        },
        Duration::from_millis(10),
    );

    ui_thread::invoke_pending_actions();
    assert_eq!(delayed.load(Ordering::SeqCst), 0);
    assert_eq!(ui_thread::count(), 1);
    assert_eq!(ui_thread::count_expired(), 0);

    std::thread::sleep(Duration::from_millis(40));
    assert_eq!(ui_thread::count_expired(), 1);
    ui_thread::invoke_pending_actions();
    assert_eq!(delayed.load(Ordering::SeqCst), 1);
    assert_eq!(ui_thread::count(), 0);
    // An interval runs at most once per drain, however late the drain is.
    assert_eq!(ticks.load(Ordering::SeqCst), 1);

    ui_thread::clear_interval(&interval);
    assert!(!interval.active());
    std::thread::sleep(Duration::from_millis(15));
    ui_thread::invoke_pending_actions();
    assert_eq!(ticks.load(Ordering::SeqCst), 1);
    ui_thread::reset_for_tests();
}

#[test]
fn rust_only_delays_and_intervals_run_on_the_virtual_ui_clock() {
    own_queue();
    let _clock = clock::scoped_virtual(None);
    let delayed = Arc::new(AtomicUsize::new(0));
    let d = Arc::clone(&delayed);
    ui_thread::run_on_idle_after(Duration::from_millis(500), move || {
        d.fetch_add(1, Ordering::SeqCst);
    });
    let ticks = Arc::new(AtomicUsize::new(0));
    let t = Arc::clone(&ticks);
    let interval = ui_thread::set_interval(
        move || {
            t.fetch_add(1, Ordering::SeqCst);
        },
        Duration::from_millis(100),
    );
    let start_ms = ui_thread::current_timer_ms();

    // Wall time passing does nothing while the UI clock stands still.
    std::thread::sleep(Duration::from_millis(20));
    ui_thread::invoke_pending_actions();
    assert_eq!(ui_thread::current_timer_ms(), start_ms);
    assert_eq!(
        (delayed.load(Ordering::SeqCst), ticks.load(Ordering::SeqCst)),
        (0, 0)
    );
    // The drain armed a timed redraw for the interval, the earliest work.
    assert_eq!(
        agg_gui::animation::peek_next_draw_deadline(),
        Some(clock::now() + Duration::from_millis(100))
    );

    clock::advance(Duration::from_millis(100));
    ui_thread::invoke_pending_actions();
    // Whole milliseconds since the epoch, truncated (C# `ElapsedMilliseconds`),
    // so 100 virtual ms move the count by 99 or 100 depending on the fraction.
    let moved = ui_thread::current_timer_ms() - start_ms;
    assert!((99..=100).contains(&moved), "moved {moved} ms");
    assert_eq!(
        (delayed.load(Ordering::SeqCst), ticks.load(Ordering::SeqCst)),
        (0, 1)
    );

    clock::advance(Duration::from_millis(400));
    assert_eq!(ui_thread::count_expired(), 1);
    ui_thread::invoke_pending_actions();
    assert_eq!(
        (delayed.load(Ordering::SeqCst), ticks.load(Ordering::SeqCst)),
        (1, 2)
    );
    ui_thread::clear_interval(&interval);
    ui_thread::reset_for_tests();
}

#[test]
fn rust_only_delays_queued_from_another_thread_start_at_the_next_drain() {
    let queue = own_queue();
    let _clock = clock::scoped_virtual(None);
    ui_thread::invoke_pending_actions();
    let ran = Arc::new(AtomicUsize::new(0));
    let r = Arc::clone(&ran);
    let worker_queue = queue.clone();
    // A worker's clock is real; the UI thread's is virtual. The delay counts
    // on the UI clock from the drain that first sees it.
    std::thread::spawn(move || {
        worker_queue.run_on_idle_after(Duration::from_millis(50), move || {
            r.fetch_add(1, Ordering::SeqCst);
        });
    })
    .join()
    .expect("worker");
    assert_eq!(ui_thread::count(), 1);
    assert_eq!(ui_thread::count_expired(), 0, "not stamped until a drain");
    ui_thread::invoke_pending_actions();
    clock::advance(Duration::from_millis(49));
    ui_thread::invoke_pending_actions();
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    clock::advance(Duration::from_millis(1));
    ui_thread::invoke_pending_actions();
    assert_eq!(ran.load(Ordering::SeqCst), 1);
}

#[test]
fn rust_only_each_ui_thread_drains_only_its_own_queue() {
    let mine = own_queue();
    let ran_here = Arc::new(AtomicUsize::new(0));
    let r = Arc::clone(&ran_here);
    ui_thread::run_on_idle(move || {
        r.fetch_add(1, Ordering::SeqCst);
    });
    // Another UI thread, bound to its own queue, drains its own work only.
    let other = std::thread::spawn(|| {
        let theirs = UiQueue::new();
        theirs.attach_current_thread();
        let ran_there = Arc::new(AtomicUsize::new(0));
        let r = Arc::clone(&ran_there);
        ui_thread::run_on_idle(move || {
            r.fetch_add(1, Ordering::SeqCst);
        });
        ui_thread::invoke_pending_actions();
        assert!(ui_thread::is_ui_thread());
        ran_there.load(Ordering::SeqCst)
    })
    .join()
    .expect("other UI thread");
    assert_eq!(other, 1);
    assert_eq!(
        ran_here.load(Ordering::SeqCst),
        0,
        "not drained by the other thread"
    );

    // A worker serving this UI thread posts through its captured queue.
    let handle = ui_thread::current_queue();
    assert!(handle.same_queue(&mine));
    let r = Arc::clone(&ran_here);
    std::thread::spawn(move || {
        handle.run_on_idle(move || {
            r.fetch_add(10, Ordering::SeqCst);
        })
    })
    .join()
    .expect("worker");
    ui_thread::invoke_pending_actions();
    assert_eq!(ran_here.load(Ordering::SeqCst), 11);
}

#[test]
fn rust_only_run_on_ui_thread_runs_now_only_on_the_ui_thread() {
    own_queue();
    ui_thread::reset_for_tests();
    assert!(!ui_thread::is_ui_thread());
    let ran = Arc::new(AtomicUsize::new(0));
    let r = Arc::clone(&ran);
    ui_thread::run_on_ui_thread(move || {
        r.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(
        ran.load(Ordering::SeqCst),
        0,
        "queued: not the UI thread yet"
    );
    ui_thread::mark_current_thread_as_ui_thread();
    assert!(ui_thread::is_ui_thread());
    let r = Arc::clone(&ran);
    ui_thread::run_on_ui_thread(move || {
        r.fetch_add(10, Ordering::SeqCst);
    });
    assert_eq!(
        ran.load(Ordering::SeqCst),
        10,
        "ran at once on the UI thread"
    );
    ui_thread::invoke_pending_actions();
    assert_eq!(ran.load(Ordering::SeqCst), 11);
}

#[test]
fn rust_only_a_panicking_action_is_reported_and_the_rest_still_run() {
    own_queue();
    let reports: Rc<RefCell<Vec<UnhandledReport>>> = Rc::default();
    let sink = Rc::clone(&reports);
    let _handler = set_unhandled_handler(move |r| sink.borrow_mut().push(r.clone()));
    let ran = Arc::new(AtomicUsize::new(0));
    let r = Arc::clone(&ran);
    ui_thread::run_on_idle(|| panic!("idle action failed"));
    ui_thread::run_on_idle(move || {
        r.fetch_add(1, Ordering::SeqCst);
    });
    ui_thread::invoke_pending_actions();
    assert_eq!(
        ran.load(Ordering::SeqCst),
        1,
        "the action after the panic ran"
    );
    assert_eq!(
        *reports.borrow(),
        vec![UnhandledReport {
            origin: UnhandledOrigin::IdleAction,
            message: "idle action failed".to_string(),
        }]
    );
    // The queue keeps working.
    let r = Arc::clone(&ran);
    ui_thread::run_on_idle(move || {
        r.fetch_add(1, Ordering::SeqCst);
    });
    ui_thread::invoke_pending_actions();
    assert_eq!(ran.load(Ordering::SeqCst), 2);
}

#[test]
fn rust_only_with_no_handler_a_panic_escapes_after_the_rest_has_run() {
    own_queue();
    let ran = Arc::new(AtomicUsize::new(0));
    let r = Arc::clone(&ran);
    ui_thread::run_on_idle(|| panic!("first"));
    ui_thread::run_on_idle(|| panic!("second"));
    ui_thread::run_on_idle(move || {
        r.fetch_add(1, Ordering::SeqCst);
    });
    let escaped = std::panic::catch_unwind(ui_thread::invoke_pending_actions)
        .expect_err("an unreported panic must not vanish");
    assert_eq!(agg_gui::unhandled::panic_message(escaped.as_ref()), "first");
    assert_eq!(
        ran.load(Ordering::SeqCst),
        1,
        "every queued action still ran once"
    );
    ui_thread::invoke_pending_actions();
    assert_eq!(ran.load(Ordering::SeqCst), 1, "nothing ran twice");
}

thread_local! {
    static HOOK_RUNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn count_hook_run() {
    HOOK_RUNS.with(|c| c.set(c.get() + 1));
}

#[test]
fn rust_only_drain_hooks_run_once_per_drain_after_the_actions() {
    own_queue();
    // Process-wide, so it runs on the other tests' drains too; it only counts
    // on the thread that drains.
    ui_thread::add_drain_hook(count_hook_run);
    ui_thread::add_drain_hook(count_hook_run);
    let seen_by_action = Arc::new(AtomicUsize::new(usize::MAX));
    let seen = Arc::clone(&seen_by_action);
    ui_thread::run_on_idle(move || {
        seen.store(HOOK_RUNS.with(|c| c.get()), Ordering::SeqCst);
    });
    ui_thread::invoke_pending_actions();
    assert_eq!(
        seen_by_action.load(Ordering::SeqCst),
        0,
        "actions run before hooks"
    );
    assert_eq!(HOOK_RUNS.with(|c| c.get()), 1, "added twice, runs once");
    ui_thread::invoke_pending_actions();
    assert_eq!(HOOK_RUNS.with(|c| c.get()), 2, "and on every drain");
}
