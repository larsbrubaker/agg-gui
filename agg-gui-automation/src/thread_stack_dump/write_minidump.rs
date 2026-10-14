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
//!   threads, so a child forked here does it. Yama (`ptrace_scope` 1) only
//!   lets an ancestor trace, so the parent first names the child as its
//!   tracer with `PR_SET_PTRACER` (what the .NET runtime does before it starts
//!   createdump), and the child waits on a pipe until it has. The child
//!   stops this process (SIGSTOP plus ptrace), writes the dump, resumes it and
//!   exits; its error, if any, comes back through a second pipe rather than
//!   the console.
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
    use std::fs::File;
    use std::io::{self, Read, Write};
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::{CaptureFailure, DUMP_WRITER_TIMEOUT};

    /// How often the parent checks whether the forked writer has exited.
    const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(5);

    fn pipe() -> io::Result<(File, File)> {
        let mut fds = [0; 2];
        // SAFETY: `fds` is a valid two-element array for pipe to fill.
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: pipe succeeded, so both descriptors are open and owned
        // by nothing else.
        let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        Ok((File::from(read), File::from(write)))
    }

    pub(super) fn write_dump(dump_path: &Path) -> Result<(), CaptureFailure> {
        // Everything the child needs is made before the fork, so the child
        // only writes.
        let parent = libc::pid_t::try_from(std::process::id())?;
        // SAFETY: gettid has no preconditions.
        let blamed_thread = unsafe { libc::gettid() };
        let mut dump_file = File::create(dump_path)?;
        let (mut go_read, mut go_write) = pipe()?;
        let (mut error_read, mut error_write) = pipe()?;

        // SAFETY: the child runs only the dump writer and then `_exit`s, never
        // returning into the caller's stack or running its destructors.
        // glibc keeps malloc usable in the child of a multithreaded fork.
        let child = unsafe { libc::fork() };
        if child < 0 {
            return Err(io::Error::last_os_error().into());
        }
        if child == 0 {
            drop(go_write);
            drop(error_read);
            // Wait for the parent to allow tracing (or to give up).
            let mut go = [0u8; 1];
            let _ = go_read.read(&mut go);
            let result =
                minidump_writer::minidump_writer::MinidumpWriterConfig::new(parent, blamed_thread)
                    .write(&mut dump_file);
            let code = match result {
                Ok(_) => 0,
                Err(error) => {
                    let _ = write!(error_write, "minidump-writer failed: {error}");
                    1
                }
            };
            // SAFETY: _exit ends the child without running the parent's
            // atexit handlers or flushing its copied stdio buffers.
            unsafe { libc::_exit(code) }
        }

        drop(go_read);
        drop(error_write);
        drop(dump_file);
        // Yama's ptrace_scope 1 only lets an ancestor trace this process;
        // name the child. Without Yama this fails with EINVAL and is not
        // needed.
        // SAFETY: prctl with PR_SET_PTRACER takes a pid and has no pointers.
        unsafe { libc::prctl(libc::PR_SET_PTRACER, child as libc::c_ulong, 0, 0, 0) };
        let _ = go_write.write_all(&[1]);
        drop(go_write);

        let status = wait_for_exit(child);
        // SAFETY: as above; 0 clears the tracer exception.
        unsafe { libc::prctl(libc::PR_SET_PTRACER, 0 as libc::c_ulong, 0, 0, 0) };

        let mut error = String::new();
        let _ = error_read.read_to_string(&mut error);
        match status {
            Err(timeout) => Err(timeout.into()),
            Ok(0) => Ok(()),
            Ok(code) if error.is_empty() => {
                Err(format!("the forked dump writer exited with {code}").into())
            }
            Ok(_) => Err(error.into()),
        }
    }

    /// The forked writer's exit code, or why there is none (it was killed at
    /// the timeout, or died of a signal).
    fn wait_for_exit(child: libc::pid_t) -> Result<i32, String> {
        let deadline = Instant::now() + DUMP_WRITER_TIMEOUT;
        loop {
            let mut status = 0;
            // SAFETY: `status` is a valid out pointer; `child` is our child.
            let waited = unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) };
            if waited == child {
                if libc::WIFEXITED(status) {
                    return Ok(libc::WEXITSTATUS(status));
                }
                if libc::WIFSIGNALED(status) {
                    return Err(format!(
                        "the forked dump writer (pid {child}) died of signal {}",
                        libc::WTERMSIG(status)
                    ));
                }
            } else if waited < 0 {
                return Err(format!(
                    "waiting for the forked dump writer failed: {}",
                    io::Error::last_os_error()
                ));
            }
            if Instant::now() >= deadline {
                // SAFETY: signalling and reaping our own child.
                unsafe {
                    libc::kill(child, libc::SIGKILL);
                    libc::waitpid(child, &mut status, 0);
                }
                return Err(format!(
                    "the forked dump writer (pid {child}) did not exit within {} s and was killed",
                    DUMP_WRITER_TIMEOUT.as_secs()
                ));
            }
            std::thread::sleep(EXIT_POLL_INTERVAL);
        }
    }
}
