//! `StartupFailureLog`: where an application records a failure that killed
//! its own startup — the port of agg-sharp's
//! `GuiAutomation/StartupFailureLog.cs`.
//!
//! A dead application builds none of its widgets, so the first thing a test
//! reports is a shallow "Named GuiWidget not found" several steps after the
//! cause. The boot code records the cause here, in a process-wide slot that
//! needs no ambient context, and
//! [`AutomationRunner::widget_not_found_message`](crate::AutomationRunner::widget_not_found_message)
//! reads it back in the failing test's own thread, so the symptom carries
//! the cause. The description is also written at once to the process's real
//! standard error (`std::io::stderr()`, which libtest's output capture —
//! only the `print!` family — does not intercept), so a run whose test never
//! fails visibly (a hang, a crashed host) still leaves the stack in the log.
//!
//! C# stamped the time as local `HH:mm:ss.fff`; without a time-zone database
//! this stamps UTC (`HH:mm:ss.fffZ`). C#'s `Exception.ToString()` (type,
//! message, stack) becomes the error's display text, its `source()` chain
//! and the backtrace where the failure is recorded.

use std::any::Any;
use std::error::Error;
use std::fmt::Write as _;
use std::io::Write;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

/// The sink a description is written to the moment it is recorded.
pub type ImmediateSink = Box<dyn Write + Send>;

/// The sentence [`StartupFailureLog::append_to`] puts between the message and
/// the recorded failure (C#'s literal).
pub const NEVER_STARTED: &str =
    "The application never finished starting up, so its widgets were never built. The startup failure was:";

struct State {
    last_failure_description: Option<String>,
    /// `None` until first used, then the real standard error unless a test
    /// replaced it (`Some(None)`: no sink at all).
    immediate_sink: Option<Option<ImmediateSink>>,
}

static STATE: Mutex<State> = Mutex::new(State {
    last_failure_description: None,
    immediate_sink: None,
});

/// A poisoned lock still holds a usable description: diagnostics never
/// turn one failure into a second.
fn state() -> MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// C# `StartupFailureLog` (a static class); see the module docs.
pub struct StartupFailureLog;

impl StartupFailureLog {
    /// C# `ImmediateSink` setter: where a recorded failure is written at once
    /// (`None`: nowhere). Settable so a test can read what was written
    /// without spraying the suite's own stderr.
    pub fn set_immediate_sink(sink: Option<ImmediateSink>) {
        state().immediate_sink = Some(sink);
    }

    /// C# `LastFailureDescription`: the last startup failure recorded in this
    /// process, or `None` when nothing has failed.
    pub fn last_failure_description() -> Option<String> {
        state().last_failure_description.clone()
    }

    /// C# `Record(phase, exception)`: record a failure that stopped the
    /// application from finishing its startup. `phase` names what was being
    /// brought up (empty reads as "startup").
    pub fn record(phase: &str, error: &dyn Error) {
        let mut text = error.to_string();
        let mut source = error.source();
        while let Some(cause) = source {
            let _ = write!(text, "\n ---> {cause}");
            source = cause.source();
        }
        Self::record_text(phase, &text);
    }

    /// [`record`](Self::record) for a caught panic's payload (what
    /// `std::panic::catch_unwind` returns), read as its `&str` or `String`
    /// message.
    pub fn record_panic(phase: &str, payload: &(dyn Any + Send)) {
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "a panic with a non-string payload".to_string()
        };
        Self::record_text(phase, &format!("panicked: {message}"));
    }

    fn record_text(phase: &str, failure: &str) {
        let description = describe(phase, failure);
        let mut state = state();
        state.last_failure_description = Some(description.clone());
        let sink = state
            .immediate_sink
            .get_or_insert_with(|| Some(Box::new(std::io::stderr())));
        if let Some(sink) = sink.as_mut() {
            // Diagnostics may never be the reason a startup failure turns
            // into a second failure.
            let _ = sink.write_all(description.as_bytes());
            let _ = sink.flush();
        }
    }

    /// C# `Reset`: forget any recorded failure. Called as an application
    /// boots, so one test's startup crash is never reported against the next
    /// test's unrelated failure.
    pub fn reset() {
        state().last_failure_description = None;
    }

    /// C# `AppendTo`: `message` with the recorded startup failure, if there
    /// is one, appended — so the shallow symptom carries the deep cause.
    /// `message` comes back unchanged when nothing was recorded.
    pub fn append_to(message: &str) -> String {
        match Self::last_failure_description() {
            Some(failure) if !failure.is_empty() => {
                format!("{message}\n{NEVER_STARTED}\n{failure}")
            }
            _ => message.to_string(),
        }
    }
}

/// C#'s description: `Application {phase} failed at {time} on thread
/// {thread}:` then the failure (with a backtrace) on its own lines.
fn describe(phase: &str, failure: &str) -> String {
    let phase = if phase.is_empty() { "startup" } else { phase };
    let thread = std::thread::current();
    let thread_label = match thread.name() {
        Some(name) => format!("{:?} ({name})", thread.id()),
        None => format!("{:?}", thread.id()),
    };
    let backtrace = std::backtrace::Backtrace::force_capture();
    format!(
        "Application {phase} failed at {} on thread {thread_label}:\n{failure}\n{backtrace}\n",
        utc_time_of_day()
    )
}

/// `HH:mm:ss.fffZ` of the current UTC time.
fn utc_time_of_day() -> String {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ms_of_day = since_epoch.as_millis() % 86_400_000;
    let (h, rest) = (ms_of_day / 3_600_000, ms_of_day % 3_600_000);
    let (m, rest) = (rest / 60_000, rest % 60_000);
    let (s, ms) = (rest / 1000, rest % 1000);
    format!("{h:02}:{m:02}:{s:02}.{ms:03}Z")
}
