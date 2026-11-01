//! A small local executor for the provider tests: a [`Spawner`] that keeps the tasks given to
//! it and polls them when pumped, and keeps what it is asked to log and every fault reported, so
//! a test can drive a search to its end and read what it logged.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use agg_gui_files::{LocalTask, Spawner};

/// Keeps tasks until pumped, and what they logged.
#[derive(Default)]
pub struct TestSpawner {
    tasks: RefCell<Vec<LocalTask>>,
    /// Every `log_error` line, as "[category] message".
    pub logs: RefCell<Vec<String>>,
    /// Every fault reported.
    pub faults: RefCell<Vec<String>>,
}

impl TestSpawner {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// Polls every task once; tasks that finish are dropped. Answers how many are left.
    pub fn pump(&self) -> usize {
        let tasks = std::mem::take(&mut *self.tasks.borrow_mut());
        let mut cx = Context::from_waker(Waker::noop());
        let mut left = Vec::new();
        for mut task in tasks {
            if task.as_mut().poll(&mut cx) == Poll::Pending {
                left.push(task);
            }
        }
        let mut tasks = self.tasks.borrow_mut();
        left.append(&mut tasks);
        *tasks = left;
        tasks.len()
    }

    /// Pumps until `done` holds, failing after `limit`.
    pub fn pump_until(&self, limit: Duration, done: impl Fn() -> bool) {
        let start = Instant::now();
        while !done() {
            self.pump();
            assert!(start.elapsed() < limit, "the work did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Spawner for TestSpawner {
    fn spawn_task(&self, task: LocalTask) {
        self.tasks.borrow_mut().push(task);
    }

    fn report_fault(&self, error: Arc<dyn std::error::Error + Send + Sync>) {
        self.faults.borrow_mut().push(error.to_string());
    }

    fn log_error(&self, category: &str, message: &str) {
        self.logs
            .borrow_mut()
            .push(format!("[{category}] {message}"));
    }
}
