//! Port of agg-sharp's `Tests/Agg.Tests/Agg Automation Tests/ThreadStackDumpTests.cs`
//! (feature `stack-dump`): the capture retry policy and the dump-writer
//! subprocess of `thread_stack_dump`. The stack dump is only ever exercised
//! for real on a build server, on a failure path, minutes into a hang, so if
//! it were broken there nobody would find out until the one run that needed
//! it printed an apology instead of an answer.
//!
//! Where C# ran the runtime's createdump (and `/bin/sleep`), these run the
//! controllable `agg-gui-fake-dump-writer` (`tests/support/fake_dump_writer.rs`),
//! so they run on every desktop OS. The tests that walk a real dump are the
//! next ThreadStackDump slice.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agg_gui_automation::thread_stack_dump::{
    capture_with, capture_with_retries, run_dump_writer, write_dump_with, CaptureFailure,
    DumpWalker, DumpWriterError, DumpWriterProgram, CAPTURE_ATTEMPTS, DUMP_WRITER_TIMEOUT,
};

fn fake_writer() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agg-gui-fake-dump-writer"))
}

fn temp_path(extension: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let count = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    std::env::temp_dir().join(format!(
        "agg-threadstacks-test-{}-{nanos:x}-{count}.{extension}",
        std::process::id()
    ))
}

