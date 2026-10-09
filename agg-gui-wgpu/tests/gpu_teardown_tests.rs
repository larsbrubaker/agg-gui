//! agg-sharp `Tests/Agg.Tests/Agg.RenderCore/GpuTeardownTests.cs`, ported 1:1:
//! the time budget a window close puts around the GPU drain
//! (`agg_gui_wgpu::drain_within_budget`, which `Gpu::release_within_budget`
//! and agg-gui-shell's close path run the device poll through).
//!
//! No GPU here, deliberately: the thing under test is the budget, and the slow
//! native wait it exists for is only slow on a software rasterizer. A blocking
//! closure stands in for it. C# exceptions are the drain's `Err` (or a panic)
//! here.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agg_gui_wgpu::{drain_within_budget, BudgetReport, GPU_TEARDOWN_BUDGET};

fn collector() -> (Arc<Mutex<Vec<String>>>, BudgetReport) {
    let reports = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&reports);
    let report: BudgetReport = Arc::new(move |message: &str| {
        sink.lock().unwrap().push(message.to_string());
    });
    (reports, report)
}

fn find(reports: &Arc<Mutex<Vec<String>>>, needle: &str) -> Option<String> {
    reports
        .lock()
        .unwrap()
        .iter()
        .find(|m| m.contains(needle))
        .cloned()
}

#[test]
fn a_quick_drain_runs_to_completion() {
    let drained = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&drained);
    let (reports, report) = collector();

    let finished = drain_within_budget(
        move || {
            flag.store(true, Ordering::SeqCst);
            Ok::<(), String>(())
        },
        "quick",
        Duration::from_secs(30),
        true,
        Some(report),
    );

    // True is the caller's permission to go on and release.
    assert_eq!(finished, Ok(true));
    assert!(drained.load(Ordering::SeqCst));
    // A drain that finished inside its budget is not news.
    assert!(reports.lock().unwrap().is_empty());
}

/// The reason this exists: a drain that outlasts its budget must hand the
/// calling thread back, not hold it for as long as the driver feels like.
#[test]
fn a_drain_that_outlasts_its_budget_is_abandoned() {
    let (release_drain, wait_drain) = mpsc::channel::<()>();
    let (drain_finished, wait_finished) = mpsc::channel::<()>();
    let (reports, report) = collector();

    let elapsed = Instant::now();
    let finished = drain_within_budget(
        move || {
            // Stands in for wgpu waiting on frames a software rasterizer has
            // not finished.
            let _ = wait_drain.recv();
            let _ = drain_finished.send(());
            Ok::<(), String>(())
        },
        "slow device",
        Duration::from_millis(200),
        true,
        Some(report),
    );
    let elapsed = elapsed.elapsed();

    // False is what stops the caller releasing a surface the drain is inside.
    assert_eq!(finished, Ok(false));
    // The caller waits the budget, not however long the GPU takes.
    assert!(elapsed < Duration::from_secs(10), "waited {elapsed:?}");
    // The message has to name which device was abandoned.
    assert!(find(&reports, "slow device").is_some());

    // The abandoned drain is still running; letting it finish keeps the test
    // from leaving a live thread behind.
    let _ = release_drain.send(());
    assert!(wait_finished.recv_timeout(Duration::from_secs(10)).is_ok());
}

/// A drain that fails is a bug in the drain, and it has to surface on the
/// thread that asked for it — the same place it would without a budget.
#[test]
fn an_exception_from_the_drain_reaches_the_caller() {
    let finished = drain_within_budget(
        || Err::<(), _>("drain blew up".to_string()),
        "throwing",
        Duration::from_secs(30),
        true,
        None,
    );
    assert_eq!(finished, Err("drain blew up".to_string()));
}

