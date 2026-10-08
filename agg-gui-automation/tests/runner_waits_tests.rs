//! Rust-only tests of the runner's waits (agg-gui-automation
//! `src/runner/waits.rs`): `delay`, `wait_for_pending_ui_work`,
//! `wait_until` / `wait_for`, `assert` and `wait_for_draw`, and how C#'s
//! sleeps and reset events map to pumped frames on the virtual clock
//! (`docs/design/gui-automation.md`, section 5). The ported C# test that
//! exercises them is `automation_runner_tests.rs`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agg_gui::{clock, ui_thread};
use agg_gui_automation::runner::{
    DEFAULT_CHECK_INTERVAL_MILLISECONDS, DEFAULT_DELAY_SECONDS, DEFAULT_UI_WORK_WAIT_MILLISECONDS,
};
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationError, AutomationRunner, AutomationWindow, RunOptions,
};

fn window() -> (AutomationWindow, ()) {
    (AutomationWindow::new(300.0, 200.0), ())
}

/// Run `body` against an empty window and return what it returns.
fn run<R: Send + 'static>(body: impl FnOnce(&mut AutomationRunner) -> R + Send + 'static) -> R {
    show_window_and_execute_tests(RunOptions::default(), window, move |runner, _| {
        let result = body(runner);
        runner.mark_test_complete();
        result
    })
    .expect("run")
}

#[test]
fn rust_only_delay_pumps_a_frame_per_frame_interval_of_ui_time() {
    let (frames, elapsed) = run(|runner| {
        let frames = runner.driver().frames();
        let started = clock::now();
        runner.delay(DEFAULT_DELAY_SECONDS);
        (runner.driver().frames() - frames, clock::since(started))
    });

    assert_eq!(frames, 20, "0.2 s of 10 ms frames");
    assert_eq!(elapsed, Duration::from_millis(200));
}

#[test]
fn rust_only_a_zero_delay_pumps_nothing() {
    let frames = run(|runner| {
        let frames = runner.driver().frames();
        runner.delay(0.0);
        runner.delay(-1.0);
        runner.driver().frames() - frames
    });

    assert_eq!(frames, 0);
}

#[test]
fn rust_only_waiting_for_pending_ui_work_runs_what_was_queued_in_one_frame() {
    let (ran_before, answered, ran_after, frames) = run(|runner| {
        let ran = Arc::new(AtomicBool::new(false));
        let ran_in_action = Arc::clone(&ran);
        ui_thread::run_on_idle(move || ran_in_action.store(true, Ordering::SeqCst));
        let ran_before = ran.load(Ordering::SeqCst);
        let frames = runner.driver().frames();
        let answered = runner.wait_for_pending_ui_work(DEFAULT_UI_WORK_WAIT_MILLISECONDS);
        (
            ran_before,
            answered,
            ran.load(Ordering::SeqCst),
            runner.driver().frames() - frames,
        )
    });

    assert!(!ran_before);
    assert!(answered);
    assert!(ran_after, "the queued action ran before the wait returned");
    assert_eq!(frames, 1);
}

#[test]
fn rust_only_a_zero_millisecond_pending_ui_work_wait_gives_up_at_once() {
    let (answered, frames) = run(|runner| {
        let frames = runner.driver().frames();
        let answered = runner.wait_for_pending_ui_work(0);
        (answered, runner.driver().frames() - frames)
    });

    assert!(!answered, "C# reports a zero-millisecond wait as given up");
    assert_eq!(frames, 0);
}

#[test]
fn rust_only_wait_until_looks_again_each_check_interval_until_the_condition_holds() {
    let (satisfied, frames_waited) = run(|runner| {
        let start = runner.driver().frames();
        let satisfied = runner.wait_until(
            |runner| runner.driver().frames() >= start + 7,
            5.0,
            DEFAULT_CHECK_INTERVAL_MILLISECONDS,
        );
        (satisfied, runner.driver().frames() - start)
    });

    assert!(satisfied);
    assert_eq!(frames_waited, 7, "one 10 ms frame between looks");
}

