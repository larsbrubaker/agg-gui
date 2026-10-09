//! Rust-only tests for `gpu_budget.rs` that need its crate-private seams: a
//! thread the OS refuses (`Spawner`), and the close-time release decision
//! (`release_after_drain`) driven by stand-in drains instead of a GPU. The
//! ports of agg-sharp's GpuStartupTests and GpuTeardownTests, which use only
//! the public API, are in `tests/gpu_startup_tests.rs` and
//! `tests/gpu_teardown_tests.rs`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use super::{create_with_spawner, drain_with_spawner, release_after_drain, BudgetReport};

/// An OS that will not start another thread.
fn refuse_thread(_name: String, _job: Box<dyn FnOnce() + Send>) -> std::io::Result<()> {
    Err(std::io::Error::other("no more threads"))
}

fn collector() -> (Arc<Mutex<Vec<String>>>, BudgetReport) {
    let reports = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&reports);
    let report: BudgetReport = Arc::new(move |message: &str| {
        sink.lock().unwrap().push(message.to_string());
    });
    (reports, report)
}

/// Stands in for a device bundle: counts how many times it was released.
struct Bundle(Arc<AtomicUsize>);

impl Drop for Bundle {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn rust_only_a_refused_thread_builds_the_device_inline() {
    let caller = std::thread::current().id();
    let (reports, report) = collector();
    let built = create_with_spawner(
        move || Ok::<_, String>(std::thread::current().id()),
        "no threads",
        Duration::from_millis(1),
        true,
        Some(report),
        refuse_thread,
    );
    // Built, on the caller, and not reported as a timeout.
    assert_eq!(built, Ok(Some(caller)));
    assert!(reports.lock().unwrap().is_empty());
}

#[test]
fn rust_only_a_refused_thread_drains_inline() {
    let caller = std::thread::current().id();
    let drain_thread = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&drain_thread);
    let (reports, report) = collector();
    let finished = drain_with_spawner(
        move || {
            *seen.lock().unwrap() = Some(std::thread::current().id());
            Ok::<(), String>(())
        },
        "no threads",
        Duration::from_millis(1),
        true,
        Some(report),
        refuse_thread,
    );
    // True: the drain ran to completion, so the caller may release.
    assert_eq!(finished, Ok(true));
    assert_eq!(*drain_thread.lock().unwrap(), Some(caller));
    assert!(reports.lock().unwrap().is_empty());
}

/// wgpu 29 panics when a lost device is polled; a close must still release
/// and finish rather than resume that panic.
#[test]
fn rust_only_the_release_survives_a_drain_that_panics() {
    let released = Arc::new(AtomicUsize::new(0));
    let freed = release_after_drain(
        Bundle(Arc::clone(&released)),
        false,
        || -> Result<(), String> { panic!("Device is lost") },
        "panicking drain",
        Duration::from_secs(30),
        true,
        None,
    );
    assert!(freed);
    assert_eq!(released.load(Ordering::SeqCst), 1);
}

#[test]
fn rust_only_a_device_already_lost_is_released_without_a_drain() {
    let released = Arc::new(AtomicUsize::new(0));
    let drained = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&drained);
    let freed = release_after_drain(
        Bundle(Arc::clone(&released)),
        true,
        move || -> Result<(), String> {
            flag.store(true, Ordering::SeqCst);
            Ok(())
        },
        "lost device",
        Duration::from_secs(30),
        true,
        None,
    );
    assert!(freed);
    assert!(
        !drained.load(Ordering::SeqCst),
        "a lost device is not polled"
    );
    assert_eq!(released.load(Ordering::SeqCst), 1);
}

#[test]
fn rust_only_a_failed_drain_is_followed_by_the_release() {
    let released = Arc::new(AtomicUsize::new(0));
    let freed = release_after_drain(
        Bundle(Arc::clone(&released)),
        false,
        || Err::<(), _>("Timeout"),
        "failed drain",
        Duration::from_secs(30),
        true,
        None,
    );
    assert!(freed);
    assert_eq!(released.load(Ordering::SeqCst), 1);
}

#[test]
fn rust_only_a_drained_device_is_released_and_an_abandoned_one_leaked() {
    let released = Arc::new(AtomicUsize::new(0));
    let freed = release_after_drain(
        Bundle(Arc::clone(&released)),
        false,
        || Ok::<(), String>(()),
        "quick",
        Duration::from_secs(30),
        true,
        None,
    );
    assert!(freed);
    assert_eq!(released.load(Ordering::SeqCst), 1);

    let leaked = Arc::new(AtomicUsize::new(0));
    let (release_drain, wait_drain) = mpsc::channel::<()>();
    let (reports, report) = collector();
    let freed = release_after_drain(
        Bundle(Arc::clone(&leaked)),
        false,
        move || -> Result<(), String> {
            let _ = wait_drain.recv();
            Ok(())
        },
        "slow device",
        Duration::from_millis(200),
        true,
        Some(report),
    );
    assert!(!freed);
    assert!(reports
        .lock()
        .unwrap()
        .iter()
        .any(|m| m.contains("slow device")));
    let _ = release_drain.send(());
    // Never released, even once the abandoned drain finishes.
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(leaked.load(Ordering::SeqCst), 0);
}
