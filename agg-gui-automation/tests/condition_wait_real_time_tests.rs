//! Tests of how the runner's waits spend real time while the driver's
//! background-busy hook reports work (`src/runner/waits.rs`, "Real time in
//! condition waits"): a condition wait whose condition is false (the idle
//! wait's condition is idle) paces its frames to real time while the work
//! runs, so it can't give up before a worker thread's result lands; `delay`,
//! the pending-UI-work wait and bare pumps stay free; and a wait whose
//! condition already holds never sleeps.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use agg_gui::{App, Color, Rect};
use agg_gui_automation::{
    AutomationConfig, AutomationRunner, FrameKind, HeadlessDriver, ProbeWidget, UiDriver,
};

fn runner() -> AutomationRunner<HeadlessDriver> {
    let root = ProbeWidget::new("root").with_child(Box::new(
        ProbeWidget::new("red")
            .with_bounds(Rect::new(0.0, 0.0, 10.0, 10.0))
            .with_background(Color::rgb(1.0, 0.0, 0.0)),
    ));
    let mut runner = AutomationRunner::attach(
        HeadlessDriver::new(Box::new(root), 40, 40),
        AutomationConfig::default(),
    );
    runner.wait_for_idle(1.0);
    runner
}

/// A worker thread that finishes after `real_ms` of real time, with the busy
/// hook reporting it until then; the flag turns true when it is done.
fn start_worker(runner: &mut AutomationRunner<HeadlessDriver>, real_ms: u64) -> Arc<AtomicBool> {
    let done = Arc::new(AtomicBool::new(false));
    let worker_done = Arc::clone(&done);
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(real_ms));
        worker_done.store(true, Ordering::SeqCst);
    });
    let hook_done = Arc::clone(&done);
    runner
        .driver_mut()
        .set_background_busy(Box::new(move |_: &App| !hook_done.load(Ordering::SeqCst)));
    done
}

/// Work that never finishes.
fn always_busy(runner: &mut AutomationRunner<HeadlessDriver>) {
    runner
        .driver_mut()
        .set_background_busy(Box::new(|_: &App| true));
}

#[test]
fn a_condition_wait_lasts_until_a_worker_result_lands_in_real_time() {
    let mut runner = runner();
    // 0.5 s of UI time is 50 frames, well under a millisecond each here; the
    // worker needs 200 ms of real time.
    let done = start_worker(&mut runner, 200);
    let seen = Arc::clone(&done);
    assert!(
        runner.wait_until(move |_| seen.load(Ordering::SeqCst), 0.5, 10),
        "the wait saw the worker's result"
    );

    let done = start_worker(&mut runner, 200);
    let seen = Arc::clone(&done);
    runner.assert(
        move |_| seen.load(Ordering::SeqCst),
        "assert saw the worker's result",
        0.5,
        10,
    );
}

#[test]
fn a_name_wait_paces_its_frames_while_work_runs() {
    let mut runner = runner();
    // The name never shows; the work runs 200 ms. A wait of 2 s of UI time
    // takes at least the work's real time, then the rest goes free.
    let done = start_worker(&mut runner, 200);
    let started = Instant::now();
    assert!(!runner.wait_for_name("missing", 2.0));
    let waited = started.elapsed();
    assert!(
        done.load(Ordering::SeqCst),
        "the work finished inside the wait"
    );
    assert!(
        waited >= Duration::from_millis(150),
        "the wait's frames took real time while the work ran: {waited:?}"
    );
}

#[test]
fn the_idle_wait_lasts_until_the_work_ends_in_real_time() {
    let mut runner = runner();
    // 1 s of UI time is 100 frames, well under a millisecond each here; the
    // worker needs 200 ms of real time, and UI time keeps pace with it.
    let done = start_worker(&mut runner, 200);
    let started = agg_gui::clock::now();
    assert!(runner.wait_for_idle(1.0), "idle once the worker is done");
    assert!(done.load(Ordering::SeqCst));
    let ui_time = agg_gui::clock::since(started);
    assert!(
        ui_time < Duration::from_millis(900),
        "the wait used about the work's real time of its budget, not all of it: {ui_time:?}"
    );
}

#[test]
fn delay_the_pending_work_wait_and_pumps_take_no_real_time_while_work_runs() {
    let mut runner = runner();
    always_busy(&mut runner);
    let started = Instant::now();
    // 500 frames: 5 s if each slept a frame interval.
    runner.delay(5.0);
    assert!(!runner.wait_for_pending_ui_work(2000), "work still running");
    for _ in 0..100 {
        runner.driver_mut().pump(FrameKind::Reactive);
    }
    let took = started.elapsed();
    assert!(
        took < Duration::from_millis(2500),
        "800 busy frames outside condition waits sleep nothing: {took:?}"
    );
}

#[test]
fn a_satisfied_wait_never_sleeps() {
    let mut runner = runner();
    always_busy(&mut runner);
    let frames = runner.driver().frames();
    let started = Instant::now();
    for _ in 0..200 {
        assert!(runner.wait_until(|_| true, 5.0, 10));
        // Any widget of the name, shown or not: the look is what is measured.
        assert!(runner.wait_for_name_with("red", 5.0, false, None));
    }
    let took = started.elapsed();
    assert_eq!(runner.driver().frames(), frames, "no frame was pumped");
    assert!(
        took < Duration::from_millis(1000),
        "satisfied waits took {took:?}"
    );
}

#[test]
fn a_condition_wait_with_no_background_work_takes_no_real_time() {
    let mut runner = runner();
    runner
        .driver_mut()
        .set_background_busy(Box::new(|_: &App| false));
    let started = Instant::now();
    // 500 frames: 5 s if each slept a frame interval.
    assert!(!runner.wait_until(|_| false, 5.0, 10));
    let took = started.elapsed();
    assert!(
        took < Duration::from_millis(2500),
        "a false condition with nothing running sleeps nothing: {took:?}"
    );
}
