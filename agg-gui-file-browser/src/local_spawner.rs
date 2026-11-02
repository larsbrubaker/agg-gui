//! [`LocalSpawner`]: an `agg_gui_files::Spawner` for an app with no executor of its own. Work it
//! is given runs on the UI thread that spawned it, a step at a time, from that thread's drain of
//! agg-gui's UI queue (`agg_gui::ui_thread::add_drain_hook`, which the shells run every loop
//! iteration and a headless driver every pumped frame).
//!
//! - **Nothing runs inside `spawn_task`.** The task is queued and the thread's frame loop woken;
//!   its first step runs at the next drain, so a caller holding a borrow while it spawns never
//!   meets its own task.
//! - **One step per ready task per drain.** A task polled `Pending` waits for its waker. A wake
//!   from any thread marks the task ready and wakes the frame loop of the UI thread that spawned
//!   it (its `UiQueue`); a task still ready after a drain (it yielded) asks for another frame.
//!   An app whose work needs more than a step per frame runs its own executor and passes that.
//! - **Faults are reported, never lost.** `report_fault` hands the error's text to the report
//!   the spawner was made with (a browser's `BrowserConfig::report`); `log_error` goes to its
//!   log, standard error unless an app gives one. A task that panics is dropped and the panic
//!   goes on to the drain, which reports it through `agg_gui::report_unhandled`; the tasks that
//!   drain had not reached yet run at the next.
//!
//! The queue is per thread, as agg-gui's UI state is, so parallel headless tests never run each
//! other's work.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use agg_gui::ui_thread::{add_drain_hook, current_queue, UiQueue};
use agg_gui_files::{LocalTask, Spawner};

use crate::config::ErrorReport;

/// A queued task and its readiness.
struct Slot {
    /// `None` while a drain is polling it.
    future: RefCell<Option<LocalTask>>,
    wake: Arc<TaskWake>,
}

/// A task's waker: marks the task ready and wakes the spawning thread's frame loop.
struct TaskWake {
    ready: AtomicBool,
    queue: Mutex<UiQueue>,
}

impl Wake for TaskWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if !self.ready.swap(true, Ordering::AcqRel) {
            // A poisoned lock still holds the queue: a panic elsewhere must not stop the wake.
            let queue = self.queue.lock().unwrap_or_else(|p| p.into_inner());
            queue.signal_async_state_change();
        }
    }
}

thread_local! {
    static TASKS: RefCell<Vec<Rc<Slot>>> = const { RefCell::new(Vec::new()) };
}

fn forget(slot: &Rc<Slot>) {
    TASKS.with(|tasks| tasks.borrow_mut().retain(|s| !Rc::ptr_eq(s, slot)));
}

/// When a task's poll unwinds: takes the panicking task out of the queue, so it is not kept, and
/// marks the tasks this drain had not reached yet ready again, so they run at the next drain.
struct OnUnwind<'a> {
    polling: &'a Rc<Slot>,
    not_reached: &'a [Rc<Slot>],
}

impl Drop for OnUnwind<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            forget(self.polling);
            for slot in self.not_reached {
                slot.wake.ready.store(true, Ordering::Release);
            }
            agg_gui::animation::request_draw();
        }
    }
}

/// Runs one step of every ready task spawned on this thread by a [`LocalSpawner`]. The drain
/// hook every [`LocalSpawner`] installs; an app pumping its own loop without agg-gui's drain
/// calls it there. Safe to nest (a task that drains): a task already being polled is left to the
/// outer call.
pub fn run_ready_local_tasks() {
    let ready: Vec<Rc<Slot>> = TASKS.with(|tasks| {
        tasks
            .borrow()
            .iter()
            .filter(|s| s.wake.ready.swap(false, Ordering::AcqRel))
            .cloned()
            .collect()
    });
    for (at, slot) in ready.iter().enumerate() {
        let taken = slot.future.borrow_mut().take();
        let Some(mut future) = taken else {
            // An outer drain is polling it: keep the wake for the next drain.
            slot.wake.ready.store(true, Ordering::Release);
            continue;
        };
        let waker = Waker::from(Arc::clone(&slot.wake));
        let mut cx = Context::from_waker(&waker);
        let guard = OnUnwind {
            polling: slot,
            not_reached: &ready[at + 1..],
        };
        let poll = future.as_mut().poll(&mut cx);
        drop(guard);
        match poll {
            Poll::Ready(()) => forget(slot),
            Poll::Pending => *slot.future.borrow_mut() = Some(future),
        }
    }
    let more = TASKS.with(|tasks| {
        tasks
            .borrow()
            .iter()
            .any(|s| s.wake.ready.load(Ordering::Acquire))
    });
    if more {
        agg_gui::animation::request_draw();
    }
}

/// How many tasks spawned on this thread have not finished.
pub fn unfinished_local_tasks() -> usize {
    TASKS.with(|tasks| tasks.borrow().len())
}

/// Where a spawner logs a failure already handled: (category, message).
pub type ErrorLog = Rc<dyn Fn(&str, &str)>;

/// The log of a spawner whose app gives none: standard error.
fn log_to_stderr(category: &str, message: &str) {
    eprintln!("{category}: {message}");
}

/// A [`Spawner`] running work on the spawning UI thread's drains (see the module header).
#[derive(Clone)]
pub struct LocalSpawner {
    report: ErrorReport,
    log: ErrorLog,
}

impl LocalSpawner {
    /// A spawner reporting faults through `report` and logging to standard error. Installs the
    /// drain hook that runs its tasks (once per process, however many spawners there are).
    pub fn new(report: ErrorReport) -> Self {
        add_drain_hook(run_ready_local_tasks);
        Self {
            report,
            log: Rc::new(log_to_stderr),
        }
    }

    /// The same spawner, logging through `log` (category, message).
    pub fn with_log(mut self, log: ErrorLog) -> Self {
        self.log = log;
        self
    }
}

impl Spawner for LocalSpawner {
    fn spawn_task(&self, task: LocalTask) {
        let queue = current_queue();
        let wake = Arc::new(TaskWake {
            ready: AtomicBool::new(true),
            queue: Mutex::new(queue.clone()),
        });
        TASKS.with(|tasks| {
            tasks.borrow_mut().push(Rc::new(Slot {
                future: RefCell::new(Some(task)),
                wake,
            }))
        });
        queue.signal_async_state_change();
    }

    fn report_fault(&self, error: Arc<dyn std::error::Error + Send + Sync>) {
        (self.report)(&error.to_string());
    }

    fn log_error(&self, category: &str, message: &str) {
        (self.log)(category, message);
    }
}
