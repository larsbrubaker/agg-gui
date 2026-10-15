//! One UI-thread work queue ([`UiQueue`]): agg-sharp `UiThread`'s queue state
//! and `InvokePendingActions`, as a value instead of process statics.
//!
//! [`super`] decides which queue a thread posts to and drains; this file is
//! the queue itself — immediate actions, deferred actions, intervals — and the
//! drain. As in C#, a drain runs its work outside the queue lock on a private
//! copy (an action may queue more work, which runs on the next drain; a nested
//! or concurrent drain never sees a half-built list), every queued action runs
//! exactly once whichever thread pumps, and an interval's next run is advanced
//! under the lock so two pumps cannot both run it. An interval's action runs
//! with no lock held, so it may clear its own interval (C# `ClearInterval`
//! from inside the tick) or queue more work.
//!
//! Times are [`crate::clock`] instants. A deferred action or interval queued
//! from a thread drained by this queue is stamped with that thread's clock
//! when it is queued; one queued from any other thread (a worker, whose clock
//! is not the UI's) is stamped by the next drain, so a virtual UI clock
//! governs it either way.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use web_time::Instant;

use crate::unhandled::{run_contained, UnhandledOrigin};

pub(super) type Action = Box<dyn FnOnce() + Send>;
type RepeatAction = Box<dyn FnMut() + Send>;

pub(super) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// When deferred work is due: a UI-clock instant, or a delay still waiting for
/// the next drain to stamp it (queued from a thread with another clock).
#[derive(Clone, Copy)]
enum DueAt {
    At(Instant),
    After(Duration),
}

impl DueAt {
    fn stamp(&mut self, now: Instant) -> Instant {
        if let DueAt::After(delay) = *self {
            *self = DueAt::At(now + delay);
        }
        match *self {
            DueAt::At(at) => at,
            DueAt::After(_) => now,
        }
    }

    fn is_due(&self, now: Instant) -> bool {
        matches!(*self, DueAt::At(at) if at <= now)
    }
}

/// A repeating action from `set_interval`; pass it to `clear_interval` to stop
/// it.
pub struct RunningInterval {
    /// The repeating action; taken out while it runs, so no lock is held
    /// through the call, and put back afterwards unless cleared meanwhile.
    action: Mutex<Option<RepeatAction>>,
    /// Set by `clear_interval` (and `UiQueue::clear`); never unset.
    cleared: AtomicBool,
    interval: Duration,
    next_run: Mutex<DueAt>,
    queue: Weak<Shared>,
}

impl RunningInterval {
    /// C# `Active`: false once cleared.
    pub fn active(&self) -> bool {
        !self.cleared.load(Ordering::Acquire)
    }

    /// Stop the interval: mark it cleared and drop its action (when a tick
    /// is running it, the tick drops it on return instead).
    fn stop(&self) {
        self.cleared.store(true, Ordering::Release);
        let action = lock(&self.action).take();
        drop(action);
    }

    /// Run one tick. The action is moved out of its slot for the call, so it
    /// can call `clear_interval` on itself without deadlocking; a second pump
    /// reaching the same interval while a tick runs finds the slot empty and
    /// skips that tick rather than waiting for it.
    fn execute(&self) {
        let Some(action) = lock(&self.action).take() else {
            return;
        };
        // Put the action back even when the tick panics (the drain contains
        // the panic and the interval keeps running, as before).
        struct Restore<'a> {
            interval: &'a RunningInterval,
            action: Option<RepeatAction>,
        }
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                let mut slot = lock(&self.interval.action);
                // Checked under the slot lock: `stop` sets the flag before
                // taking this lock, so a clear racing this tick either sees
                // the action back and drops it, or this sees the flag.
                if !self.interval.cleared.load(Ordering::Acquire) {
                    *slot = self.action.take();
                }
            }
        }
        let mut restore = Restore {
            interval: self,
            action: Some(action),
        };
        if let Some(action) = restore.action.as_mut() {
            action();
        }
    }
}

#[derive(Default)]
struct Pending {
    call_later: Vec<Action>,
    deferred: Vec<(DueAt, Action)>,
    intervals: Vec<Arc<RunningInterval>>,
}