/// Whether process `pid` still exists, asked of the OS's own tool so the
/// answer does not come from the code under test.
fn process_is_running(pid: u32) -> bool {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .expect("run tasklist");
        String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("ps")
            .args(["-p", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("run ps")
            .success()
    }
}

/// The dump is taken of a process that is still running, so the thread list
/// can change underneath it. The loss lives in the captured file, so a
/// re-capture is the only thing that can clear it, and this test is what
/// says the second capture actually happens.
#[test]
fn a_capture_that_loses_the_dump_race_is_retaken_not_reparsed() {
    let mut attempts = 0;
    let mut waits = 0;

    let report = capture_with_retries(
        "proving a lost dump race is retried",
        || {
            attempts += 1;
            if attempts == 1 {
                // The real shape of the failure this guards against.
                return Err(CaptureFailure::from(
                    "An item with the same key has already been added. Key: 624794",
                ));
            }
            Ok("second capture\n".to_string())
        },
        || waits += 1,
    )
    .expect("the second capture succeeds");

    assert_eq!(attempts, 2);
    assert_eq!(waits, 1);
    assert!(report.contains("second capture"), "{report}");

    // The report has to admit the retry, or a run that took two tries reads
    // as one that took none.
    assert!(
        report.contains(&format!(
            "attempt 1 of {CAPTURE_ATTEMPTS} failed and was re-taken"
        )),
        "{report}"
    );
    assert!(report.contains("Key: 624794"), "{report}");
}

/// The retry is bounded, and running out of attempts still fails, naming
/// the reason and every attempt's cause.
#[test]
fn a_capture_that_never_succeeds_gives_up_and_reports_every_attempt() {
    let mut attempts = 0;

    let thrown = capture_with_retries(
        "proving the retry is bounded",
        || {
            attempts += 1;
            Err(CaptureFailure::from(format!("capture {attempts} failed")))
        },
        || {},
    )
    .expect_err("every attempt fails");

    assert_eq!(attempts, CAPTURE_ATTEMPTS);
    assert_eq!(thrown.failures.len(), CAPTURE_ATTEMPTS);
    let message = thrown.to_string();
    assert!(
        message.contains("proving the retry is bounded"),
        "{message}"
    );
    assert!(
        message.contains(&format!("capture {CAPTURE_ATTEMPTS} failed")),
        "{message}"
    );
}

/// A dump writer that inherits the host's stdout prints its status lines
/// into the test run's console, where they read like a crash. The writer
/// has to hand its log back to the caller instead of printing it.
#[test]
fn writing_a_dump_captures_the_dump_writers_log_instead_of_printing_it() {
    let dump_path = temp_path("dmp");
    let writer = DumpWriterProgram::new(fake_writer()).with_argument("write");

    let result = write_dump_with(&writer, &dump_path);
    let dump_len = std::fs::metadata(&dump_path).map(|meta| meta.len());
    let _ = std::fs::remove_file(&dump_path);

    let log = result.expect("the writer succeeds");
    assert!(
        log.contains(&format!(
            "Gathering state for process {}",
            std::process::id()
        )),
        "{log}"
    );
    assert!(log.contains("Dump successfully written"), "{log}");
    assert!(dump_len.expect("the dump was written") > 0);
}

/// A writer suspends every thread of the process it dumps, including the one
/// that would drain a pipe of its output, so a writer whose output was
/// piped back here blocked for good once it had written 64 KB. A verbose
/// writer writes megabytes; it has to finish, with all of its log handed back.
#[test]
fn a_dump_writer_with_more_output_than_a_pipe_holds_still_finishes() {
    let log_path = temp_path("log");

    let log = run_dump_writer(
        &fake_writer(),
        ["flood", "1048576"],
        &log_path,
        DUMP_WRITER_TIMEOUT,
    )
    .expect("the flooding writer finishes");

    assert!(log.len() > 64 * 1024, "{} bytes of log", log.len());
    assert!(!log_path.exists());
}

/// A dump writer that never exits must not take the watchdog, and the test
/// run, down with it: it is killed at the timeout and the capture fails with
/// a reason, which the retry and the console writer handle.
#[test]
fn a_dump_writer_that_never_exits_is_killed_at_the_timeout() {
    let timer = Instant::now();

    let thrown = run_dump_writer(
        &fake_writer(),
        ["hang"],
        &temp_path("log"),
        Duration::from_millis(300),
    )
    .expect_err("the writer never exits");

    assert!(
        matches!(thrown, DumpWriterError::TimedOut { .. }),
        "{thrown:?}"
    );
    assert!(timer.elapsed() < Duration::from_secs(30));

    // The message names the writer's pid; that process must be gone, not
    // left running.
    let message = thrown.to_string();
    let pid_text = message
        .split("(pid ")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .expect("the message names the pid");
    let pid: u32 = pid_text.parse().expect("a numeric pid");
    assert!(!process_is_running(pid), "writer {pid} still running");
}

/// A writer that exits unsuccessfully fails the attempt with its exit code
/// and its log, so the retry note says why.
#[test]
fn rust_only_a_failed_dump_writer_reports_its_exit_code_and_log() {
    let log_path = temp_path("log");

    let thrown = run_dump_writer(
        &fake_writer(),
        ["fail", "3"],
        &log_path,
        DUMP_WRITER_TIMEOUT,
    )
    .expect_err("the writer fails");

    let message = thrown.to_string();
    assert!(
        matches!(thrown, DumpWriterError::Failed { .. }),
        "{thrown:?}"
    );
    assert!(message.contains("exited with 3"), "{message}");
    assert!(message.contains("failing on purpose"), "{message}");
    assert!(!log_path.exists());
}

/// Records the dump each attempt walked; fails the first walk the way a
/// dump that lost the race does.
struct RecordingWalker {
    walked: RefCell<Vec<(PathBuf, String)>>,
}

impl DumpWalker for RecordingWalker {
    fn report_from_dump(
        &self,
        reason: &str,
        dump_path: &Path,
        _write_duration: Duration,
        _capture_started: Instant,
    ) -> Result<String, CaptureFailure> {
        let contents = std::fs::read_to_string(dump_path)?;
        let mut walked = self.walked.borrow_mut();
        walked.push((dump_path.to_path_buf(), contents.clone()));
        if walked.len() == 1 {
            return Err("its register contexts do not match its stacks".into());
        }
        Ok(format!("reason: {reason}\n{contents}"))
    }
}

/// A retried capture writes a whole new dump to a new file rather than
/// re-reading the one that failed, and no dump file outlives its capture.
#[test]
fn rust_only_each_capture_attempt_walks_a_fresh_dump_that_is_then_deleted() {
    let writer = DumpWriterProgram::new(fake_writer()).with_argument("write");
    let walker = RecordingWalker {
        walked: RefCell::new(Vec::new()),
    };

    let report = capture_with("proving each attempt is fresh", &writer, &walker)
        .expect("the second attempt succeeds");

    let walked = walker.walked.borrow();
    assert_eq!(walked.len(), 2);
    assert_ne!(walked[0].0, walked[1].0);
    for (path, contents) in walked.iter() {
        assert!(contents.contains("fake dump of process"), "{contents}");
        assert!(!path.exists(), "{} was left behind", path.display());
    }
    assert!(
        report.contains("reason: proving each attempt is fresh"),
        "{report}"
    );
    assert!(
        report.contains("its register contexts do not match its stacks"),
        "{report}"
    );
}
