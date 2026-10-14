//! `ThreadStackDump` (feature `stack-dump`): the port of agg-sharp's
//! `GuiAutomation/ThreadStackDump.cs`, which captures the call stack of every
//! thread in this process as text, so a hang that only reproduces on a build
//! server can name the frame it is stuck in. A hung UI thread prints nothing
//! by definition, and a CI log that says only "the window did not close" is
//! the same as no log at all.
//!
//! The mechanism is C#'s: have another process write a dump of this one,
//! then walk the dump. This module holds the machinery around that:
//!
//! - `retry` — [`capture_with_retries`]: a capture of a live process can lose
//!   a race with it, so a failed capture is retaken (a fresh dump, never a
//!   re-parse) up to [`CAPTURE_ATTEMPTS`] times, and the report admits every
//!   lost attempt; running out returns [`CaptureGaveUp`] with each cause.
//! - `dump_writer` — [`run_dump_writer`]: the writer subprocess, its output
//!   sent to a log file (never a pipe this process reads, never the console)
//!   and handed back, and the writer killed at a timeout.
//! - here — [`capture_with`]: one capture is a fresh, uniquely named dump
//!   file, written by a [`DumpWriterProgram`], walked by a [`DumpWalker`],
//!   then deleted.
//!
//! The minidump writer program and the walker that turns a dump into the
//! report (`ThreadStackDump.Capture`/`ReportFromDump`, thread labels, the
//! capturing-thread check) are the slice after this one
//! (`docs/design/gui-automation.md`, section 9).

mod dump_writer;
mod retry;

pub use dump_writer::{
    run_dump_writer, write_dump_with, DumpWriterError, DumpWriterProgram, DUMP_WRITER_TIMEOUT,
};
pub use retry::{
    capture_with_retries, CaptureFailure, CaptureGaveUp, CAPTURE_ATTEMPTS, DELAY_BETWEEN_ATTEMPTS,
};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Turns a written dump into the all-thread report.
pub trait DumpWalker {
    /// Walks the dump at `dump_path` (owned by the caller, who deletes it)
    /// into the report. `write_duration` is how long writing it took;
    /// `capture_started` is when the capture began. An error means the dump
    /// cannot be trusted (it lost a race with the running process), so
    /// [`capture_with`] takes a new one.
    fn report_from_dump(
        &self,
        reason: &str,
        dump_path: &Path,
        write_duration: std::time::Duration,
        capture_started: Instant,
    ) -> Result<String, CaptureFailure>;
}

/// Builds the all-thread stack report with `writer` and `walker`, retrying
/// a lost capture with a fresh dump and pausing [`DELAY_BETWEEN_ATTEMPTS`]
/// between attempts. C#'s `Capture` with the writer and walker supplied.
pub fn capture_with(
    reason: &str,
    writer: &DumpWriterProgram,
    walker: &dyn DumpWalker,
) -> Result<String, CaptureGaveUp> {
    capture_with_retries(
        reason,
        || capture_once(reason, writer, walker),
        || thread::sleep(DELAY_BETWEEN_ATTEMPTS),
    )
}

fn capture_once(
    reason: &str,
    writer: &DumpWriterProgram,
    walker: &dyn DumpWalker,
) -> Result<String, CaptureFailure> {
    // A distinct file per capture: two watchdogs latching at once must not
    // fight over one path, and a retry must not read back the file whose
    // contents just failed to parse.
    let dump_path = unique_dump_path();
    let started = Instant::now();

    let result = write_dump_with(writer, &dump_path)
        .map_err(CaptureFailure::from)
        .and_then(|_log| walker.report_from_dump(reason, &dump_path, started.elapsed(), started));

    // A leaked temp file is not worth failing a diagnostic over; the OS will
    // clear it.
    let _ = std::fs::remove_file(&dump_path);

    result
}

/// `agg-threadstacks-<pid>-<unique>.dmp` in the temp directory. C# used a
/// GUID; the time and a process-wide counter are as unique within one
/// process, and the pid separates processes.
fn unique_dump_path() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let count = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    std::env::temp_dir().join(format!(
        "agg-threadstacks-{}-{nanos:x}-{count}.dmp",
        std::process::id()
    ))
}
