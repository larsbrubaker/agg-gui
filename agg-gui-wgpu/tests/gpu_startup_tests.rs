//! agg-sharp `Tests/Agg.Tests/Agg.RenderCore/GpuStartupTests.cs`, ported 1:1:
//! the time budget a window puts around building its render device
//! (`agg_gui_wgpu::create_within_budget`, which `Gpu::new` runs its adapter and
//! device requests through).
//!
//! No GPU here: the stall this bounds is a synchronous native call that only
//! misbehaves on a loaded software rasterizer. A blocking factory stands in for
//! it, which is why the device is built through a closure. C# exceptions are
//! the factory's `Err` here.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agg_gui_wgpu::{create_within_budget, BudgetReport, GPU_STARTUP_BUDGET};

/// A stand-in for a render device; identity is all these tests need.
#[derive(Debug, PartialEq, Eq)]
struct FakeDevice(u32);

fn collector() -> (Arc<Mutex<Vec<String>>>, BudgetReport) {
    let reports = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&reports);
    let report: BudgetReport = Arc::new(move |message: &str| {
        sink.lock().unwrap().push(message.to_string());
    });
    (reports, report)
}

fn any_contains(reports: &Arc<Mutex<Vec<String>>>, needle: &str) -> bool {
    reports.lock().unwrap().iter().any(|m| m.contains(needle))
}

#[test]
fn a_device_that_arrives_in_time_is_returned() {
    let (reports, report) = collector();
    let built = create_within_budget(
        || Ok::<_, String>(FakeDevice(7)),
        "quick",
        Duration::from_secs(30),
        true,
        Some(report),
    );
    assert_eq!(built, Ok(Some(FakeDevice(7))));
    assert!(reports.lock().unwrap().is_empty());
}

/// The failure this exists for: the acquisition does not come back, and the
/// caller has to be handed its thread again so the window can say it has no
/// device — rather than waiting forever with nothing to report.
#[test]
fn an_acquisition_that_outlasts_its_budget_gives_up_and_says_so() {
    let (release_build, wait_build) = mpsc::channel::<()>();
    let (reports, report) = collector();

    let elapsed = Instant::now();
    let built = create_within_budget(
        move || {
            let _ = wait_build.recv();
            Ok::<_, String>(FakeDevice(1))
        },
        "stuck adapter",
        Duration::from_millis(200),
        true,
        Some(report),
    );
    let elapsed = elapsed.elapsed();

    // `None` is what tells the host there is no device rather than wait on.
    assert_eq!(built, Ok(None));
    // The caller waits the budget, not however long the driver takes.
    assert!(elapsed < Duration::from_secs(10), "waited {elapsed:?}");
    assert!(any_contains(&reports, "stuck adapter"));

    let _ = release_build.send(());
}

/// A device that turns up after the window gave up is leaked rather than
/// released — releasing it would tear down a swapchain next to a window that
/// may already be gone. It has to say so, and must not take the process down.
#[test]
fn a_device_that_arrives_after_the_budget_is_reported_and_leaked() {
    let (release_build, wait_build) = mpsc::channel::<()>();
    let (reports, report) = collector();

    let built = create_within_budget(
        move || {
            let _ = wait_build.recv();
            Ok::<_, String>(FakeDevice(2))
        },
        "late device",
        Duration::from_millis(200),
        true,
        Some(report),
    );
    assert_eq!(built, Ok(None));

    let _ = release_build.send(());

    let deadline = Instant::now();
    while !any_contains(&reports, "leaked") && deadline.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(10));
    }
    // A device nobody asked for any more still has to be accounted for out loud.
    assert!(any_contains(&reports, "leaked"));
}

/// A build that fails is a real error — a machine with no usable adapter, a
/// refused limit — and it has to reach the caller, who is the only one who can
/// turn it into a window that explains itself.
#[test]
fn a_failure_inside_the_build_reaches_the_caller() {
    let built = create_within_budget(
        || Err::<FakeDevice, _>("no adapter".to_string()),
        "throwing",
        Duration::from_secs(30),
        true,
        None,
    );
    assert_eq!(built, Err("no adapter".to_string()));
}

/// The browser leg: one thread, so there is nothing to build on and nothing to
/// bound it with. Built inline, and runnable on the desktop because the
/// platform is a parameter.
#[test]
fn with_no_background_thread_the_device_is_built_inline() {
    let caller = std::thread::current().id();
    let build_thread = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&build_thread);

    let built = create_within_budget(
        move || {
            *seen.lock().unwrap() = Some(std::thread::current().id());
            Ok::<_, String>(FakeDevice(3))
        },
        "browser",
        Duration::from_millis(1),
        false,
        None,
    );

    assert!(matches!(built, Ok(Some(_))));
    assert_eq!(*build_thread.lock().unwrap(), Some(caller));
}

/// The budget and the message a user is shown when it runs out are agg-sharp's
/// (`GpuStartup.DefaultBudget`, `WebGpuControl`'s start-up error).
#[test]
fn rust_only_startup_budget_and_timeout_message_match_mattercad() {
    assert_eq!(GPU_STARTUP_BUDGET, Duration::from_secs(15));
    let error = agg_gui_wgpu::GpuInitError::StartupTimedOut {
        budget: GPU_STARTUP_BUDGET,
    };
    assert_eq!(
        error.to_string(),
        "The GPU device could not be created: the adapter or device request did not return \
         within 15s."
    );
    assert_eq!(
        agg_gui_wgpu::GpuConfig::new("t").startup_budget,
        GPU_STARTUP_BUDGET
    );
}
