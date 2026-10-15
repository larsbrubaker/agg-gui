//! A process-wide device built once, in the background, that callers wait on
//! within their own start-up budget — and that a caller after a timeout still
//! gets if the creation finishes late. [`crate::headless::HeadlessGpu::shared`]
//! keeps one [`SharedStartup`] per adapter choice.
//!
//! **Why not `create_within_budget` behind a `OnceLock`.** A window's device
//! that arrives after its budget must be leaked (`crate::gpu_budget`'s module
//! docs: dropping it races the window's destruction). A process-wide device has
//! no window to race, so a late arrival can be kept and handed out instead. And
//! a `OnceLock` caching the timeout would turn one slow request into a failure
//! for every later caller in the process: a machine at 100% CPU (parallel
//! builds next to a test run) takes longer than 15 s to open an ordinary
//! device, which then fails every GPU test in that binary even though the
//! device turns up a few seconds later.
//!
//! **The budget counts from when the creation began**, not from each call: a
//! caller waits until `started + its budget`. So callers that disagree get
//! each their own deadline on the one shared request, and a request that truly
//! hangs costs the first caller its budget and every caller after the
//! deadline nothing — each still gets the timeout, and each timeout is
//! reported, so a hung driver never passes silently.
//!
//! **What is settled stays settled.** A device, or a failure the creation
//! itself returned (no adapter, a refused request, a panic), is the answer
//! for the rest of the process; only "still running" is not an answer.
//!
//! The thread start is a parameter ([`Spawner`]) so the tests in
//! `gpu_shared_startup_tests.rs` can stand in for a refusing OS; the creation
//! is a closure so they can stand in for a slow or hung driver.

use std::panic::{self, AssertUnwindSafe};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::gpu_budget::{format_budget_seconds, panic_text, BudgetReport, Spawner};

/// The longest single condvar wait. The loop re-checks the deadline after
/// each, so a budget near `Duration::MAX` waits in steps rather than handing
/// the platform a duration it would have to clamp.
const MAX_WAIT_STEP: Duration = Duration::from_secs(3600);

/// One process-wide value of type `T` (a device bundle), created at most once.
pub(crate) struct SharedStartup<T: 'static, E> {
    state: Mutex<State<T, E>>,
    settled: Condvar,
}

enum State<T: 'static, E> {
    /// Nobody has asked yet.
    Idle,
    /// The creation is running. `abandoned` once a caller's budget expired
    /// while waiting on it, so the arrival is reported as late.
    Creating { started: Instant, abandoned: bool },
    /// The creation finished: the value (leaked, never dropped) or its error.
    Settled(Result<&'static T, E>),
}

