//! `LocalSpawner` runs an app's background work with no executor of its own: a spawned task
//! starts at the UI thread's next drain (never inside the spawn), waits for its waker, wakes from
//! any thread, steps once per drain, and its faults reach the report and log the app gave. Each
//! test runs on its own thread, whose queue is its own.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use agg_gui::ui_thread::{invoke_pending_actions, reset_for_tests};
use agg_gui_file_browser::{unfinished_local_tasks, BrowserConfig, LocalSpawner};
use agg_gui_files::{yield_now, Spawner};

fn spawner() -> LocalSpawner {
    reset_for_tests();
    LocalSpawner::new(Rc::new(|_| {}))
}

#[test]
fn a_spawned_task_runs_at_the_next_drain_not_inside_the_spawn() {
    let spawner = spawner();
    let ran = Rc::new(Cell::new(false));
    let flag = Rc::clone(&ran);
    spawner.spawn_task(Box::pin(async move { flag.set(true) }));
    assert!(!ran.get(), "nothing runs inside spawn_task");
    assert_eq!(unfinished_local_tasks(), 1);
    invoke_pending_actions();
    assert!(ran.get());
    assert_eq!(unfinished_local_tasks(), 0);
}

/// Pending until another thread opens it.
struct Gate(Arc<Mutex<(bool, Option<Waker>)>>);

impl Future for Gate {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut gate = self.0.lock().expect("gate lock");
        if gate.0 {
            Poll::Ready(())
        } else {
            gate.1 = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[test]
fn a_waiting_task_finishes_after_a_wake_from_another_thread() {
    let spawner = spawner();
    let shared = Arc::new(Mutex::new((false, None::<Waker>)));
    let done = Rc::new(Cell::new(false));
    let (gate, flag) = (Gate(Arc::clone(&shared)), Rc::clone(&done));
    spawner.spawn_task(Box::pin(async move {
        gate.await;
        flag.set(true);
    }));
    invoke_pending_actions();
    invoke_pending_actions();
    assert!(!done.get(), "a task waits for its waker");
    assert_eq!(unfinished_local_tasks(), 1);

    let opener = Arc::clone(&shared);
    std::thread::spawn(move || {
        let waker = {
            let mut gate = opener.lock().expect("gate lock");
            gate.0 = true;
            gate.1.take()
        };
        waker.expect("the task left its waker").wake();
    })
    .join()
    .expect("the opening thread");

    invoke_pending_actions();
    assert!(done.get());
    assert_eq!(unfinished_local_tasks(), 0);
}

#[test]
fn a_yielding_task_takes_one_step_per_drain() {
    let spawner = spawner();
    let steps = Rc::new(Cell::new(0));
    let counter = Rc::clone(&steps);
    spawner.spawn_task(Box::pin(async move {
        for _ in 0..3 {
            counter.set(counter.get() + 1);
            yield_now().await;
        }
    }));
    for expected in 1..=3 {
        invoke_pending_actions();
        assert_eq!(steps.get(), expected);
    }
    assert_eq!(
        unfinished_local_tasks(),
        1,
        "the last yield has not returned yet"
    );
    invoke_pending_actions();
    assert_eq!(unfinished_local_tasks(), 0);
}

#[test]
fn a_task_spawned_by_a_task_runs_at_a_later_drain() {
    let spawner = Rc::new(spawner());
    let order = Rc::new(RefCell::new(Vec::new()));
    let (inner_spawner, outer_order) = (Rc::clone(&spawner), Rc::clone(&order));
    spawner.spawn_task(Box::pin(async move {
        outer_order.borrow_mut().push("outer");
        let inner_order = Rc::clone(&outer_order);
        inner_spawner.spawn_task(Box::pin(
            async move { inner_order.borrow_mut().push("inner") },
        ));
    }));
    invoke_pending_actions();
    invoke_pending_actions();
    assert_eq!(*order.borrow(), vec!["outer", "inner"]);
    assert_eq!(unfinished_local_tasks(), 0);
}

#[test]
fn a_panicking_task_is_dropped_and_the_others_still_run() {
    let spawner = spawner();
    let ran = Rc::new(Cell::new(false));
    let flag = Rc::clone(&ran);
    spawner.spawn_task(Box::pin(async { panic!("a task that fails") }));
    spawner.spawn_task(Box::pin(async move { flag.set(true) }));
    // With no unhandled-panic handler on this thread, the drain re-raises the panic.
    let drained = std::panic::catch_unwind(std::panic::AssertUnwindSafe(invoke_pending_actions));
    assert!(drained.is_err(), "the drain passes the panic on");
    // The other task ran in the same drain or runs at the next.
    invoke_pending_actions();
    assert!(ran.get());
    assert_eq!(unfinished_local_tasks(), 0);
}

#[derive(Debug)]
struct Failure;

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Couldn't list Brackets")
    }
}

impl std::error::Error for Failure {}

#[test]
fn faults_reach_the_report_and_handled_failures_the_log() {
    reset_for_tests();
    let reported = Rc::new(RefCell::new(Vec::<String>::new()));
    let logged = Rc::new(RefCell::new(Vec::<String>::new()));
    let (report_to, log_to) = (Rc::clone(&reported), Rc::clone(&logged));
    let spawner = LocalSpawner::new(Rc::new(move |m: &str| {
        report_to.borrow_mut().push(m.into())
    }))
    .with_log(Rc::new(move |category: &str, message: &str| {
        log_to.borrow_mut().push(format!("{category}: {message}"))
    }));
    spawner.report_fault(Arc::new(Failure));
    spawner.log_error("LibrarySearch", "a section failed");
    assert_eq!(
        *reported.borrow(),
        vec!["Couldn't list Brackets".to_string()]
    );
    assert_eq!(
        *logged.borrow(),
        vec!["LibrarySearch: a section failed".to_string()]
    );
}

#[test]
fn the_default_configs_spawner_reports_through_the_configs_report() {
    reset_for_tests();
    let reported = Rc::new(RefCell::new(Vec::<String>::new()));
    let report_to = Rc::clone(&reported);
    let config = BrowserConfig::builder()
        .report(Rc::new(move |m: &str| {
            report_to.borrow_mut().push(m.into())
        }))
        .build();
    config.spawner.report_fault(Arc::new(Failure));
    (config.report)("Couldn't rename Bracket");
    assert_eq!(
        *reported.borrow(),
        vec![
            "Couldn't list Brackets".to_string(),
            "Couldn't rename Bracket".to_string()
        ]
    );
    let ran = Rc::new(Cell::new(false));
    let flag = Rc::clone(&ran);
    config
        .spawner
        .spawn_task(Box::pin(async move { flag.set(true) }));
    invoke_pending_actions();
    assert!(ran.get(), "the default spawner runs its work at the drain");
}
