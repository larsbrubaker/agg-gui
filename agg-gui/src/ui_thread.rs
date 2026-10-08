//! Work queued for the UI thread: agg-sharp `Gui/UiThread.cs`.
//!
//! [`run_on_idle`] queues an action (from any thread), [`run_on_idle_after`]
//! queues one to run once a delay has passed on the UI clock
//! ([`crate::clock`]), [`set_interval`] repeats one until [`clear_interval`].
//! The shells drain the queue every loop iteration and before each painted
//! frame ([`invoke_pending_actions`]), so what an action changes shows in that
//! frame; a headless driver drains it once per pumped frame. Queued work runs
//! under panic containment and reports through [`crate::report_unhandled`].
//!
//! Queuing wakes the frame loop of the thread that drains the queue
//! ([`UiQueue::signal_async_state_change`]: safe from any thread; it wakes a
//! parked native loop through the shell's waker and the web loop on its next
//! tick), and each drain arms `request_draw_after` for the earliest deferred
//! action or interval still pending, so a reactive loop sleeps exactly until
//! it is due.
//!
//! **Wakeups are per queue, so per UI thread.** Each queue counts the wakeups
//! aimed at it, and [`crate::animation`] merges only the calling thread's
//! queue's count into its draw request and epochs ([`current_wakeups`]). A
//! post or `animation::signal_async_state_change` from an unbound worker goes
//! to the main queue and wakes its owner; parallel UI threads (headless tests)
//! never see each other's wakeups.
//!
//! **One queue per UI thread.** C# has one process-wide queue; agg-gui's UI
//! state is per thread, and tests run windows on threads of their own in
//! parallel, so each UI thread gets its own [`UiQueue`] and a test can never
//! drain another test's work. Which queue a thread uses:
//! - A thread that drains ([`invoke_pending_actions`]), declares itself
//!   ([`mark_current_thread_as_ui_thread`]) or resets ([`reset_for_tests`])
//!   is bound to a queue: the process's **main** queue when no live UI thread
//!   owns it yet, otherwise a queue of its own.
//! - Any other thread (a worker) posts to the main queue — in an app, the one
//!   UI thread's. A worker serving a different UI thread posts through that
//!   thread's [`current_queue`] handle, or binds to it with
//!   [`UiQueue::attach_current_thread`] (which is also how several threads
//!   pump one queue).
//! - When the thread that owns the main queue ends, the next UI thread to bind
//!   adopts it, with anything still queued on it.
//!
//! Not ported: the `Func<Task>` overloads and `SwitchToUiThreadAsync` /
//! `YieldToFrame` (async/await plumbing with no Rust counterpart),
//! `RunWithFrequencyLimit` and `ExecuteWhen` (ported with their first
//! callers), `DrainForNestedPump` ([`invoke_pending_actions`] is already safe
//! to nest).

mod queue;

use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use web_time::Instant;

use queue::lock;
pub use queue::{clear_interval, RunningInterval, UiQueue};

/// The process's main queue and whether a live UI thread owns it.
struct Main {
    queue: UiQueue,
    owned: bool,
}

static MAIN: Mutex<Option<Main>> = Mutex::new(None);

/// Functions every drain runs after the queued actions.
static DRAIN_HOOKS: Mutex<Vec<fn()>> = Mutex::new(Vec::new());

/// Releases the main queue when the thread that owns it ends, so the next UI
/// thread adopts it instead of the work landing on a queue nobody drains.
struct MainOwnership;

impl Drop for MainOwnership {
    fn drop(&mut self) {
        if let Some(main) = lock(&MAIN).as_mut() {
            main.owned = false;
        }
    }
}

thread_local! {
    /// The queue this thread posts to and drains, once bound.
    static BOUND: RefCell<Option<UiQueue>> = const { RefCell::new(None) };
    /// C# `IsUiThread`: this thread pumps its queue.
    static PUMPS: Cell<bool> = const { Cell::new(false) };
    static OWNS_MAIN: RefCell<Option<MainOwnership>> = const { RefCell::new(None) };
}

fn main_queue() -> UiQueue {
    lock(&MAIN)
        .get_or_insert_with(|| Main {
            queue: UiQueue::new(),
            owned: false,
        })
        .queue
        .clone()
}

fn bound() -> Option<UiQueue> {
    BOUND.with(|b| b.borrow().clone())
}

/// Whether the calling thread is bound to `queue`.
fn thread_uses(queue: &UiQueue) -> bool {
    BOUND.with(|b| b.borrow().as_ref().is_some_and(|q| q.same_queue(queue)))
}

/// The queue the calling thread is bound to, binding it first when it is not:
/// the unowned main queue, or a new queue of its own.
fn bind_current_thread() -> UiQueue {
    if let Some(queue) = bound() {
        return queue;
    }
    let queue = {
        let mut main = lock(&MAIN);
        match main.as_mut() {
            Some(m) if m.owned => UiQueue::new(),
            Some(m) => {
                m.owned = true;
                OWNS_MAIN.with(|o| *o.borrow_mut() = Some(MainOwnership));
                m.queue.clone()
            }
            None => {
                let queue = UiQueue::new();
                *main = Some(Main {
                    queue: queue.clone(),
                    owned: true,
                });
                OWNS_MAIN.with(|o| *o.borrow_mut() = Some(MainOwnership));
                queue
            }
        }
    };
    BOUND.with(|b| *b.borrow_mut() = Some(queue.clone()));
    queue
}