impl<T, E> SharedStartup<T, E>
where
    T: Send + Sync + 'static,
    E: Clone + Send + 'static,
{
    pub(crate) const fn new() -> Self {
        Self {
            state: Mutex::new(State::Idle),
            settled: Condvar::new(),
        }
    }

    /// The shared value: created by `create` on the first call (on a thread
    /// `spawn` starts, or inline when `background_thread_available` is false
    /// or the thread is refused), and waited on until `budget` after that
    /// creation began. Later calls never run their `create`.
    ///
    /// `Ok(Some)` once the value exists, `Err` when the creation failed
    /// (a panic becomes `from_panic`'s error), `Ok(None)` when this caller's
    /// deadline passed with the creation still running — reported to
    /// `report`, and not cached: a later call gets the value if it arrives.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn get_or_create<F>(
        &'static self,
        create: F,
        label: &str,
        budget: Duration,
        from_panic: fn(String) -> E,
        background_thread_available: bool,
        report: &BudgetReport,
        spawn: Spawner,
    ) -> Result<Option<&'static T>, E>
    where
        F: FnOnce() -> Result<T, E> + Send + 'static,
    {
        let called = Instant::now();
        let mut state = self.lock();
        let started = match &*state {
            State::Settled(result) => return result.clone().map(Some),
            State::Creating { started, .. } => *started,
            State::Idle => {
                let started = Instant::now();
                *state = State::Creating {
                    started,
                    abandoned: false,
                };
                drop(state);
                self.start(
                    create,
                    label,
                    from_panic,
                    background_thread_available,
                    report,
                    spawn,
                );
                state = self.lock();
                started
            }
        };
        // `None`: a budget so long the deadline is past what `Instant` holds.
        let deadline = started.checked_add(budget);
        loop {
            if let State::Settled(result) = &*state {
                return result.clone().map(Some);
            }
            let remaining = match deadline {
                Some(deadline) => deadline.saturating_duration_since(Instant::now()),
                None => MAX_WAIT_STEP,
            };
            if remaining.is_zero() {
                break;
            }
            let (next, _) = self
                .settled
                .wait_timeout(state, remaining.min(MAX_WAIT_STEP))
                .unwrap_or_else(|e| e.into_inner());
            state = next;
        }
        if let State::Creating { abandoned, .. } = &mut *state {
            *abandoned = true;
        }
        drop(state);
        report(&format!(
            "GpuStartup: '{label}' did not produce a device inside {}s of its creation starting \
             (started {}s ago; this call waited {}s). The adapter or device request has not \
             returned; it keeps running, and a later call gets the device if it arrives.",
            format_budget_seconds(budget),
            format_budget_seconds(started.elapsed()),
            format_budget_seconds(called.elapsed()),
        ));
        Ok(None)
    }

    /// Run `create` to its end and settle the outcome — on its own thread
    /// when there is one, else here.
    fn start<F>(
        &'static self,
        create: F,
        label: &str,
        from_panic: fn(String) -> E,
        background_thread_available: bool,
        report: &BudgetReport,
        spawn: Spawner,
    ) where
        F: FnOnce() -> Result<T, E> + Send + 'static,
    {
        let job_label = label.to_string();
        let job_report = Arc::clone(report);
        let job = move || {
            // Never let a panic escape a thread nobody joins, and never leave
            // the state at `Creating` for callers to wait on forever.
            let outcome = panic::catch_unwind(AssertUnwindSafe(create))
                .unwrap_or_else(|payload| Err(from_panic(panic_text(payload.as_ref()))));
            self.settle(outcome, &job_label, &job_report);
        };
        if !background_thread_available {
            job();
            return;
        }
        // The job reaches the thread through a channel once the spawn has
        // succeeded, so a refused thread leaves it here to run inline.
        let (send_job, receive_job) = mpsc::channel::<Box<dyn FnOnce() + Send>>();
        let spawned = spawn(
            format!("gpu-startup ({label})"),
            Box::new(move || {
                if let Ok(job) = receive_job.recv() {
                    job();
                }
            }),
        );
        if let Err(error) = spawned {
            log::warn!("agg-gui-wgpu: could not start a budget thread ({error}); running inline");
            job();
            return;
        }
        if let Err(mpsc::SendError(job)) = send_job.send(Box::new(job)) {
            job();
        }
    }

    /// Record the creation's outcome and wake every waiting caller.
    fn settle(&self, outcome: Result<T, E>, label: &str, report: &BudgetReport) {
        // Never dropped: callers hold `&'static` borrows of it.
        let result = outcome.map(|value| &*Box::leak(Box::new(value)));
        let succeeded = result.is_ok();
        let mut state = self.lock();
        let late = match &*state {
            State::Creating {
                started,
                abandoned: true,
            } => Some(started.elapsed()),
            _ => None,
        };
        *state = State::Settled(result);
        self.settled.notify_all();
        drop(state);
        if let Some(elapsed) = late {
            let outcome = if succeeded {
                "arrived; later calls get it"
            } else {
                "failed; later calls get that error"
            };
            report(&format!(
                "GpuStartup: '{label}' {outcome} ({}s after its creation started, after a \
                 caller's budget had expired).",
                format_budget_seconds(elapsed),
            ));
        }
    }

    fn lock(&self) -> MutexGuard<'_, State<T, E>> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
#[path = "gpu_shared_startup_tests.rs"]
mod tests;
