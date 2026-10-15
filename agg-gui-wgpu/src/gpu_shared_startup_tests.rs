//! Rust-only tests for `gpu_shared_startup.rs`: stand-in creations (quick,
//! slow, hung, failing, panicking) and a refusing OS drive the shared device
//! creation the headless device is built through, with no GPU.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use super::SharedStartup;
use crate::gpu_budget::{spawn_thread, BudgetReport};

/// A stand-in device; identity is all these tests need.
#[derive(Debug, PartialEq, Eq)]
struct FakeDevice(u32);

type Startup = SharedStartup<FakeDevice, String>;

/// A fresh startup for one test. Leaked: the creation thread holds it for
/// `'static`, as the real one is a `static`.
fn startup() -> &'static Startup {
    Box::leak(Box::new(Startup::new()))
}

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

fn panicked(text: String) -> String {
    format!("panicked: {text}")
}

fn refuse_thread(_name: String, _job: Box<dyn FnOnce() + Send>) -> std::io::Result<()> {
    Err(std::io::Error::other("no more threads"))
}

/// A creation that blocks until `release` is sent (or its sender dropped),
/// counting how often it ran.
fn gated(
    id: u32,
    runs: &Arc<AtomicUsize>,
) -> (
    mpsc::Sender<()>,
    impl FnOnce() -> Result<FakeDevice, String> + Send + 'static,
) {
    let (release, gate) = mpsc::channel::<()>();
    let runs = Arc::clone(runs);
    let create = move || {
        runs.fetch_add(1, Ordering::SeqCst);
        let _ = gate.recv();
        Ok(FakeDevice(id))
    };
    (release, create)
}

#[test]
fn the_device_is_created_once_and_every_call_gets_it() {
    let shared = startup();
    let (reports, report) = collector();
    let runs = Arc::new(AtomicUsize::new(0));
    let call = |id: u32| {
        let runs = Arc::clone(&runs);
        shared.get_or_create(
            move || {
                runs.fetch_add(1, Ordering::SeqCst);
                Ok(FakeDevice(id))
            },
            "quick",
            Duration::from_secs(30),
            panicked,
            true,
            &report,
            spawn_thread,
        )
    };
    let first = call(1).unwrap().expect("in time");
    let second = call(2).unwrap().expect("already there");
    assert_eq!(*first, FakeDevice(1));
    // The same value, and the second creation never ran.
    assert!(std::ptr::eq(first, second));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert!(reports.lock().unwrap().is_empty());
}

/// The slow-machine case: the first caller's budget expires, the creation
/// finishes afterwards, and a later caller gets the device rather than a
/// cached timeout.
#[test]
fn a_device_that_arrives_after_a_timeout_reaches_later_callers() {
    let shared = startup();
    let (reports, report) = collector();
    let runs = Arc::new(AtomicUsize::new(0));
    let (release, create) = gated(5, &runs);
    let first = shared.get_or_create(
        create,
        "slow",
        Duration::from_millis(20),
        panicked,
        true,
        &report,
        spawn_thread,
    );
    assert_eq!(first, Ok(None));
    assert!(any_contains(
        &reports,
        "'slow' did not produce a device inside"
    ));

    release.send(()).unwrap();
    let (unused_release, unused_create) = gated(6, &runs);
    let later = shared
        .get_or_create(
            unused_create,
            "slow",
            Duration::from_secs(60),
            panicked,
            true,
            &report,
            spawn_thread,
        )
        .unwrap()
        .expect("the late device is handed out");
    drop(unused_release);
    assert_eq!(*later, FakeDevice(5));
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "only the first creation ran"
    );
    // The arrival is reported once it lands (on the creation thread, which
    // settles before it reports; wait for the report briefly).
    let deadline = Instant::now() + Duration::from_secs(10);
    while !any_contains(&reports, "'slow' arrived; later calls get it") {
        assert!(
            Instant::now() < deadline,
            "late arrival reported: {reports:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Callers that disagree each get their own deadline on the one request: a
/// longer budget than the first caller's keeps waiting and gets the device.
#[test]
fn a_later_caller_with_a_longer_budget_waits_for_the_device() {
    let shared = startup();
    let (_reports, report) = collector();
    let runs = Arc::new(AtomicUsize::new(0));
    let (release, create) = gated(9, &runs);
    let first = shared.get_or_create(
        create,
        "patient",
        Duration::from_millis(10),
        panicked,
        true,
        &report,
        spawn_thread,
    );
    assert_eq!(first, Ok(None));
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        release.send(()).unwrap();
    });
    let (_unused_release, unused_create) = gated(10, &runs);
    let patient = shared.get_or_create(
        unused_create,
        "patient",
        Duration::from_secs(60),
        panicked,
        true,
        &report,
        spawn_thread,
    );
    releaser.join().unwrap();
    assert_eq!(patient.unwrap().map(|d| d.0), Some(9));
}

