//! `agg_os::linux::fork_minidump::write_minidump_from_fork`: a real minidump
//! of this test process is written to a temp file and starts with the
//! minidump signature, also with `Duration::MAX` as the timeout (too large
//! for a deadline, so none), and a path that cannot be created gives an
//! error (before any fork) rather than a hang. Nothing is left behind. The
//! dump's contents are walked by agg-gui-automation's real-dump tests
//! (`tests/thread_stack_dump_tests/real_dumps.rs`).
//!
//! The tests that fork take `CAPTURE` first: `PR_SET_PTRACER` is one setting
//! for the whole process, so the function's callers run one capture at a
//! time, and the test harness runs tests on parallel threads.

#![cfg(all(target_os = "linux", feature = "fork-minidump"))]

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use agg_os::linux::fork_minidump::{write_minidump_from_fork, ForkDumpError};

/// The writer's timeout here: agg-gui-automation's, generous for a CI runner.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Held by each test while its child writes, one capture at a time.
static CAPTURE: Mutex<()> = Mutex::new(());

/// A temp file name no other test or run uses.
fn temp_dump_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("agg-os-{name}-{}.dmp", std::process::id()))
}

/// Writes a dump to a temp file named after `name` with `timeout`, and
/// checks that it was written and starts with the minidump signature.
fn assert_dump_is_written(name: &str, timeout: Duration) {
    let path = temp_dump_path(name);
    let result = {
        let _capture = CAPTURE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        write_minidump_from_fork(&path, timeout)
    };
    let bytes = std::fs::read(&path);
    let _ = std::fs::remove_file(&path);
    result.expect("the forked writer wrote the dump");
    let bytes = bytes.expect("the dump file is readable");
    assert!(
        bytes.starts_with(b"MDMP"),
        "the dump does not start with the minidump signature: {:?}",
        &bytes[..bytes.len().min(4)]
    );
}

#[test]
fn a_dump_of_this_process_is_written() {
    assert_dump_is_written("fork-minidump", TIMEOUT);
}

#[test]
fn a_timeout_too_large_to_represent_means_no_timeout() {
    assert_dump_is_written("fork-minidump-no-timeout", Duration::MAX);
}

#[test]
fn a_path_that_cannot_be_created_is_an_error() {
    let path = temp_dump_path("no-such-directory").join("dump.dmp");
    let error = write_minidump_from_fork(&path, TIMEOUT).expect_err("no file, no dump");
    assert!(
        matches!(error, ForkDumpError::Io(_)),
        "unexpected error: {error:?}"
    );
    assert!(!path.exists());
}
