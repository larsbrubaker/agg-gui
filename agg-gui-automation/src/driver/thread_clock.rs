//! The UI clock shared by every headless driver on one thread.
//!
//! [`agg_gui::clock`] is per thread, so two drivers alive on one thread at
//! once (MatterCAD's restart tests hold the old and the new app) run on one
//! clock. A plain [`ClockGuard`] per driver would break that: guards restore
//! the clock they found, so dropping the first driver before the second put
//! the second back on the real clock mid-test, and the second's drop then
//! left the thread on a stale virtual clock. Here the drivers on a thread
//! share one guard instead:
//!
//! - the first [`ClockPolicy::Virtual`] driver on a thread takes the guard
//!   (the thread goes virtual at the current time); later ones join it;
//! - the guard drops, restoring the clock the thread had before, when the
//!   *last* virtual driver drops, in whatever order they go;
//! - every virtual driver's frame advances the shared clock by one frame
//!   interval (each frame is UI time, whichever window pumped it).
//!
//! The drivers alive on one thread must agree on the policy. A real-clock
//! driver beside a virtual one would see virtual time that only the other
//! advances, so a wait on it would never end; creating one is refused with a
//! panic naming the rule.

use std::cell::RefCell;

use agg_gui::clock::{self, ClockGuard};

use super::ClockPolicy;

#[derive(Default)]
struct ThreadClock {
    virtual_drivers: usize,
    real_drivers: usize,
    guard: Option<ClockGuard>,
}

thread_local! {
    static THREAD_CLOCK: RefCell<ThreadClock> = RefCell::new(ThreadClock::default());
}

/// A driver's membership in its thread's clock; leaving happens on drop.
pub(super) struct ClockShare {
    policy: ClockPolicy,
}

impl ClockShare {
    /// Join the thread's clock under `policy`. Panics when a driver on the
    /// other policy is alive on this thread (see the module docs).
    pub(super) fn join(policy: ClockPolicy) -> Self {
        THREAD_CLOCK.with(|tc| {
            let mut tc = tc.borrow_mut();
            match policy {
                ClockPolicy::Virtual => {
                    assert!(
                        tc.real_drivers == 0,
                        "a virtual-clock HeadlessDriver cannot share its thread with a live \
                         real-clock one: drivers alive on one thread must use the same ClockPolicy"
                    );
                    if tc.virtual_drivers == 0 {
                        tc.guard = Some(clock::scoped_virtual(None));
                    }
                    tc.virtual_drivers += 1;
                }
                ClockPolicy::Real => {
                    assert!(
                        tc.virtual_drivers == 0,
                        "a real-clock HeadlessDriver cannot share its thread with a live \
                         virtual-clock one: drivers alive on one thread must use the same ClockPolicy"
                    );
                    tc.real_drivers += 1;
                }
            }
        });
        Self { policy }
    }
}

impl Drop for ClockShare {
    fn drop(&mut self) {
        // `try_with`: a driver dropped while the thread's locals are being
        // torn down has no clock left to restore.
        let guard = THREAD_CLOCK
            .try_with(|tc| {
                let mut tc = tc.borrow_mut();
                match self.policy {
                    ClockPolicy::Virtual => {
                        tc.virtual_drivers = tc.virtual_drivers.saturating_sub(1);
                        if tc.virtual_drivers == 0 {
                            return tc.guard.take();
                        }
                    }
                    ClockPolicy::Real => tc.real_drivers = tc.real_drivers.saturating_sub(1),
                }
                None
            })
            .ok()
            .flatten();
        // Dropped outside the borrow: restoring touches another thread-local.
        drop(guard);
    }
}