/// A request that truly hangs is reported to every caller, and once its
/// deadline has passed a caller is told at once instead of waiting again.
#[test]
fn a_hung_creation_times_out_every_caller_without_waiting_again() {
    let shared = startup();
    let (reports, report) = collector();
    let runs = Arc::new(AtomicUsize::new(0));
    let budget = Duration::from_millis(300);
    let (release, create) = gated(1, &runs);
    let first = shared.get_or_create(
        create,
        "hung",
        budget,
        panicked,
        true,
        &report,
        spawn_thread,
    );
    assert_eq!(first, Ok(None));

    let (_unused_release, unused_create) = gated(2, &runs);
    let asked = Instant::now();
    let second = shared.get_or_create(
        unused_create,
        "hung",
        budget,
        panicked,
        true,
        &report,
        spawn_thread,
    );
    let waited = asked.elapsed();
    assert_eq!(second, Ok(None));
    assert!(
        waited < budget,
        "past the deadline a caller does not wait out the budget again (waited {waited:?})"
    );
    let timeouts = reports
        .lock()
        .unwrap()
        .iter()
        .filter(|m| m.contains("'hung' did not produce a device"))
        .count();
    assert_eq!(timeouts, 2, "each timed-out caller is reported");
    // Let the stand-in thread finish.
    drop(release);
}

#[test]
fn a_failed_creation_is_every_callers_answer() {
    let shared = startup();
    let (_reports, report) = collector();
    let runs = Arc::new(AtomicUsize::new(0));
    for _ in 0..2 {
        let runs = Arc::clone(&runs);
        let result = shared.get_or_create(
            move || {
                runs.fetch_add(1, Ordering::SeqCst);
                Err::<FakeDevice, _>("no adapter".to_string())
            },
            "failing",
            Duration::from_secs(30),
            panicked,
            true,
            &report,
            spawn_thread,
        );
        assert_eq!(result, Err("no adapter".to_string()));
    }
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[test]
fn a_panicking_creation_becomes_an_error_for_every_caller() {
    let shared = startup();
    let (_reports, report) = collector();
    for _ in 0..2 {
        let result = shared.get_or_create(
            || -> Result<FakeDevice, String> { panic!("driver exploded") },
            "panicking",
            Duration::from_secs(30),
            panicked,
            true,
            &report,
            spawn_thread,
        );
        assert_eq!(result, Err("panicked: driver exploded".to_string()));
    }
}

#[test]
fn a_refused_thread_or_no_thread_creates_inline() {
    let caller = std::thread::current().id();
    for background in [true, false] {
        let shared: &'static SharedStartup<std::thread::ThreadId, String> =
            Box::leak(Box::new(SharedStartup::new()));
        let (reports, report) = collector();
        let built = shared.get_or_create(
            || Ok(std::thread::current().id()),
            "inline",
            Duration::from_millis(1),
            panicked,
            background,
            &report,
            refuse_thread,
        );
        assert_eq!(built.unwrap().copied(), Some(caller));
        assert!(reports.lock().unwrap().is_empty());
    }
}
