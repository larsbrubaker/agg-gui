//! A minidump of this process written by a forked child (feature
//! `fork-minidump`, Linux only).
//!
//! agg-gui-automation's all-thread stack dump
//! (`src/thread_stack_dump/write_minidump.rs`) is the consumer: on Linux it
//! writes its dump with [`write_minidump_from_fork`] and turns a
//! [`ForkDumpError`] into its own messages. The retries, the timeout's value
//! and the report stay there.
//!
//! ptrace cannot attach to the caller's own threads, so a child forked here
//! does it. Yama (`ptrace_scope` 1) only lets an ancestor trace, so the
//! parent first names the child as its tracer with `PR_SET_PTRACER` (what the
//! .NET runtime does before it starts createdump), and the child waits on a
//! pipe until it has. The child stops this process (SIGSTOP plus ptrace),
//! writes the dump with minidump-writer, resumes it and exits; its error, if
//! any, comes back through a second pipe rather than the console.
//!
//! Why this is one function that fixes what the child does, rather than a
//! `fork` that runs a closure: in the child of a multi-threaded process only
//! some work is acceptable (the other threads are gone, and any lock they held
//! stays held), and whether a body is acceptable is an argument about that
//! body. The argument for this one is in the SAFETY comment on the fork.

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::num::TryFromIntError;
use std::path::Path;
use std::time::{Duration, Instant};

/// How often the parent checks whether the forked writer has exited.
const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// The forked writer's exit code when something in its body panicked: the
/// code a panicking Rust process exits with.
const PANIC_EXIT_CODE: libc::c_int = 101;

/// Ends the forked child with [`PANIC_EXIT_CODE`] when it is dropped. The
/// child makes one first thing after the fork and keeps it until its own
/// `_exit`, so a panic that unwinds out of the child's body ends the child
/// there instead of unwinding into the caller's stack (running the caller's
/// destructors and code in the child of a multi-threaded process).
struct ExitOnUnwind;

impl Drop for ExitOnUnwind {
    fn drop(&mut self) {
        // SAFETY: _exit has no preconditions; it ends the child here, without
        // running the parent's atexit handlers, flushing its copied stdio
        // buffers or unwinding any further.
        unsafe { libc::_exit(PANIC_EXIT_CODE) }
    }
}

/// Why [`write_minidump_from_fork`] wrote no dump.
#[derive(Debug)]
#[non_exhaustive]
pub enum ForkDumpError {
    /// This process's id does not fit a `pid_t`; nothing was created.
    ParentPid(TryFromIntError),
    /// Creating the dump file, a pipe, or the child failed.
    Io(io::Error),
    /// The child exited with a code other than 0: 1 when minidump-writer
    /// returned an error, 101 when something in the child panicked. `message`
    /// is what it reported through its error pipe (minidump-writer's error,
    /// prefixed `minidump-writer failed: `), empty when it reported nothing.
    WriterFailed {
        /// The child's exit code.
        code: i32,
        /// The child's report, possibly empty.
        message: String,
    },
    /// The child died of signal `signal`.
    Signaled {
        /// The child's process id.
        pid: i32,
        /// The signal's number.
        signal: i32,
    },
    /// `waitpid` on the child failed.
    WaitFailed(io::Error),
    /// The child had not exited at the timeout, and was killed and reaped.
    TimedOut {
        /// The child's process id.
        pid: i32,
    },
}

impl fmt::Display for ForkDumpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParentPid(error) => write!(f, "this process's id is not a pid_t: {error}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::WriterFailed { code, message } if message.is_empty() => {
                write!(f, "the forked dump writer exited with {code}")
            }
            Self::WriterFailed { message, .. } => write!(f, "{message}"),
            Self::Signaled { pid, signal } => {
                write!(
                    f,
                    "the forked dump writer (pid {pid}) died of signal {signal}"
                )
            }
            Self::WaitFailed(error) => {
                write!(f, "waiting for the forked dump writer failed: {error}")
            }
            Self::TimedOut { pid } => {
                write!(
                    f,
                    "the forked dump writer (pid {pid}) did not exit in time and was killed"
                )
            }
        }
    }
}

impl std::error::Error for ForkDumpError {}

