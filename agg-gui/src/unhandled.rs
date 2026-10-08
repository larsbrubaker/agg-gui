//! Unhandled UI-thread failures: agg-sharp `UiThread.UnhandledException` /
//! `ReportUnhandledException`.
//!
//! Work agg-gui runs on the UI thread on someone else's behalf — the idle
//! actions [`crate::ui_thread`] drains each frame, and the hooks that run with
//! them — is contained: a panic in one of them is caught, turned into an
//! [`UnhandledReport`] and handed to [`report_unhandled`], and the rest of the
//! frame's work still runs. An automation runner installs a handler with
//! [`set_unhandled_handler`] so a silent UI failure becomes a test failure.
//!
//! The handler is per thread, like the rest of agg-gui's UI state, so tests
//! running on their own threads each see only their own failures. With no
//! handler installed nothing is swallowed: the containing code re-raises the
//! first unreported panic once the rest of its work has run (C#'s DEBUG
//! behaviour, which lets the exception escape; a release build in C# drops it
//! when nothing subscribes).

use std::any::Any;
use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

/// Where an unhandled failure happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnhandledOrigin {
    /// A queued UI-thread action (`ui_thread::run_on_idle` and friends,
    /// intervals, drain hooks).
    IdleAction,
}

/// One unhandled UI-thread failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnhandledReport {
    pub origin: UnhandledOrigin,
    /// The panic message (`"<non-string panic payload>"` when the payload was
    /// neither a `&str` nor a `String`).
    pub message: String,
}

impl UnhandledReport {
    /// A report for a caught panic `payload`.
    pub fn from_panic(origin: UnhandledOrigin, payload: &(dyn Any + Send)) -> Self {
        Self {
            origin,
            message: panic_message(payload),
        }
    }
}

/// The text of a panic payload.
pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

type Handler = Rc<dyn Fn(&UnhandledReport)>;

thread_local! {
    static HANDLER: RefCell<Option<Handler>> = const { RefCell::new(None) };
}

/// Restores the handler that was installed before [`set_unhandled_handler`]
/// when dropped.
#[must_use = "dropping the guard immediately removes the handler"]
pub struct UnhandledHandlerGuard {
    previous: Option<Handler>,
}

impl Drop for UnhandledHandlerGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        HANDLER.with(|h| *h.borrow_mut() = previous);
    }
}

/// C# `UiThread.UnhandledException += handler`, for this thread, until the
/// returned guard drops.
pub fn set_unhandled_handler(
    handler: impl Fn(&UnhandledReport) + 'static,
) -> UnhandledHandlerGuard {
    let previous = HANDLER.with(|h| h.borrow_mut().replace(Rc::new(handler)));
    UnhandledHandlerGuard { previous }
}

/// Whether this thread has an unhandled-failure handler.
pub fn has_unhandled_handler() -> bool {
    HANDLER.with(|h| h.borrow().is_some())
}

/// C# `UiThread.ReportUnhandledException`: hand `report` to this thread's
/// handler. Returns whether a handler took it. A handler that itself panics
/// is ignored, as C# ignores an exception thrown by a subscriber.
pub fn report_unhandled(report: &UnhandledReport) -> bool {
    // Cloned out so the handler may install or remove handlers itself.
    let Some(handler) = HANDLER.with(|h| h.borrow().clone()) else {
        return false;
    };
    let _ = catch_unwind(AssertUnwindSafe(|| handler(report)));
    true
}

/// Runs `work`, containing a panic: a panic is reported through
/// [`report_unhandled`]; when no handler takes it, its payload is returned so
/// the caller can re-raise it once the rest of its work is done.
pub(crate) fn run_contained(
    origin: UnhandledOrigin,
    work: impl FnOnce(),
) -> Option<Box<dyn Any + Send>> {
    let payload = catch_unwind(AssertUnwindSafe(work)).err()?;
    if report_unhandled(&UnhandledReport::from_panic(origin, payload.as_ref())) {
        None
    } else {
        Some(payload)
    }
}