impl UiQueue {
    /// Bind the calling thread to this queue: its posts go here and its
    /// drains pump it. For a worker serving a UI thread other than the main
    /// one, and for several threads pumping one queue.
    pub fn attach_current_thread(&self) {
        BOUND.with(|b| *b.borrow_mut() = Some(self.clone()));
    }
}

/// The queue the calling thread posts to: its own once bound, otherwise the
/// main queue. Capture it on a UI thread to post from a worker that serves it.
pub fn current_queue() -> UiQueue {
    bound().unwrap_or_else(main_queue)
}

/// The wakeup count of the queue the calling thread reads its wakeups from
/// (its bound queue, otherwise the main queue). [`crate::animation`] merges a
/// change into the thread's draw request and epochs. Zero while thread-locals
/// are being torn down.
pub(crate) fn current_wakeups() -> u64 {
    match BOUND.try_with(|b| b.borrow().as_ref().map(UiQueue::wakeups)) {
        Ok(Some(count)) => count,
        Ok(None) => main_queue().wakeups(),
        Err(_) => 0,
    }
}

/// Whether the calling thread reads its wakeups from `queue`.
fn reads_wakeups_of(queue: &UiQueue) -> bool {
    match BOUND.try_with(|b| b.borrow().as_ref().map(|q| q.same_queue(queue))) {
        Ok(Some(same)) => same,
        Ok(None) => main_queue().same_queue(queue),
        Err(_) => false,
    }
}

/// C# `CurrentTimerMs`: milliseconds on this thread's UI clock since the
/// process's first use of the queue (zero for a virtual time before that).
pub fn current_timer_ms() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = *EPOCH.get_or_init(Instant::now);
    crate::clock::since(epoch).as_millis() as u64
}

/// C# `RunOnIdle(action)`: run `action` on the UI thread at the next drain,
/// even when called from the UI thread.
pub fn run_on_idle(action: impl FnOnce() + Send + 'static) {
    current_queue().run_on_idle(action);
}

/// C# `RunOnIdle(action, delayInSeconds)`.
pub fn run_on_idle_after(delay: Duration, action: impl FnOnce() + Send + 'static) {
    current_queue().run_on_idle_after(delay, action);
}

/// C# `RunOnUiThread`: run now on the UI thread, otherwise queue.
pub fn run_on_ui_thread(action: impl FnOnce() + Send + 'static) {
    if is_ui_thread() {
        action();
    } else {
        run_on_idle(action);
    }
}

/// C# `SetInterval`: run `action` every `interval`, first after one interval.
pub fn set_interval(
    action: impl FnMut() + Send + 'static,
    interval: Duration,
) -> Arc<RunningInterval> {
    current_queue().set_interval(action, interval)
}

/// C# `IsUiThread`: the calling thread pumps a queue (it has drained one, or
/// marked itself).
pub fn is_ui_thread() -> bool {
    PUMPS.with(|p| p.get())
}

/// C# `MarkCurrentThreadAsUiThread`: bind the calling thread to its queue and
/// make it the UI thread. The shells call this at startup.
pub fn mark_current_thread_as_ui_thread() {
    bind_current_thread();
    PUMPS.with(|p| p.set(true));
}

/// C# `Count`: deferred actions waiting for their time on this thread's queue.
pub fn count() -> usize {
    current_queue().count()
}

/// C# `CountExpired`: deferred actions on this thread's queue whose time has
/// come.
pub fn count_expired() -> usize {
    current_queue().count_expired()
}

/// Run `hook` on every drain, after the queued actions (an app's own
/// per-frame pumps: executors, task polls). Adding a hook twice adds it once.
pub fn add_drain_hook(hook: fn()) {
    let mut hooks = lock(&DRAIN_HOOKS);
    if !hooks.iter().any(|h| *h as usize == hook as usize) {
        hooks.push(hook);
    }
}

fn drain_hooks() -> Vec<fn()> {
    lock(&DRAIN_HOOKS).clone()
}

/// C# `InvokePendingActions`: run everything queued and due on this thread's
/// queue, then the drain hooks. A panic in any of them is reported through
/// [`crate::report_unhandled`]; with no handler on this thread the first one
/// is re-raised after the rest has run. Safe to nest (an action may drain).
pub fn invoke_pending_actions() {
    mark_current_thread_as_ui_thread();
    current_queue().drain();
}

/// C# `ResetForTests`: drop everything queued on this thread's queue, stop its
/// intervals and forget that this thread is the UI thread. The thread stays
/// bound to its queue, so what it queues next is still its own.
pub fn reset_for_tests() {
    bind_current_thread().clear();
    PUMPS.with(|p| p.set(false));
}
