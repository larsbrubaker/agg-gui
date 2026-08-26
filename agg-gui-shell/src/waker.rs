//! The agg-gui host waker, and the guard that guarantees it is uninstalled.
//!
//! agg-gui signals a reactive host from background threads through
//! `animation::set_host_waker`. Ours sends a winit user event, which wakes a
//! loop parked in `ControlFlow::Wait`.
//!
//! Two rules, both learned the hard way:
//!
//! * **Install only once the window and the GPU exist.** A waker installed
//!   earlier can fire against an event loop that is about to be torn down by a
//!   failed init.
//! * **Uninstall on every exit path.** The waker slot is process-global and
//!   holds an `EventLoopProxy`; leaving a stale one behind after the loop ends
//!   keeps the proxy alive and makes a later `run` in the same process signal
//!   the wrong loop. That is what [`HostWakerGuard`] is for — it clears on
//!   drop, so a clean exit, an error return and a panic all behave.

use winit::event_loop::EventLoopProxy;

/// Clears the installed host waker when dropped.
pub(crate) struct HostWakerGuard;

impl HostWakerGuard {
    /// Install a waker that wakes `proxy`'s event loop, returning the guard
    /// that removes it again.
    pub(crate) fn install(proxy: EventLoopProxy<()>) -> Self {
        agg_gui::animation::set_host_waker(move || {
            // A closed event loop is not an error — the app is exiting and the
            // wakeup has nowhere to go.
            let _ = proxy.send_event(());
        });
        Self
    }
}

impl Drop for HostWakerGuard {
    fn drop(&mut self) {
        agg_gui::animation::clear_host_waker();
    }
}
