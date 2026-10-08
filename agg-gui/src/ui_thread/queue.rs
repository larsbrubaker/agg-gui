//! One UI-thread work queue ([`UiQueue`]): agg-sharp `UiThread`'s queue state
//! and `InvokePendingActions`, as a value instead of process statics.
//!
//! [`super`] decides which queue a thread posts to and drains; this file is
//! the queue itself — immediate actions, deferred actions, intervals — and the
//! drain. As in C#, a drain runs its work outside the queue lock on a private
//! copy (an action may queue more work, which runs on the next drain; a nested
//! or concurrent drain never sees a half-built list), every queued action runs
//! exactly once whichever thread pumps, and an interval's next run is advanced
//! under the lock so two pumps cannot both run it.
//!
//! Times are [`crate::clock`] instants. A deferred action or interval queued
//! from a thread drained by this queue is stamped with that thread's clock
//! when it is queued; one queued from any other thread (a worker, whose clock
//! is not the UI's) is stamped by the next drain, so a virtual UI clock
//! governs it either way.

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
    action: Mutex<Option<RepeatAction>>,
    interval: Duration,
    next_run: Mutex<DueAt>,
    queue: Weak<Shared>,
}

impl RunningInterval {
    /// C# `Active`: false once cleared.
    pub fn active(&self) -> bool {
        lock(&self.action).is_some()
    }

    fn execute(&self) {
        if let Some(action) = lock(&self.action).as_mut() {
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
        crate::animation::signal_async_state_change();
    }

    /// C# `RunOnIdle(action, delayInSeconds)`.
    pub fn run_on_idle_after(&self, delay: Duration, action: impl FnOnce() + Send + 'static) {
        let due = self.due_after(delay);
        lock(&self.shared.pending)
            .deferred
            .push((due, Box::new(action)));
        // Wake once so the drain arms a timed redraw for it.
        crate::animation::signal_async_state_change();
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
            interval,
            next_run: Mutex::new(self.due_after(interval)),
            queue: Arc::downgrade(&self.shared),
        });
        lock(&self.shared.pending)
            .intervals
            .push(Arc::clone(&running));
        crate::animation::signal_async_state_change();
        running
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
            *lock(&interval.action) = None;
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
            *lock(&running.action) = None;
            pending.intervals.retain(|i| !Arc::ptr_eq(i, running));
        }
        None => *lock(&running.action) = None,
    }
}
