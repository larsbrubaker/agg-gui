//! `ThreadStackDump` (feature `stack-dump`): the port of agg-sharp's
//! `GuiAutomation/ThreadStackDump.cs`, which captures the call stack of every
//! thread in this process as text, so a hang that only reproduces on a build
//! server can name the frame it is stuck in. A hung UI thread prints nothing
//! by definition, and a CI log that says only "the window did not close" is
//! the same as no log at all.
//!
//! The mechanism is C#'s: have something outside the threads being dumped
//! write a minidump of this process, then walk the dump. Its parts:
//!
//! - `retry` — [`capture_with_retries`]: a capture of a live process can lose
//!   a race with it, so a failed capture is retaken (a fresh dump, never a
//!   re-parse) up to [`CAPTURE_ATTEMPTS`] times, and the report admits every
//!   lost attempt; running out returns [`CaptureGaveUp`] with each cause.
//! - `write_minidump` — [`write_dump_of_this_process`]: `minidump-writer`
//!   run from a dedicated thread (macOS) or a forked child (Linux).
//! - `dump_writer` — [`run_dump_writer`]: a writer *program* run as a
//!   subprocess, its output sent to a log file (never a pipe this process
//!   reads, never the console) and handed back, and the writer killed at a
//!   timeout; [`capture_with`] takes one.
//! - `stack_pointers` — each dumped thread's stack pointer, and the repair
//!   of a dump whose thread contexts repeat one.
//! - `walker` — [`report_from_dump`]: the minidump walked into the report,
//!   frames named in-process, the capturing thread checked.
//! - `registry` — [`register_current_thread`]: the labels threads give
//!   themselves for the report.
//! - here — [`capture`], [`write_to_console`] and [`capture_with`]: one
//!   capture is a fresh, uniquely named dump file, written, walked, then
//!   deleted.

mod dump_writer;
mod registry;
mod retry;
mod stack_pointers;
mod walker;
mod write_minidump;

pub use dump_writer::{
    run_dump_writer, write_dump_with, DumpWriterError, DumpWriterProgram, DUMP_WRITER_TIMEOUT,
};
pub use registry::{current_os_thread_id, register_current_thread};
pub use retry::{
    capture_with_retries, CaptureFailure, CaptureGaveUp, CAPTURE_ATTEMPTS, DELAY_BETWEEN_ATTEMPTS,
};
pub use write_minidump::write_dump_of_this_process;

pub use stack_pointers::{
    drop_thread_contexts_with_repeated_stack_pointers, read_thread_stack_pointers,
    ThreadStackPointer,
};
pub use walker::{report_from_dump, MinidumpWalker, MAX_FRAMES_PER_THREAD};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// This module's path, which the walker looks for on the capturing thread.
const MODULE_PATH: &str = module_path!();

/// Builds the all-thread stack report: a minidump of this process written
/// by [`write_dump_of_this_process`] and walked by a [`MinidumpWalker`] that
/// checks the calling thread, retried as [`capture_with`] retries. An error
/// means every attempt failed; callers on a failure path should use
/// [`write_to_console`], which cannot fail.
pub fn capture(reason: &str) -> Result<String, CaptureGaveUp> {
    let walker = MinidumpWalker {
        capturing_thread: Some(current_os_thread_id()),
    };
    capture_with_retries(
        reason,
        || {
            capture_once(reason, &walker, |dump_path| {
                write_dump_of_this_process(dump_path)
            })
        },
        || thread::sleep(DELAY_BETWEEN_ATTEMPTS),
    )
}

/// Prints the all-thread stack report to stdout (which is what a test log
/// captures), degrading to a one-line note if the capture fails.
///
/// Never fails. It is only ever called from a path that is already
/// reporting a failure, and a diagnostic that replaces the failure it was
/// meant to explain is worse than no diagnostic.
pub fn write_to_console(reason: &str) {
    match capture(reason) {
        Ok(report) => println!("{report}"),
        Err(error) => println!("THREAD STACK DUMP FAILED ({reason}): {error}"),
    }
}

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
        || {
            capture_once(reason, walker, |dump_path| {
                write_dump_with(writer, dump_path).map_err(CaptureFailure::from)
            })
        },
        || thread::sleep(DELAY_BETWEEN_ATTEMPTS),
    )
}

fn capture_once(
    reason: &str,
    walker: &dyn DumpWalker,
    write_dump: impl FnOnce(&Path) -> Result<String, CaptureFailure>,
) -> Result<String, CaptureFailure> {
    // A distinct file per capture: two watchdogs latching at once must not
    // fight over one path, and a retry must not read back the file whose
    // contents just failed to parse.
    let dump_path = unique_dump_path();
    let started = Instant::now();

    let result = write_dump(&dump_path)
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
