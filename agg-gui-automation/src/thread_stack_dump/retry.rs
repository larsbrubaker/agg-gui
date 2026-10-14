//! The capture retry policy of agg-sharp's `GuiAutomation/ThreadStackDump.cs`
//! (`CaptureWithRetries`, `CaptureAttempts`, `DescribeFailedAttempts`).
//!
//! A dump is taken of a process that is still running, so the thread list it
//! describes can change underneath the capture and the capture can lose that
//! race. The loss is a property of the *written file*, not of its parse, so a
//! retry has to write a whole new dump rather than re-read the one that
//! failed; the attempt closure handed to [`capture_with_retries`] must take a
//! fresh dump every time. Split out from the capture itself (the parent
//! module's `capture_with`) so the policy is testable without losing a real
//! race on purpose.

use std::error::Error;
use std::fmt;
use std::time::Duration;

/// Why one capture attempt failed: anything the writer or the walker
/// returned. Its display text is what the report and the give-up error show.
pub type CaptureFailure = Box<dyn Error + Send + Sync + 'static>;

/// How many complete captures to try before giving up.
///
/// Five rather than two because the losses come in bursts, so attempts are
/// nowhere near independent. Measured in C# over 250 captures taken against
/// 24 threads each creating and joining a thread in a tight loop, on a 10
/// core mac also running 16 spinners: unretried, 35 captures failed; retried,
/// 52 first attempts failed but only 7 survived three attempts and none
/// survived five.
pub const CAPTURE_ATTEMPTS: usize = 5;

/// Pause between capture attempts. The race is lost in bursts, so an
/// immediate retry is likelier to land in the same burst; this is short
/// enough to be free next to the minutes a watchdog waited to get here.
pub const DELAY_BETWEEN_ATTEMPTS: Duration = Duration::from_millis(250);

/// Every attempt failed: C#'s `AggregateException` from `CaptureWithRetries`.
#[derive(Debug)]
pub struct CaptureGaveUp {
    /// Why the dump was taken.
    pub reason: String,
    /// One entry per attempt, in order ([`CAPTURE_ATTEMPTS`] of them).
    pub failures: Vec<CaptureFailure>,
}

impl fmt::Display for CaptureGaveUp {
    /// .NET's `AggregateException.Message`: the summary, then each inner
    /// message in parentheses, so a log line carries every attempt's cause.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Thread stack capture failed on all {CAPTURE_ATTEMPTS} attempts ({}).",
            self.reason
        )?;
        for failure in &self.failures {
            write!(f, " ({failure})")?;
        }
        Ok(())
    }
}

impl Error for CaptureGaveUp {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.failures
            .last()
            .map(|failure| failure.as_ref() as &(dyn Error + 'static))
    }
}

/// Runs `capture_attempt` until it produces a report or [`CAPTURE_ATTEMPTS`]
/// is exhausted, calling `wait_between_attempts` after each failed attempt
/// but the last.
///
/// A report that took more than one attempt starts with one line per lost
/// race, otherwise a report that took several tries looks exactly like one
/// that took none, and the next person to debug this starts over.
pub fn capture_with_retries(
    reason: &str,
    mut capture_attempt: impl FnMut() -> Result<String, CaptureFailure>,
    mut wait_between_attempts: impl FnMut(),
) -> Result<String, CaptureGaveUp> {
    let mut failures: Vec<CaptureFailure> = Vec::new();

    for attempt in 1..=CAPTURE_ATTEMPTS {
        match capture_attempt() {
            Ok(report) => {
                return Ok(if failures.is_empty() {
                    report
                } else {
                    describe_failed_attempts(&failures) + &report
                });
            }
            Err(failure) => {
                failures.push(failure);
                if attempt < CAPTURE_ATTEMPTS {
                    wait_between_attempts();
                }
            }
        }
    }

    Err(CaptureGaveUp {
        reason: reason.to_string(),
        failures,
    })
}

/// One line per lost race, printed above the report that finally worked.
///
/// C# also printed the exception's type name; a Rust error has no runtime
/// type name, so the line carries its display text alone.
fn describe_failed_attempts(failures: &[CaptureFailure]) -> String {
    let mut note = String::new();
    for (index, failure) in failures.iter().enumerate() {
        note.push_str(&format!(
            "(thread stack capture attempt {} of {CAPTURE_ATTEMPTS} failed and was re-taken: {failure})\n",
            index + 1
        ));
    }
    note
}
