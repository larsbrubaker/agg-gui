//! Tests for the host waker in `animation.rs`, pulled in via `#[path]` as
//! its `host_waker_tests` child module.
//!
//! Coverage for the reactive-host waker hook: a worker-thread
//! [`signal_async_state_change`] must be able to nudge a host parked in
//! `ControlFlow::Wait`.
//!
//! `HOST_WAKER` and the main queue's wakeup count (which these unbound test
//! threads signal and check) are process-global, and other tests in this crate call
//! `signal_async_state_change` concurrently — so
//! these tests serialize against each other with a local mutex, count
//! *their own* invocations rather than reading the global counter after
//! the fact, and assert with `>=` where a foreign signal could add more.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// Serializes only these tests; unrelated tests may still signal in
/// parallel, which is why every assertion below tolerates extra fires.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|p| p.into_inner())
}

#[test]
fn waker_fires_on_signal() {
    let _guard = serial();
    let fires = Arc::new(AtomicU64::new(0));
    let seen = Arc::clone(&fires);
    set_host_waker(move || {
        seen.fetch_add(1, Ordering::AcqRel);
    });

    signal_async_state_change();
    clear_host_waker();

    assert!(
        fires.load(Ordering::Acquire) >= 1,
        "installed waker must run when any thread signals an async change"
    );
}

#[test]
fn waker_sees_counter_already_bumped() {
    let _guard = serial();
    let before = crate::ui_thread::main_queue_wakeups();
    let observed = Arc::new(AtomicU64::new(0));
    let sink = Arc::clone(&observed);
    set_host_waker(move || {
        // Snapshot from inside the waker: the host is woken only after the
        // bump is published, otherwise it would park again having seen
        // nothing.
        sink.store(crate::ui_thread::main_queue_wakeups(), Ordering::Release);
    });

    signal_async_state_change();
    clear_host_waker();

    assert!(
        observed.load(Ordering::Acquire) > before,
        "the counter must be bumped BEFORE the waker runs"
    );
}

#[test]
fn replacing_the_waker_retires_the_old_one() {
    let _guard = serial();
    let old_fires = Arc::new(AtomicU64::new(0));
    let old_sink = Arc::clone(&old_fires);
    set_host_waker(move || {
        old_sink.fetch_add(1, Ordering::AcqRel);
    });

    let new_fires = Arc::new(AtomicU64::new(0));
    let new_sink = Arc::clone(&new_fires);
    set_host_waker(move || {
        new_sink.fetch_add(1, Ordering::AcqRel);
    });
    let old_baseline = old_fires.load(Ordering::Acquire);

    signal_async_state_change();
    clear_host_waker();

    assert!(
        new_fires.load(Ordering::Acquire) >= 1,
        "the replacement waker runs"
    );
    assert_eq!(
        old_fires.load(Ordering::Acquire),
        old_baseline,
        "the replaced waker no longer runs"
    );
}

#[test]
fn clear_stops_the_waker() {
    let _guard = serial();
    let fires = Arc::new(AtomicU64::new(0));
    let sink = Arc::clone(&fires);
    set_host_waker(move || {
        sink.fetch_add(1, Ordering::AcqRel);
    });
    clear_host_waker();
    let baseline = fires.load(Ordering::Acquire);

    signal_async_state_change();

    assert_eq!(
        fires.load(Ordering::Acquire),
        baseline,
        "a cleared waker must not run again"
    );
}

#[test]
fn signal_from_a_worker_thread_wakes_the_host() {
    let _guard = serial();
    let fires = Arc::new(AtomicU64::new(0));
    let sink = Arc::clone(&fires);
    set_host_waker(move || {
        sink.fetch_add(1, Ordering::AcqRel);
    });

    // The whole point of the hook: the signal originates off the main
    // thread, where thread-local bumps are invisible to the host.
    std::thread::spawn(signal_async_state_change)
        .join()
        .expect("worker thread signalled without panicking");
    clear_host_waker();

    assert!(
        fires.load(Ordering::Acquire) >= 1,
        "a worker-thread signal must reach the host waker"
    );
}

#[test]
fn signalling_without_a_waker_is_a_no_op() {
    let _guard = serial();
    clear_host_waker();
    // Must not panic and must still publish the cross-thread bump.
    let before = crate::ui_thread::main_queue_wakeups();
    signal_async_state_change();
    assert!(
        crate::ui_thread::main_queue_wakeups() > before,
        "counter-only behaviour is unchanged when no waker is installed"
    );
}
