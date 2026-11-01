//! `WakeTimer`: the deadline that bounds a wait wakes it once the timeout passes, and a timer
//! dropped first (the answer came) ends its thread without waking anything, so no thread sleeps
//! out a timeout nobody waits for. Native only: in the browser the timer starts no thread.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Wake, Waker};
use std::time::{Duration, Instant};

use agg_gui_files::WakeTimer;

/// Counts its wakes.
#[derive(Default)]
struct CountingWaker(AtomicUsize);

impl Wake for CountingWaker {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// Waits (bounded) until `done` holds.
fn wait_for(done: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while !done() {
        if start.elapsed() > Duration::from_secs(5) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    true
}

#[test]
fn the_timer_wakes_the_wait_once_the_timeout_passes() {
    static LIVE: AtomicUsize = AtomicUsize::new(0);
    let wakes = Arc::new(CountingWaker::default());
    let timer = WakeTimer::start(
        Waker::from(Arc::clone(&wakes)),
        Duration::from_millis(20),
        &LIVE,
    );
    assert!(
        wait_for(|| wakes.0.load(Ordering::SeqCst) == 1),
        "the deadline woke the wait"
    );
    assert!(
        wait_for(|| LIVE.load(Ordering::SeqCst) == 0),
        "the thread ends after waking"
    );
    // Dropping the timer once it has fired wakes nothing more.
    drop(timer);
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1, "it wakes once");
}

#[test]
fn a_timer_dropped_before_its_timeout_ends_its_thread_without_waking() {
    static LIVE: AtomicUsize = AtomicUsize::new(0);
    let wakes = Arc::new(CountingWaker::default());
    let timer = WakeTimer::start(
        Waker::from(Arc::clone(&wakes)),
        Duration::from_secs(60),
        &LIVE,
    );
    assert_eq!(
        LIVE.load(Ordering::SeqCst),
        1,
        "a timer thread runs while the wait does"
    );
    drop(timer);
    // Far inside the 60 s it would otherwise sleep.
    assert!(
        wait_for(|| LIVE.load(Ordering::SeqCst) == 0),
        "the thread ended with the timer"
    );
    assert_eq!(wakes.0.load(Ordering::SeqCst), 0, "nothing was woken");
}
