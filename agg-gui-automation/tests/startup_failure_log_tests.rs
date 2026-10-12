//! Rust-only tests of `StartupFailureLog` (agg-gui-automation
//! `src/startup_failure_log.rs`, C# `StartupFailureLog`) and of
//! `widget_not_found_message` carrying what it recorded. The log is
//! process-wide, so these live in their own test binary and take turns.

use std::io::Write;
use std::sync::{Arc, Mutex, MutexGuard};

use agg_gui_automation::startup_failure_log::NEVER_STARTED;
use agg_gui_automation::{AutomationRunner, StartupFailureLog};

static TURN: Mutex<()> = Mutex::new(());

/// Take this binary's turn at the log, with a fresh log whose immediate
/// writes land in the returned buffer.
fn fresh_log() -> (MutexGuard<'static, ()>, Arc<Mutex<Vec<u8>>>) {
    let turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    StartupFailureLog::reset();
    let written = Arc::new(Mutex::new(Vec::new()));
    StartupFailureLog::set_immediate_sink(Some(Box::new(SharedSink(Arc::clone(&written)))));
    (turn, written)
}

struct SharedSink(Arc<Mutex<Vec<u8>>>);

impl Write for SharedSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct BootError(&'static str, Option<Box<BootError>>);

impl std::fmt::Display for BootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for BootError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.1.as_deref().map(|e| e as _)
    }
}

#[test]
fn rust_only_nothing_recorded_leaves_the_message_unchanged() {
    let (_turn, written) = fresh_log();
    assert_eq!(StartupFailureLog::last_failure_description(), None);
    assert_eq!(
        StartupFailureLog::append_to("ClickByName Failed"),
        "ClickByName Failed"
    );
    assert_eq!(
        AutomationRunner::widget_not_found_message("ClickByName", "Save Button"),
        "ClickByName Failed: Named GuiWidget not found [Save Button]"
    );
    assert!(written.lock().unwrap().is_empty());
}

#[test]
fn rust_only_a_recorded_failure_is_described_written_at_once_and_appended() {
    let (_turn, written) = fresh_log();
    let error = BootError(
        "settings unreadable",
        Some(Box::new(BootError("disk gone", None))),
    );
    StartupFailureLog::record("Startup", &error);

    let description = StartupFailureLog::last_failure_description().expect("recorded");
    assert!(
        description.starts_with("Application Startup failed at "),
        "{description}"
    );
    assert!(description.contains(" on thread "), "{description}");
    assert!(
        description.contains(":\nsettings unreadable\n ---> disk gone\n"),
        "{description}"
    );
    assert_eq!(
        String::from_utf8(written.lock().unwrap().clone()).unwrap(),
        description
    );

    assert_eq!(
        AutomationRunner::widget_not_found_message("ClickByName", "Save Button"),
        format!(
            "ClickByName Failed: Named GuiWidget not found [Save Button]\n{NEVER_STARTED}\n{description}"
        )
    );
    StartupFailureLog::reset();
}

#[test]
fn rust_only_an_empty_phase_reads_as_startup() {
    let (_turn, _written) = fresh_log();
    StartupFailureLog::record("", &BootError("boom", None));
    let description = StartupFailureLog::last_failure_description().expect("recorded");
    assert!(
        description.starts_with("Application startup failed at "),
        "{description}"
    );
    StartupFailureLog::reset();
}

#[test]
fn rust_only_a_panic_payload_is_recorded_with_its_message() {
    let (_turn, _written) = fresh_log();
    let payload = std::panic::catch_unwind(|| panic!("window build failed")).unwrap_err();
    StartupFailureLog::record_panic("Load", payload.as_ref());
    let description = StartupFailureLog::last_failure_description().expect("recorded");
    assert!(
        description.starts_with("Application Load failed at "),
        "{description}"
    );
    assert!(
        description.contains("panicked: window build failed"),
        "{description}"
    );
    StartupFailureLog::reset();
}

#[test]
fn rust_only_reset_forgets_the_failure() {
    let (_turn, _written) = fresh_log();
    StartupFailureLog::record("Startup", &BootError("boom", None));
    StartupFailureLog::reset();
    assert_eq!(StartupFailureLog::last_failure_description(), None);
    assert_eq!(StartupFailureLog::append_to("m"), "m");
}

#[test]
fn rust_only_no_immediate_sink_still_records() {
    let (_turn, _written) = fresh_log();
    StartupFailureLog::set_immediate_sink(None);
    StartupFailureLog::record("Startup", &BootError("boom", None));
    assert!(StartupFailureLog::last_failure_description().is_some());
    StartupFailureLog::reset();
}