#[test]
fn rust_only_wait_until_gives_up_at_its_maximum_and_answers_with_a_last_look() {
    let (satisfied, looks, elapsed) = run(|runner| {
        let mut looks = 0;
        let started = clock::now();
        let satisfied = runner.wait_until(
            |_| {
                looks += 1;
                false
            },
            0.5,
            100,
        );
        (satisfied, looks, clock::since(started))
    });

    assert!(!satisfied);
    // Looks at 0, 100, ..., 400 ms, then the last look at 500 ms.
    assert_eq!(looks, 6);
    assert_eq!(elapsed, Duration::from_millis(500));
}

#[test]
fn rust_only_a_zero_second_wait_until_is_a_single_look() {
    let (met, unmet, looks, frames) = run(|runner| {
        let frames = runner.driver().frames();
        let met = runner.wait_until(|_| true, 0.0, 10);
        let mut looks = 0;
        let unmet = runner.wait_until(
            |_| {
                looks += 1;
                false
            },
            0.0,
            10,
        );
        (met, unmet, looks, runner.driver().frames() - frames)
    });

    assert!(met);
    assert!(!unmet);
    assert_eq!(looks, 1);
    assert_eq!(frames, 0);
}

#[test]
fn rust_only_wait_for_carries_on_whether_or_not_the_condition_held() {
    let elapsed = run(|runner| {
        let started = clock::now();
        runner
            .wait_for(|_| false, 0.3, 10)
            .wait_for(|_| true, 0.3, 10);
        clock::since(started)
    });

    assert_eq!(elapsed, Duration::from_millis(300));
}

#[test]
fn rust_only_assert_passes_a_condition_that_holds() {
    run(|runner| {
        runner.assert(|_| true, "never shown", 0.0, 10);
    });
}

#[test]
fn rust_only_assert_fails_the_run_with_require_failed() {
    let result = show_window_and_execute_tests(RunOptions::default(), window, |runner, _| {
        runner.assert(|_| false, "the thing never happened", 0.1, 10);
        runner.mark_test_complete();
    });

    assert_eq!(
        result,
        Err(AutomationError::BodyPanicked(
            "Require Failed: the thing never happened".to_string()
        ))
    );
}

#[test]
fn rust_only_wait_for_draw_paints_a_frame_even_when_nothing_asked_for_one() {
    let (idle_painted, forced_painted) = run(|runner| {
        // Let the window settle so a reactive frame has nothing to draw.
        runner.delay(0.1);
        let painted = runner.driver().frames_painted();
        runner.wait_for_pending_ui_work(DEFAULT_UI_WORK_WAIT_MILLISECONDS);
        let idle_painted = runner.driver().frames_painted() - painted;
        runner.wait_for_draw();
        let forced_painted = runner.driver().frames_painted() - painted - idle_painted;
        (idle_painted, forced_painted)
    });

    assert_eq!(idle_painted, 0);
    assert_eq!(forced_painted, 1);
}

/// Sends on drop, so the test can see the body unwind.
struct UnwindSignal(mpsc::Sender<bool>);

impl Drop for UnwindSignal {
    fn drop(&mut self) {
        let _ = self.0.send(std::thread::panicking());
    }
}

#[test]
fn rust_only_a_wait_in_a_timed_out_run_unwinds_at_its_next_frame() {
    let (tx, rx) = mpsc::channel();
    let started = Instant::now();

    let result = show_window_and_execute_tests(
        RunOptions {
            secs_to_test_failure: 0.3,
            timeout_is_the_expected_outcome: true,
            ..RunOptions::default()
        },
        window,
        move |runner, _| {
            let _signal = UnwindSignal(tx);
            // On the real clock this would wait a day; the cancel stops it.
            runner.wait_until(
                |_| {
                    std::thread::sleep(Duration::from_millis(5));
                    false
                },
                86_400.0,
                10,
            );
            runner.mark_test_complete();
        },
    );

    assert_eq!(result, Err(AutomationError::Timeout));
    let unwound = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the stuck wait unwinds once the run is cancelled");
    assert!(
        unwound,
        "the body left by panicking, not by finishing the wait"
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}