#[derive(Default)]
pub(super) struct Shared {
    pending: Mutex<Pending>,
    /// Frame-loop wakeups aimed at the thread(s) that drain this queue:
    /// bumped by posts and [`UiQueue::signal_async_state_change`], merged
    /// into a draining thread's draw request and epochs by
    /// `crate::animation` (see [`super::current_wakeups`]).
    wakeups: AtomicU64,
    /// Remote image loads started on a thread that uses this queue and not
    /// yet completed (`crate::widgets::markdown::image_loader`): each load
    /// holds this queue's handle and settles its count from whichever thread
    /// completes or drops it, so parallel UI threads count only their own.
    remote_images_in_flight: AtomicUsize,
}

/// A UI-thread work queue. Cheap to clone (a shared handle) and `Send`, so a
/// worker can carry the queue of the UI thread that started it and post to it
/// directly, and several threads can pump one queue.
#[derive(Clone, Default)]
pub struct UiQueue {
    pub(super) shared: Arc<Shared>,
}

enum Due {
    Once(Action),
    Repeat(Arc<RunningInterval>),
}

impl UiQueue {
    /// A new, empty queue that no thread uses yet (see
    /// [`UiQueue::attach_current_thread`]).
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `self` and `other` are the same queue.
    pub fn same_queue(&self, other: &UiQueue) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    /// When work delayed by `delay` and queued now is due: stamped with this
    /// thread's clock when this thread uses this queue, otherwise at the next
    /// drain.
    fn due_after(&self, delay: Duration) -> DueAt {
        if super::thread_uses(self) {
            DueAt::At(crate::clock::now() + delay)
        } else {
            DueAt::After(delay)
        }
    }

    /// C# `RunOnIdle(action)`: run `action` at this queue's next drain, even
    /// when called from the thread that drains it.
    pub fn run_on_idle(&self, action: impl FnOnce() + Send + 'static) {
        lock(&self.shared.pending).call_later.push(Box::new(action));
        self.signal_async_state_change();
    }

    /// C# `RunOnIdle(action, delayInSeconds)`.
    pub fn run_on_idle_after(&self, delay: Duration, action: impl FnOnce() + Send + 'static) {
        let due = self.due_after(delay);
        lock(&self.shared.pending)
            .deferred
            .push((due, Box::new(action)));
        // Wake once so the drain arms a timed redraw for it.
        self.signal_async_state_change();
    }

    /// C# `SetInterval`: run `action` every `interval`, first after one
    /// interval.
    pub fn set_interval(
        &self,
        action: impl FnMut() + Send + 'static,
        interval: Duration,
    ) -> Arc<RunningInterval> {
        let running = Arc::new(RunningInterval {
            action: Mutex::new(Some(Box::new(action))),
            cleared: AtomicBool::new(false),
            interval,
            next_run: Mutex::new(self.due_after(interval)),
            queue: Arc::downgrade(&self.shared),
        });
        lock(&self.shared.pending)
            .intervals
            .push(Arc::clone(&running));
        self.signal_async_state_change();
        running
    }

    /// Wake the thread(s) that drain this queue: their next
    /// `animation::wants_draw` / epoch read sees a draw request and advances
    /// the invalidation and async-state epochs, as
    /// [`crate::animation::signal_async_state_change`] does for the calling
    /// thread's own queue. Safe from any thread; a worker serving a UI thread
    /// other than the main one signals through that thread's
    /// [`super::current_queue`] handle. Also runs the host waker.
    pub fn signal_async_state_change(&self) {
        self.shared.wakeups.fetch_add(1, Ordering::AcqRel);
        if super::raises_poster_flags(self) {
            // Same-thread caller: raise its flags now as well, so a read in
            // the same dispatch sees them without waiting for the merge.
            crate::animation::bump_local_async_state();
        }
        crate::animation::wake_host();
    }

    /// Remote markdown image loads started on a thread that uses this queue
    /// (they wake it when they arrive) and not yet completed or dropped. A
    /// headless harness waits for its own App's queue to reach zero before
    /// checking pixels; loads other UI threads started are not counted.
    pub fn remote_images_in_flight(&self) -> usize {
        self.shared.remote_images_in_flight.load(Ordering::SeqCst)
    }

