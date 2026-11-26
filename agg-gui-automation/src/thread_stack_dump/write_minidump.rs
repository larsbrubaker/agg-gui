//! Writes a minidump of this process: the port of agg-sharp's
//! `ThreadStackDump.WriteDumpOfThisProcess`, with `minidump-writer`
//! (rust-minidump) in place of the .NET runtime's createdump.
//!
//! Where the dump is written from differs per OS, because each OS allows a
//! different reader of this process's threads:
//!
//! - **macOS: in-process, from a dedicated thread.** Reading another
//!   process's threads needs its task port, and `task_for_pid` on another
//!   process needs a debugger entitlement or root, which a test runner has
//!   neither of; reading this task's own threads needs nothing. The writer
//!   runs on a thread of its own (which it leaves out of the dump) while the
//!   calling thread waits for it, so the caller is stopped inside this module
//!   for the whole capture, as C#'s capturing thread was. Other threads are
//!   not suspended: suspending them from inside the process would deadlock
//!   the writer on any allocator lock a suspended thread holds. A hung
//!   thread, the one a watchdog dumps for, is still; a thread that ran during
//!   the capture can come back torn, which the walker's capturing-thread
//!   check and the retry treat as a lost race.
//! - **Linux: a forked child.** ptrace cannot attach to the caller's own
//!   threads, so a forked child does it: agg-os's
//!   `linux::fork_minidump::write_minidump_from_fork` (feature
//!   `fork-minidump`), which holds the fork, the `PR_SET_PTRACER` that lets
//!   the child trace this process under Yama, the child's body and the wait,
//!   since what a child of a multi-threaded process may do is part of that
//!   code's soundness. Here its errors become this module's messages.
//! - Elsewhere there is no writer yet and every capture fails with a reason.
//!
//! The writer runs under part 1's [`DUMP_WRITER_TIMEOUT`]: a forked writer
//! is killed at it, and an in-process writer is abandoned at it.

use std::path::Path;

use super::CaptureFailure;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::DUMP_WRITER_TIMEOUT;

/// Writes a minidump of this process to `dump_path`. Returns the writer's
/// own log where it has one (none of the writers here keep one, so empty).
pub fn write_dump_of_this_process(dump_path: &Path) -> Result<String, CaptureFailure> {
    write_dump(dump_path)?;
    Ok(String::new())
}

#[cfg(target_os = "macos")]
fn write_dump(dump_path: &Path) -> Result<(), CaptureFailure> {
    use std::sync::mpsc;

    let path = dump_path.to_path_buf();
    let (done, finished) = mpsc::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("agg-gui-automation stack dump writer".to_string())
        .spawn(move || {
            // `None, None`: this task, and this (the writer's) thread as the
            // handler thread, which the writer leaves out of the dump.
            let result = std::fs::File::create(&path)
                .map_err(|error| format!("could not create {}: {error}", path.display()))
                .and_then(|mut file| {
                    minidump_writer::minidump_writer::MinidumpWriter::new(None, None)
                        .dump(&mut file)
                        .map(|_| ())
                        .map_err(|error| format!("minidump-writer failed: {error}"))
                });
            // The caller may have given up at the timeout; nothing to tell.
            let _ = done.send(result);
        })?;

    match finished.recv_timeout(DUMP_WRITER_TIMEOUT) {
        Ok(result) => result.map_err(CaptureFailure::from),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "the in-process dump writer did not finish within {} s and was abandoned",
            DUMP_WRITER_TIMEOUT.as_secs()
        )
        .into()),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("the in-process dump writer thread panicked".into())
        }
    }
}

#[cfg(target_os = "linux")]
fn write_dump(dump_path: &Path) -> Result<(), CaptureFailure> {
    linux::write_dump(dump_path)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn write_dump(_dump_path: &Path) -> Result<(), CaptureFailure> {
    Err("thread stack dumps have no minidump writer on this OS".into())
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::Path;

    use agg_os::linux::fork_minidump::{write_minidump_from_fork, ForkDumpError};

    use super::{CaptureFailure, DUMP_WRITER_TIMEOUT};

    pub(super) fn write_dump(dump_path: &Path) -> Result<(), CaptureFailure> {
        write_minidump_from_fork(dump_path, DUMP_WRITER_TIMEOUT).map_err(capture_failure)
    }

    /// The capture's failure for the forked writer's error: the OS error
    /// itself, or the message saying what became of the writer.
    fn capture_failure(error: ForkDumpError) -> CaptureFailure {
        match error {
            ForkDumpError::ParentPid(error) => error.into(),
            ForkDumpError::Io(error) => error.into(),
            ForkDumpError::WriterFailed { code, message } if message.is_empty() => {
                format!("the forked dump writer exited with {code}").into()
            }
            ForkDumpError::WriterFailed { message, .. } => message.into(),
            ForkDumpError::Signaled { pid, signal } => {
                format!("the forked dump writer (pid {pid}) died of signal {signal}").into()
            }
            ForkDumpError::WaitFailed(error) => {
                format!("waiting for the forked dump writer failed: {error}").into()
            }
            ForkDumpError::TimedOut { pid } => format!(
                "the forked dump writer (pid {pid}) did not exit within {} s and was killed",
                DUMP_WRITER_TIMEOUT.as_secs()
            )
            .into(),
            // `ForkDumpError` is non-exhaustive: a reason added later.
            other => other.into(),
        }
    }
}
