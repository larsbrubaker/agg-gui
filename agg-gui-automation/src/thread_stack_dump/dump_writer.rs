//! The dump-writer subprocess of agg-sharp's `GuiAutomation/ThreadStackDump.cs`
//! (`RunDumpWriter`, `DumpWriterTimeout`, and the createdump launch of
//! `RunCreateDump`/`WriteDumpOfThisProcess`).
//!
//! A dump of this process is written by another process: a writer suspends
//! every thread of the process it dumps, and on Linux a process cannot
//! ptrace its own threads at all. [`run_dump_writer`] runs any such writer to
//! completion with three guarantees the C# learned the hard way:
//!
//! - **Its output never goes through a pipe this process reads.** The writer
//!   suspends every thread here, including whichever would drain the pipe, so
//!   once it has written a pipe buffer's worth (64 KB on macOS) it blocks on
//!   the write while this process stays suspended, forever. The writer's
//!   stdout and stderr go to a log file instead.
//! - **Its output is handed back, not printed.** A writer that inherits the
//!   host's stdout prints into the test run's console, where its status
//!   lines read like a crash; the log file is read back and returned.
//! - **It is killed at a timeout.** Nothing in-process can fire while the
//!   writer holds this process suspended, but a writer stalled any other way
//!   is killed and the capture fails with a reason the retry handles.
//!
//! [`DumpWriterProgram`] is the writer's command line and
//! [`write_dump_with`] the call that runs it against this process; which
//! program writes the real minidump is `thread_stack_dump`'s part 2.

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Longest one run of the dump writer may take before it is killed. A
/// capture measured about a second on a loaded machine; this only has to be
/// long enough never to kill a writer that was going to finish.
pub const DUMP_WRITER_TIMEOUT: Duration = Duration::from_secs(30);

/// How often [`run_dump_writer`] checks whether the writer has exited.
/// std's `Child` has no timed wait, so the wait is a poll; this is short
/// next to a capture's second and long enough to cost nothing.
const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Why one run of the dump writer did not produce a dump.
#[derive(Debug)]
pub enum DumpWriterError {
    /// The writer could not be started, or its log file could not be made.
    Start {
        /// The writer's file name.
        program: String,
        /// The underlying failure.
        error: io::Error,
    },
    /// C#'s `TimeoutException`: the writer had not exited within the timeout
    /// and was killed.
    TimedOut {
        /// The writer's file name.
        program: String,
        /// The killed writer's process id.
        pid: u32,
        /// The timeout it overran.
        timeout: Duration,
        /// Whatever it logged before it was killed.
        log: String,
    },
    /// The writer exited unsuccessfully.
    Failed {
        /// The writer's file name.
        program: String,
        /// How it exited.
        status: ExitStatus,
        /// Its log.
        log: String,
    },
}

impl fmt::Display for DumpWriterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DumpWriterError::Start { program, error } => {
                write!(f, "{program} could not be started: {error}")
            }
            DumpWriterError::TimedOut {
                program,
                pid,
                timeout,
                log,
            } => {
                let message = format!(
                    "{program} (pid {pid}) did not exit within {} s and was killed. {log}",
                    format_seconds(*timeout)
                );
                f.write_str(message.trim())
            }
            DumpWriterError::Failed {
                program,
                status,
                log,
            } => {
                let code = status
                    .code()
                    .map_or_else(|| status.to_string(), |code| code.to_string());
                write!(f, "{program} exited with {code}: {}", log.trim())
            }
        }
    }
}

