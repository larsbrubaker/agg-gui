//! Wall-clock budgets around the two GPU calls a window cannot afford to wait
//! on forever: building its device at start-up and draining the GPU at close.
//!
//! Ports agg-sharp `RenderCore/GpuStartup.cs` ([`create_within_budget`]) and
//! `RenderCore/GpuTeardown.cs` ([`drain_within_budget`]), with their budgets
//! ([`GPU_STARTUP_BUDGET`], [`GPU_TEARDOWN_BUDGET`]) and report text.
//! [`crate::Gpu::new`] builds its adapter and device through the first;
//! [`crate::Gpu::release_within_budget`] (and agg-gui-shell's close path)
//! drains through the second.
//!
//! **The stall the start-up budget exists for.** Acquiring an adapter and
//! opening a device are synchronous native calls — DXGI enumeration, then
//! creating the D3D12 device. On a machine with a real GPU they are quick; on a
//! loaded software rasterizer (WARP on GitHub's GPU-less Windows runners) one
//! of them occasionally does not come back. A window that cannot get a device
//! must fail and say so, rather than never opening: a reported failure can be
//! closed, retried and diagnosed, a hung start-up can only be killed.
//!
//! **The wait the teardown budget exists for.** Releasing a swapchain waits on
//! the device fence for the last submitted work, with an effectively infinite
//! timeout (wgpu-core's `Surface` drop unconfigures through the HAL; its
//! `Queue` drop waits too). On a software rasterizer one frame can take tens
//! of seconds, so the same call parks the UI thread inside the window close.
//! Only the drain may be abandoned, never the releases: unconfiguring and
//! releasing a surface are calls against the native window it was made over,
//! which the host destroys the moment its close path returns. So the drain (a
//! fence wait that touches no window) runs here under a budget, and the caller
//! releases on its own thread only if the drain came back in time; otherwise
//! it releases nothing, and the device and its swapchain leak until the
//! process exits — the only outcome with no race in it.
//!
//! **A device that arrives late is leaked, not dropped**, for the same reason:
//! dropping it releases its swapchain next to a window that may be gone.
//!
//! Both helpers run their work on a dedicated thread (never a pool: the work
//! may block for minutes). Where there is no second thread — the browser —
//! the work runs inline and cannot time out, which costs nothing there: the
//! browser has no native request to wedge. The platform is a parameter so a
//! desktop test can drive that leg.
//!
//! One deliberate difference from the C# join: the outcome is handed over
//! under the same lock the caller takes when its budget expires, so work that
//! finishes on the boundary is used rather than abandoned (C# could abandon a
//! device that had already arrived, or report a drain failure as "late").
//! Diagnostics go to a caller-supplied report, or to `log::warn!` (a library
//! must not print; C# writes to the console).

use std::any::Any;
use std::fmt::Display;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long a window waits for its device before giving up (C#
/// `GpuStartup.DefaultBudget`). Generous next to a healthy acquisition
/// (milliseconds with a real GPU, a second or two on a software rasterizer).
pub const GPU_STARTUP_BUDGET: Duration = Duration::from_secs(15);

/// How long a close waits for the GPU to drain before walking away (C#
/// `GpuTeardown.DefaultBudget`). A healthy device takes microseconds; this is
/// short enough to stay well under the watchdogs that call a UI "hung".
pub const GPU_TEARDOWN_BUDGET: Duration = Duration::from_secs(5);

/// Where budget diagnostics go. Called from whichever thread notices, so it
/// must tolerate being called from the work thread after the helper returned.
pub type BudgetReport = Arc<dyn Fn(&str) + Send + Sync>;

/// Whether this target can run work on a second thread (C#
/// `!OperatingSystem.IsBrowser()`).
pub const BACKGROUND_THREAD_AVAILABLE: bool = !cfg!(target_arch = "wasm32");

/// Seconds as C#'s `{TimeSpan.TotalSeconds:0.#}` prints them: at most one
/// decimal, none when it is zero (`15`, `0.2`).
pub fn format_budget_seconds(duration: Duration) -> String {
    let tenths = (duration.as_secs_f64() * 10.0).round();
    if tenths % 10.0 == 0.0 {
        format!("{}", tenths / 10.0)
    } else {
        format!("{:.1}", tenths / 10.0)
    }
}

/// The report sink, defaulting to the `log` facade.
fn report_to(report: Option<BudgetReport>) -> BudgetReport {
    report.unwrap_or_else(|| Arc::new(|message: &str| log::warn!("{message}")))
}

/// A panic payload's message, for a report.
fn panic_text(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with a non-text payload".to_string())
}

/// What the work thread produced: its own result, or the panic that ended it.
type Outcome<R> = Result<R, Box<dyn Any + Send>>;

/// The hand-over between the work thread and the caller. One lock over the
/// "who owns the outcome" decision: without it a result that lands just as
/// the budget expires is neither returned nor reported.
enum Slot<R> {
    Waiting,
    Done(Outcome<R>),
    Abandoned,
}

/// Run `work` on a thread named `thread_name`, waiting at most `budget` for
/// it. `Some` is the outcome when it arrived in time; `None` when the caller
/// gave up, in which case `late` receives the outcome on the work thread.
fn run_within_budget<R, W, L>(
    work: W,
    thread_name: String,
    budget: Duration,
    late: L,
) -> Option<Outcome<R>>
where
    R: Send + 'static,
    W: FnOnce() -> R + Send + 'static,
    L: FnOnce(Outcome<R>) + Send + 'static,
{
    let shared = Arc::new((Mutex::new(Slot::<R>::Waiting), Condvar::new()));
    let worker = Arc::clone(&shared);
    let spawned = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            // Never let a panic escape a thread nobody joins: it is handed
            // back to the caller, or reported by `late`.
            let outcome = panic::catch_unwind(AssertUnwindSafe(work));
            let (lock, ready) = &*worker;
            let mut slot = lock.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(*slot, Slot::Waiting) {
                *slot = Slot::Done(outcome);
                ready.notify_all();
                return;
            }
            drop(slot);
            late(outcome);
        });
    if let Err(error) = spawned {
        // No thread to run on (the OS refused one): the work cannot be
        // budgeted, so report that and give up rather than block.
        log::warn!("agg-gui-wgpu: could not start a budget thread: {error}");
        return None;
    }

    let (lock, ready) = &*shared;
    let slot = lock.lock().unwrap_or_else(|e| e.into_inner());
    let (mut slot, _) = ready
        .wait_timeout_while(slot, budget, |s| matches!(s, Slot::Waiting))
        .unwrap_or_else(|e| e.into_inner());
    match std::mem::replace(&mut *slot, Slot::Abandoned) {
        Slot::Done(outcome) => Some(outcome),
        _ => None,
    }
}