impl From<io::Error> for ForkDumpError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Writes a minidump of this process to `dump_path` (created or truncated)
/// from a forked child, the calling thread as the dump's blamed thread. The
/// child is killed if it has not exited `timeout` after it was let go; a
/// `timeout` too large to add to the current [`Instant`] means no timeout
/// (the wait lasts until the child exits).
///
/// The whole process is stopped while the child writes. Blocks the calling
/// thread until the child has exited or been killed.
///
/// The child exits with 0 when the dump is written, 1 when minidump-writer
/// returns an error, and 101 when something in it panics; the last two come
/// back as [`ForkDumpError::WriterFailed`].
///
/// # What the caller should know
///
/// - `PR_SET_PTRACER` is one process-wide setting: two calls at the same
///   time overwrite each other's tracer, and both children cannot trace the
///   same threads, so one of them fails ([`ForkDumpError::WriterFailed`]).
///   Callers run one capture at a time.
/// - A process that reaps children elsewhere (`waitpid(-1)`, `SIGCHLD`
///   ignored) makes the wait fail ([`ForkDumpError::WaitFailed`]). After
///   that the child is neither killed nor reaped by this function, and
///   reading its report waits for it to exit.
/// - The timeout cannot fire while the child holds the process stopped: once
///   it has stopped and attached the threads, the thread polling for it is
///   stopped too, so a child that blocks mid-dump blocks the process.
/// - The child runs minidump-writer, which may call the `log` macros, and on
///   a panic the panic hook; so whatever global logger, panic hook and global
///   allocator the process installed run in the child. One that takes a lock
///   another thread held at the moment of the fork leaves the child blocked
///   forever: killed at the timeout if that happens before it has stopped
///   this process, blocking the process (as above) if after. That is a hang,
///   not memory unsafety, and this function cannot rule it out.
pub fn write_minidump_from_fork(dump_path: &Path, timeout: Duration) -> Result<(), ForkDumpError> {
    // Everything the child needs is made before the fork, so the child
    // only writes.
    let parent = libc::pid_t::try_from(std::process::id()).map_err(ForkDumpError::ParentPid)?;
    // SAFETY: gettid has no preconditions.
    let blamed_thread = unsafe { libc::gettid() };
    let mut dump_file = File::create(dump_path)?;
    // std's pipes are close-on-exec; the child does not exec, so it sees both
    // ends just as the parent does.
    let (mut go_read, mut go_write) = io::pipe()?;
    let (mut error_read, mut error_write) = io::pipe()?;

    // SAFETY: the child runs only the body below, which the caller cannot
    // change. Between `fork` and `_exit` it closes two pipe descriptors,
    // reads one byte from the third, then runs minidump-writer over values
    // made before the fork; minidump-writer allocates, opens and reads files
    // under /proc, stops (SIGSTOP) and ptraces every thread of the parent,
    // and may call the `log` macros, so whatever global logger the host
    // installed runs in the child. On failure the child formats the error
    // (allocating) and writes it to the error pipe; then it `_exit`s. The
    // `ExitOnUnwind` guard made first ends the child with `_exit(101)` if
    // any of that panics, after the panic hook has run, so the child never
    // unwinds into the caller's stack or runs the caller's destructors
    // (with `panic = "abort"` a panic aborts the child instead).
    //
    // This relies on the C library keeping the allocator usable in the
    // child of a multi-threaded process, which glibc does. What it cannot
    // rule out: a global allocator, logger or panic hook of the host that
    // takes a lock another thread held at the moment of the fork blocks the
    // child forever. That is a hang (bounded by the timeout until the child
    // has stopped the parent), not memory unsafety: no other thread runs in
    // the child to touch what the lock guards.
    let child = unsafe { libc::fork() };
    if child < 0 {
        return Err(io::Error::last_os_error().into());
    }
    if child == 0 {
        // Lives until the `_exit` below, which does not return, so the
        // guard's `Drop` runs only if the body unwinds.
        let _exit_on_unwind = ExitOnUnwind;
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
        // SAFETY: _exit ends the child without running the parent's atexit
        // handlers or flushing its copied stdio buffers.
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

    let status = wait_for_exit(child, timeout);
    // SAFETY: as above; 0 clears the tracer exception.
    unsafe { libc::prctl(libc::PR_SET_PTRACER, 0 as libc::c_ulong, 0, 0, 0) };

    let mut message = String::new();
    let _ = error_read.read_to_string(&mut message);
    match status? {
        0 => Ok(()),
        code => Err(ForkDumpError::WriterFailed { code, message }),
    }
}

/// The forked writer's exit code, or why there is none (it was killed at
/// the timeout, or died of a signal, or could not be waited for).
fn wait_for_exit(child: libc::pid_t, timeout: Duration) -> Result<i32, ForkDumpError> {
    // None: the timeout is too large to represent, so there is no deadline.
    let deadline = Instant::now().checked_add(timeout);
    loop {
        let mut status = 0;
        // SAFETY: `status` is a valid out pointer; `child` is our child.
        let waited = unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) };
        if waited == child {
            if libc::WIFEXITED(status) {
                return Ok(libc::WEXITSTATUS(status));
            }
            if libc::WIFSIGNALED(status) {
                return Err(ForkDumpError::Signaled {
                    pid: child,
                    signal: libc::WTERMSIG(status),
                });
            }
        } else if waited < 0 {
            return Err(ForkDumpError::WaitFailed(io::Error::last_os_error()));
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            // SAFETY: signalling and reaping our own child.
            unsafe {
                libc::kill(child, libc::SIGKILL);
                libc::waitpid(child, &mut status, 0);
            }
            return Err(ForkDumpError::TimedOut { pid: child });
        }
        std::thread::sleep(EXIT_POLL_INTERVAL);
    }
}