impl std::error::Error for DumpWriterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DumpWriterError::Start { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// C#'s `{0:0.#}` of a number of seconds: at most one decimal, none when it
/// would be zero.
fn format_seconds(duration: Duration) -> String {
    let tenths = (duration.as_secs_f64() * 10.0).round() as u64;
    if tenths.is_multiple_of(10) {
        format!("{}", tenths / 10)
    } else {
        format!("{}.{}", tenths / 10, tenths % 10)
    }
}

/// Runs a dump writer to completion and returns everything it wrote to its
/// stdout and stderr, which go to the file `log_path` (deleted afterwards)
/// rather than to a pipe or the console. Kills it and returns
/// [`DumpWriterError::TimedOut`] if it has not exited within `timeout`;
/// returns [`DumpWriterError::Failed`] if it exits unsuccessfully.
///
/// `log_path` belongs to this call: a writer that keeps a log file of its
/// own must not be pointed at it. Only the writer itself is killed at the
/// timeout (C# killed its process tree); a writer is one process.
pub fn run_dump_writer<I, S>(
    executable: &Path,
    arguments: I,
    log_path: &Path,
    timeout: Duration,
) -> Result<String, DumpWriterError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let program = executable.file_name().map_or_else(
        || executable.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let arguments: Vec<OsString> = arguments.into_iter().map(Into::into).collect();

    let result = run_to_exit(executable, &program, &arguments, log_path, timeout);

    // A leaked log file is not worth failing a diagnostic over; the OS will
    // clear it.
    let _ = fs::remove_file(log_path);

    result
}

fn run_to_exit(
    executable: &Path,
    program: &str,
    arguments: &[OsString],
    log_path: &Path,
    timeout: Duration,
) -> Result<String, DumpWriterError> {
    let start_error = |error: io::Error| DumpWriterError::Start {
        program: program.to_string(),
        error,
    };

    // Both streams append to one fresh file, so their lines interleave as
    // written instead of overwriting each other (a stale file of the same
    // name would otherwise be read back as this writer's log).
    let _ = fs::remove_file(log_path);
    let stdout_log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(start_error)?;
    let stderr_log = stdout_log.try_clone().map_err(start_error)?;

    // The command owns the log handles; it is dropped as soon as the writer
    // has started, so nothing here keeps the file open past the writer.
    let mut child = {
        let mut command = Command::new(executable);
        command
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_log))
            .stderr(Stdio::from(stderr_log));
        command.spawn().map_err(start_error)?
    };

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                // It may exit between the timeout and the kill; gone either way.
                let _ = child.kill();
                let _ = child.wait();
                return Err(DumpWriterError::TimedOut {
                    program: program.to_string(),
                    pid: child.id(),
                    timeout,
                    log: read_log(log_path),
                });
            }
            Ok(None) => thread::sleep(EXIT_POLL_INTERVAL),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(start_error(error));
            }
        }
    };

    let log = read_log(log_path);
    if !status.success() {
        return Err(DumpWriterError::Failed {
            program: program.to_string(),
            status,
            log,
        });
    }
    Ok(log)
}

/// The log as text, or empty when it cannot be read: the log is a
/// diagnostic about a diagnostic and never worth failing over.
fn read_log(log_path: &Path) -> String {
    match fs::read(log_path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => String::new(),
    }
}

/// A dump writer's command line: `executable`, then `leading_arguments`,
/// then `--name <dump path> <pid>` (createdump's spelling). The writer
/// writes a dump of process `<pid>` to `<dump path>`, logs to its stdout and
/// stderr, and exits with status 0 once the dump is complete.
#[derive(Clone, Debug)]
pub struct DumpWriterProgram {
    /// The writer to run.
    pub executable: PathBuf,
    /// Arguments before `--name` (a mode switch, or a re-invoked test
    /// binary's selector).
    pub leading_arguments: Vec<OsString>,
}

impl DumpWriterProgram {
    /// A writer run with no leading arguments.
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        DumpWriterProgram {
            executable: executable.into(),
            leading_arguments: Vec::new(),
        }
    }

    /// Adds `argument` before `--name`.
    pub fn with_argument(mut self, argument: impl Into<OsString>) -> Self {
        self.leading_arguments.push(argument.into());
        self
    }
}

/// Runs `writer` against this process, writing its dump to `dump_path`, and
/// returns the writer's log (its stdout and stderr, captured through
/// `<dump_path>.log`, which is deleted afterwards). C#'s
/// `WriteDumpOfThisProcess` with the writer chosen by the caller.
pub fn write_dump_with(
    writer: &DumpWriterProgram,
    dump_path: &Path,
) -> Result<String, DumpWriterError> {
    let mut log_path = dump_path.as_os_str().to_owned();
    log_path.push(".log");

    let mut arguments = writer.leading_arguments.clone();
    arguments.push("--name".into());
    arguments.push(dump_path.as_os_str().to_owned());
    arguments.push(std::process::id().to_string().into());

    run_dump_writer(
        &writer.executable,
        arguments,
        Path::new(&log_path),
        DUMP_WRITER_TIMEOUT,
    )
}