/// Build a device with a wall-clock budget (C#
/// `GpuStartup.CreateWithinBudget`).
///
/// Returns `Ok(Some(device))` when `create` finished in time, `Ok(None)` when
/// the budget expired first (the caller gets a report naming `label`, and a
/// device that turns up later is leaked and reported), and `Err` when `create`
/// failed inside the budget. A panic inside the budget resumes on the caller.
///
/// `create` must not touch UI-thread-affine state. With
/// `background_thread_available` false it runs inline and cannot time out.
pub fn create_within_budget<T, E, F>(
    create: F,
    label: &str,
    budget: Duration,
    background_thread_available: bool,
    report: Option<BudgetReport>,
) -> Result<Option<T>, E>
where
    T: Send + 'static,
    E: Display + Send + 'static,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    if !background_thread_available {
        return create().map(Some);
    }
    let report = report_to(report);
    let late_report = Arc::clone(&report);
    let late_label = label.to_string();
    let elapsed = Instant::now();
    let outcome = run_within_budget(
        create,
        format!("gpu-startup ({label})"),
        budget,
        move |outcome: Outcome<Result<T, E>>| match outcome {
            Ok(Ok(device)) => {
                late_report(&format!(
                    "GpuStartup: '{late_label}' arrived after it was abandoned and is being leaked \
                     rather than released - releasing it would put its swapchain teardown next to \
                     a window that may already be gone."
                ));
                // Deliberate: see the module docs.
                std::mem::forget(device);
            }
            Ok(Err(error)) => late_report(&format!(
                "GpuStartup: '{late_label}' failed after it was abandoned: {error}"
            )),
            Err(payload) => late_report(&format!(
                "GpuStartup: '{late_label}' failed after it was abandoned: {}",
                panic_text(payload.as_ref())
            )),
        },
    );
    match outcome {
        Some(Ok(result)) => result.map(Some),
        Some(Err(payload)) => panic::resume_unwind(payload),
        None => {
            report(&format!(
                "GpuStartup: '{label}' did not produce a device inside {}s (waited {}s). The \
                 adapter or device request has not returned; this window will have no device and \
                 cannot paint.",
                format_budget_seconds(budget),
                format_budget_seconds(elapsed.elapsed()),
            ));
            Ok(None)
        }
    }
}

/// Wait for the GPU to drain with a wall-clock budget (C#
/// `GpuTeardown.DrainWithinBudget`).
///
/// `drain` is the GPU wait and only the wait — anything that touches the
/// native window belongs on the caller's thread after this returns
/// `Ok(true)`. `Ok(false)` means the drain was abandoned: release nothing.
/// `Err` is the drain's own failure inside the budget; a panic inside the
/// budget resumes on the caller, and a failure after abandonment is reported
/// (naming `label`), never lost. With `background_thread_available` false the
/// drain runs inline and this returns its result.
pub fn drain_within_budget<E, F>(
    drain: F,
    label: &str,
    budget: Duration,
    background_thread_available: bool,
    report: Option<BudgetReport>,
) -> Result<bool, E>
where
    E: Display + Send + 'static,
    F: FnOnce() -> Result<(), E> + Send + 'static,
{
    if !background_thread_available {
        return drain().map(|()| true);
    }
    let report = report_to(report);
    let late_report = Arc::clone(&report);
    let late_label = label.to_string();
    let elapsed = Instant::now();
    let outcome = run_within_budget(
        drain,
        format!("gpu-drain ({label})"),
        budget,
        move |outcome: Outcome<Result<(), E>>| {
            let failure = match outcome {
                Ok(Ok(())) => return,
                Ok(Err(error)) => error.to_string(),
                Err(payload) => panic_text(payload.as_ref()),
            };
            late_report(&format!(
                "GpuTeardown: the abandoned drain of '{late_label}' then failed: {failure}"
            ));
        },
    );
    match outcome {
        Some(Ok(result)) => result.map(|()| true),
        Some(Err(payload)) => panic::resume_unwind(payload),
        None => {
            report(&format!(
                "GpuTeardown: '{label}' did not go idle inside {}s and was abandoned (waited {}s). \
                 Nothing is being released, so the device and its swapchain leak until the process \
                 exits - the alternative is releasing them while the window they draw to is \
                 destroyed.",
                format_budget_seconds(budget),
                format_budget_seconds(elapsed.elapsed()),
            ));
            Ok(false)
        }
    }
}