/// The scary one: the drain fails *after* the caller has given up on it, so
/// nobody is left to hand the failure to. It must not take the process down —
/// the test surviving to its asserts is half of what it proves — and it has to
/// be reported rather than silently dropped. A panic is the Rust failure that
/// can escape a thread, so the drain panics here.
///
/// CONVERTED: C# also asserts the report carries the exception's stack (it
/// contains the test class name). A Rust panic carries no stack, so that
/// assertion has no Rust counterpart; the report's own text — the device's
/// name and the failure — is what is asserted.
#[test]
fn a_drain_that_fails_after_being_abandoned_is_reported_and_survived() {
    let (fail_the_drain, wait_fail) = mpsc::channel::<()>();
    let (reports, report) = collector();

    let finished = drain_within_budget(
        move || -> Result<(), String> {
            let _ = wait_fail.recv();
            panic!("late drain failure");
        },
        "late failure",
        Duration::from_millis(200),
        true,
        Some(report),
    );
    assert_eq!(finished, Ok(false));

    // Only now does the abandoned drain fail, with nobody joining it.
    let _ = fail_the_drain.send(());

    let deadline = Instant::now();
    while find(&reports, "late drain failure").is_none()
        && deadline.elapsed() < Duration::from_secs(10)
    {
        std::thread::sleep(Duration::from_millis(10));
    }

    // A failure nobody is waiting for is a failure nobody would ever hear of.
    let failure_report = find(&reports, "late drain failure").expect("the failure is reported");
    // The report has to name the device it belongs to.
    assert!(failure_report.contains("late failure"), "{failure_report}");
    assert!(
        failure_report.contains("the abandoned drain of 'late failure' then failed"),
        "{failure_report}"
    );
}

/// The browser leg: wasm is single threaded, so there is no thread to detach a
/// slow drain onto; the call runs inline. Runnable on the desktop because the
/// platform is a parameter.
#[test]
fn with_no_background_thread_the_drain_runs_inline() {
    let caller = std::thread::current().id();
    let drain_thread = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&drain_thread);

    let finished = drain_within_budget(
        move || {
            *seen.lock().unwrap() = Some(std::thread::current().id());
            Ok::<(), String>(())
        },
        "browser",
        Duration::from_millis(1),
        false,
        None,
    );

    assert_eq!(finished, Ok(true));
    assert_eq!(*drain_thread.lock().unwrap(), Some(caller));
}

/// The close budget is agg-sharp's `GpuTeardown.DefaultBudget`.
#[test]
fn rust_only_teardown_budget_matches_mattercad() {
    assert_eq!(GPU_TEARDOWN_BUDGET, Duration::from_secs(5));
}

/// A drain that panics inside its budget resumes the panic on the caller, the
/// same place it would surface without a budget.
#[test]
fn rust_only_a_panic_inside_the_budget_resumes_on_the_caller() {
    let resumed = std::panic::catch_unwind(|| {
        drain_within_budget(
            || -> Result<(), String> { panic!("drain panicked") },
            "panicking",
            Duration::from_secs(30),
            true,
            None,
        )
    });
    let payload = resumed.expect_err("the panic reaches the caller");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"drain panicked"));
}

/// A drain that returns an error after it was abandoned is reported, naming
/// the device, like one that panics.
#[test]
fn rust_only_a_late_error_from_an_abandoned_drain_is_reported() {
    let (fail_the_drain, wait_fail) = mpsc::channel::<()>();
    let (reports, report) = collector();

    let finished = drain_within_budget(
        move || {
            let _ = wait_fail.recv();
            Err::<(), _>("late drain error".to_string())
        },
        "late error",
        Duration::from_millis(200),
        true,
        Some(report),
    );
    assert_eq!(finished, Ok(false));
    let _ = fail_the_drain.send(());

    let deadline = Instant::now();
    while find(&reports, "late drain error").is_none()
        && deadline.elapsed() < Duration::from_secs(10)
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    let failure_report = find(&reports, "late drain error").expect("the failure is reported");
    assert!(
        failure_report.contains("the abandoned drain of 'late error' then failed"),
        "{failure_report}"
    );
}