    /// One more remote image load started for this queue.
    pub(crate) fn remote_image_started(&self) {
        self.shared
            .remote_images_in_flight
            .fetch_add(1, Ordering::SeqCst);
    }

    /// One remote image load for this queue completed (or was dropped).
    pub(crate) fn remote_image_settled(&self) {
        self.shared
            .remote_images_in_flight
            .fetch_sub(1, Ordering::SeqCst);
    }

    /// The wakeup counter [`Self::signal_async_state_change`] bumps.
    pub(super) fn wakeups(&self) -> u64 {
        self.shared.wakeups.load(Ordering::Acquire)
    }

    /// C# `Count`: deferred actions waiting for their time.
    pub fn count(&self) -> usize {
        lock(&self.shared.pending).deferred.len()
    }

    /// C# `CountExpired`: deferred actions whose time has come on this
    /// thread's clock (work not yet stamped by a drain is not expired).
    pub fn count_expired(&self) -> usize {
        let now = crate::clock::now();
        lock(&self.shared.pending)
            .deferred
            .iter()
            .filter(|(due, _)| due.is_due(now))
            .count()
    }

    /// Drop everything queued and stop every interval.
    pub fn clear(&self) {
        let mut pending = lock(&self.shared.pending);
        pending.call_later.clear();
        pending.deferred.clear();
        for interval in pending.intervals.drain(..) {
            interval.stop();
        }
    }

    /// Run everything queued and due on the calling thread, then the drain
    /// hooks; each under panic containment (see [`crate::unhandled`]). Arms
    /// a redraw when anything ran and a timed redraw for the earliest work
    /// still waiting.
    pub(super) fn drain(&self) {
        let now = crate::clock::now();
        let (call_now, next_due) = {
            let mut pending = lock(&self.shared.pending);
            let mut call_now: Vec<Due> = pending.call_later.drain(..).map(Due::Once).collect();
            let deferred = std::mem::take(&mut pending.deferred);
            for (mut due, action) in deferred {
                if due.stamp(now) <= now {
                    call_now.push(Due::Once(action));
                } else {
                    pending.deferred.push((due, action));
                }
            }
            for interval in &pending.intervals {
                let mut next_run = lock(&interval.next_run);
                if next_run.stamp(now) <= now {
                    // Advance the due time under the lock so a second pump
                    // cannot queue it twice.
                    *next_run = DueAt::At(now + interval.interval);
                    call_now.push(Due::Repeat(Arc::clone(interval)));
                }
            }
            let next_due = pending
                .deferred
                .iter()
                .map(|(due, _)| *due)
                .chain(pending.intervals.iter().map(|i| *lock(&i.next_run)))
                .filter_map(|due| match due {
                    DueAt::At(at) => Some(at),
                    DueAt::After(_) => None,
                })
                .min();
            (call_now, next_due)
        };
        let ran_any = !call_now.is_empty();
        let mut unreported = None;
        for due in call_now {
            let payload = run_contained(UnhandledOrigin::IdleAction, || match due {
                Due::Once(action) => action(),
                Due::Repeat(interval) => interval.execute(),
            });
            unreported = unreported.or(payload);
        }
        for hook in super::drain_hooks() {
            unreported = unreported.or(run_contained(UnhandledOrigin::IdleAction, hook));
        }
        if ran_any {
            crate::animation::request_draw();
        }
        if let Some(due) = next_due {
            crate::animation::request_draw_after(due.saturating_duration_since(now));
        }
        if let Some(payload) = unreported {
            // No handler took it: let it escape, as an unhandled failure must
            // not vanish. Everything else this drain owed has already run.
            std::panic::resume_unwind(payload);
        }
    }
}

/// C# `ClearInterval`.
pub fn clear_interval(running: &Arc<RunningInterval>) {
    match running.queue.upgrade() {
        Some(shared) => {
            let mut pending = lock(&shared.pending);
            running.stop();
            pending.intervals.retain(|i| !Arc::ptr_eq(i, running));
        }
        None => running.stop(),
    }
}
