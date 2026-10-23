//! Owned, cancellable scheduled draws: [`schedule_draw_after`] and the
//! [`ScheduledDraw`] handle it returns. A child module of `animation.rs`
//! (pulled in with `#[path]`), which merges these deadlines into the same
//! scheduled-draw channel as [`super::request_draw_after`].
//!
//! `request_draw_after` is fire and forget: once asked, the draw happens
//! even when the reason for it is gone. A watchdog that guards queued work
//! (wake me in a second if the work has not reported back) then paints an
//! idle window one more time after every settle, for nothing, and a host
//! whose loop sleeps while idle wakes for it. A [`ScheduledDraw`] can be
//! withdrawn: [`ScheduledDraw::cancel`], or dropping the handle.
//!
//! Every pending handle keeps its own deadline, so several can be pending at
//! once; the host is woken for the earliest of them and of the
//! `request_draw_after` deadline ([`super::peek_next_draw_deadline`]).
//! Cancelling the earliest leaves the next one in charge, and with none left
//! the host sleeps until something else asks. A deadline that comes due is
//! served exactly like a `request_draw_after` one: [`super::wants_draw`]
//! promotes it to an immediate draw and it leaves the pending set (the
//! handle then reports [`ScheduledDraw::is_pending`] `false`, and cancelling
//! it does nothing). A paint before a deadline is due keeps it pending;
//! [`super::reset_draw_signals`] withdraws every one.
//!
//! The pending set is per thread, like the rest of the draw signals, so a
//! handle is not `Send`: it is cancelled on the thread that scheduled it.
//! A handle a thread-local owns may be dropped while its thread exits, after
//! the pending set is gone; withdrawing then does nothing and never panics.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use web_time::Instant;

/// Ids for [`ScheduledDraw`] handles, unique across threads so a handle can
/// never withdraw another thread's draw.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

std::thread_local! {
    /// The pending cancellable deadlines: `(id, deadline)`, in scheduling
    /// order. A handful at most, so a plain vector serves.
    static PENDING: RefCell<Vec<(u64, Instant)>> = const { RefCell::new(Vec::new()) };
}

/// A draw scheduled with [`schedule_draw_after`] that can still be
/// withdrawn. Dropping the handle cancels the draw, as [`Self::cancel`]
/// does; keep it for as long as the draw should stay scheduled.
#[must_use = "dropping a ScheduledDraw cancels it; use request_draw_after for a draw nothing withdraws"]
#[derive(Debug)]
pub struct ScheduledDraw {
    id: u64,
    at: Instant,
    /// Not `Send`: the pending set is the scheduling thread's.
    _thread: PhantomData<Rc<()>>,
}

/// Schedule a draw `delay` from now (UI clock, [`crate::clock`]) that the
/// returned handle can withdraw. Wakes a sleeping host the same way
/// [`super::request_draw_after`] does ([`crate::frame_demand::notify`]).
pub fn schedule_draw_after(delay: Duration) -> ScheduledDraw {
    let at = crate::clock::now() + delay;
    // Relaxed: the counter orders nothing else; a u64 never wraps here.
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    PENDING.with(|p| p.borrow_mut().push((id, at)));
    crate::frame_demand::notify();
    ScheduledDraw {
        id,
        at,
        _thread: PhantomData,
    }
}

impl ScheduledDraw {
    /// When the draw is (or was) due, on the UI clock.
    pub fn deadline(&self) -> Instant {
        self.at
    }

    /// Whether the draw is still waiting: not yet served by the frame it
    /// asked for, and not withdrawn by [`super::reset_draw_signals`].
    pub fn is_pending(&self) -> bool {
        PENDING
            .try_with(|p| p.borrow().iter().any(|&(id, _)| id == self.id))
            .unwrap_or(false)
    }

    /// Withdraw the draw. Nothing paints for it; when it was the earliest
    /// scheduled deadline the host is told to look again
    /// ([`crate::frame_demand::notify`]), so a loop that sleeps while idle
    /// re-arms its timer for the next deadline or sleeps outright. Does
    /// nothing once the draw was served.
    pub fn cancel(self) {
        // `Drop` withdraws, so cancelling and dropping behave the same.
        drop(self);
    }

    fn withdraw(&self) {
        // Only `try_with`, everywhere: a handle dropped during thread
        // teardown (by another thread-local's destructor) may outlive the
        // pending set, and a panic there aborts the process. With the set
        // gone there is nothing left to withdraw and no host to tell.
        let earliest_moved = PENDING
            .try_with(|p| {
                let mut p = p.borrow_mut();
                let before = earliest_of(&p);
                let len = p.len();
                p.retain(|&(id, _)| id != self.id);
                (p.len() != len).then(|| (before, earliest_of(&p)))
            })
            .ok()
            .flatten()
            .is_some_and(|(before, after)| {
                // the host's wake time is the earliest of these and the
                // fire-and-forget deadline, which this does not touch
                let fire_and_forget = super::try_fire_and_forget_deadline();
                min_deadline(fire_and_forget, before) != min_deadline(fire_and_forget, after)
            });
        if earliest_moved {
            crate::frame_demand::notify();
        }
    }
}

impl Drop for ScheduledDraw {
    fn drop(&mut self) {
        self.withdraw();
    }
}

/// The earliest pending cancellable deadline.
pub(super) fn earliest() -> Option<Instant> {
    PENDING.with(|p| earliest_of(&p.borrow()))
}

fn earliest_of(pending: &[(u64, Instant)]) -> Option<Instant> {
    pending.iter().map(|&(_, at)| at).min()
}

/// The earlier of two optional deadlines.
pub(super) fn min_deadline(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (one, None) | (None, one) => one,
    }
}

/// Remove every deadline due at `now`; returns whether there was one.
pub(super) fn take_due(now: Instant) -> bool {
    PENDING.with(|p| {
        let mut p = p.borrow_mut();
        let len = p.len();
        p.retain(|&(_, at)| at > now);
        p.len() != len
    })
}

/// Withdraw every pending deadline.
pub(super) fn clear_all() {
    PENDING.with(|p| p.borrow_mut().clear());
}

#[cfg(test)]
#[path = "animation_scheduled_draw_cancel_tests.rs"]
mod tests;
