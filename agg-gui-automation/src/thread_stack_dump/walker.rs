//! Walks a minidump of this process into the all-thread report: the port of
//! agg-sharp's `ThreadStackDump.ReportFromDump`, `AppendRuntimeThreads` and
//! `DescribeUnregisteredThread`, with rust-minidump's processor in place of
//! ClrMD.
//!
//! The dump is of *this* process and is walked while the process still runs,
//! so every module it names is still loaded at the address it recorded. That
//! is what lets frames be named without any symbol files: each instruction
//! address is resolved in-process with `backtrace` (the binaries' own debug
//! info and symbol tables). Unwinding uses each module's call frame
//! information read from its file on disk (`DebugInfoSymbolProvider`), and
//! falls back to frame pointers and stack scanning where a module has none
//! (macOS system libraries live in the shared cache, not on disk).

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use minidump::{Minidump, MinidumpModuleList, MinidumpSystemInfo, Module};
use minidump_unwind::symbols::debuginfo::DebugInfoSymbolProvider;
use minidump_unwind::{
    FileError, FileKind, FillSymbolError, FrameSymbolizer, FrameWalker, StackFrame, SymbolProvider,
};

use super::registry::registered_role;
use super::stack_pointers::drop_thread_contexts_with_repeated_stack_pointers;
use super::{CaptureFailure, DumpWalker};

/// Frames past this per thread are elided. A runaway recursion would
/// otherwise bury the one stack the reader came for under tens of thousands
/// of identical lines.
pub const MAX_FRAMES_PER_THREAD: usize = 80;

/// The module whose frames the capturing thread must be walked through.
const THREAD_STACK_DUMP_MODULE: &str = super::MODULE_PATH;

/// The [`DumpWalker`] of a real capture: [`report_from_dump`], checking the
/// thread `capturing_thread` (an OS id as [`super::current_os_thread_id`]
/// returns it) when there is one.
pub struct MinidumpWalker {
    /// The thread that wrote the dump, or `None` not to check it.
    pub capturing_thread: Option<u64>,
}

impl DumpWalker for MinidumpWalker {
    fn report_from_dump(
        &self,
        reason: &str,
        dump_path: &Path,
        write_duration: Duration,
        capture_started: Instant,
    ) -> Result<String, CaptureFailure> {
        report_from_dump(
            reason,
            dump_path,
            write_duration,
            capture_started,
            self.capturing_thread,
        )
    }
}

/// Walks the already written minidump at `dump_path` into the all-thread
/// report. May modify the file: see
/// `drop_thread_contexts_with_repeated_stack_pointers`.
///
/// `capturing_thread` is the OS id of the thread that wrote the dump, or
/// `None` not to check it. That thread was certainly inside this module
/// while the dump was written, so a walk of it that never passes through a
/// `thread_stack_dump` frame to a named caller means the file's register
/// contexts do not match its stacks (another lost race with the running
/// process) and the dump is rejected, so a retry takes a new one.
pub fn report_from_dump(
    reason: &str,
    dump_path: &Path,
    write_duration: Duration,
    capture_started: Instant,
    capturing_thread: Option<u64>,
) -> Result<String, CaptureFailure> {
    let dropped_contexts = drop_thread_contexts_with_repeated_stack_pointers(dump_path)?;
    let dump_bytes = std::fs::metadata(dump_path)?.len();

    let mut report = String::new();
    report.push_str("======================= ALL THREAD STACKS =======================\n");
    report.push_str(&format!("reason: {reason}\n"));
    report.push_str(&format!(
        "process {} at {} UTC, dump {} MB written in {} ms\n",
        std::process::id(),
        utc_time_of_day(),
        dump_bytes / (1024 * 1024),
        write_duration.as_millis()
    ));
    if dropped_contexts > 0 {
        report.push_str(&format!(
            "({dropped_contexts} thread register context(s) repeated an earlier thread's stack pointer and were dropped; the thread that owns that stack pointer keeps the first one, others may show no frames)\n"
        ));
    }

    let dump = Minidump::read_path(dump_path)?;
    let system_info = dump.get_stream::<MinidumpSystemInfo>()?;
    let modules = dump.get_stream::<MinidumpModuleList>().unwrap_or_default();
    let symbols = InProcessSymbols {
        unwinder: block_on(DebugInfoSymbolProvider::new(&system_info, &modules)),
    };
    let state = block_on(minidump_processor::process_minidump(&dump, &symbols))?;

    let mut capturer_walked = false;
    for thread in &state.threads {
        let role = registered_role(u64::from(thread.thread_id))
            .unwrap_or_else(|| describe_unregistered_thread(thread.thread_name.as_deref()));
        report.push_str(&format!("\n--- thread os={} {role}\n", thread.thread_id));
        let is_capturer = capturing_thread == Some(u64::from(thread.thread_id));
        capturer_walked |= append_frames(&mut report, &thread.frames, is_capturer);
    }

    if let Some(capturer) = capturing_thread {
        if !capturer_walked {
            return Err(format!(
                "the dump did not walk the capturing thread (os {capturer}) through thread_stack_dump to a named caller; its register contexts do not match its stacks, or its frames could not be named"
            )
            .into());
        }
    }

    report.push_str(&format!(
        "===================== END THREAD STACKS ({} ms) =====================\n",
        capture_started.elapsed().as_millis()
    ));
    Ok(report)
}

/// Appends one thread's frames, innermost first, inlined calls before the
/// function they were inlined into. Returns true when `is_capturer` and the
/// walk went through a `thread_stack_dump` frame to a named caller below it.
fn append_frames(report: &mut String, frames: &[StackFrame], is_capturer: bool) -> bool {
    let mut lines = 0;
    let mut check = CapturerCheck::default();

    'frames: for frame in frames {
        // The processor keeps inlined calls deepest first.
        let names = frame
            .inlines
            .iter()
            .map(|inline| Some(format!("{} (inlined)", inline.function_name)))
            .chain(std::iter::once(frame.function_name.clone()));
        for name in names {
            if lines == MAX_FRAMES_PER_THREAD {
                report.push_str(&format!(
                    "    ... more than {MAX_FRAMES_PER_THREAD} frames, rest elided\n"
                ));
                break 'frames;
            }
            lines += 1;
            match &name {
                Some(name) => report.push_str(&format!("    {name}\n")),
                None => report.push_str(&format!(
                    "    [{} frame, ip 0x{:x}]\n",
                    frame.module.as_ref().map_or_else(
                        || "unknown module".to_string(),
                        |module| module.code_file().into_owned()
                    ),
                    frame.instruction
                )),
            }
            if is_capturer {
                check.visit(name.as_deref());
            }
        }
    }

    if lines == 0 {
        report.push_str("    (no frames - the thread's register context could not be walked)\n");
    }
    check.walked
}

/// The capturing thread is known to be running its caller, then
/// `thread_stack_dump`, then the dump writer, so its walk must name every
/// frame from the first `thread_stack_dump` frame down to that caller. A
/// report that reaches this module but cannot name the caller is the failure
/// this exists to prevent, so it too counts as a lost race.
#[derive(Default)]
struct CapturerCheck {
    passed_entry_point: bool,
    unresolved: bool,
    walked: bool,
}

impl CapturerCheck {
    fn visit(&mut self, name: Option<&str>) {
        if self.walked || self.unresolved {
            return;
        }
        match name {
            None if self.passed_entry_point => self.unresolved = true,
            None => {}
            Some(name) if is_thread_stack_dump_frame(name) => self.passed_entry_point = true,
            // The first named frame below this module that is not the
            // standard library's call plumbing: the caller.
            Some(name) if self.passed_entry_point && !is_runtime_frame(name) => self.walked = true,
            Some(_) => {}
        }
    }
}

fn is_thread_stack_dump_frame(name: &str) -> bool {
    let name = name.trim_start_matches('<');
    name.starts_with(THREAD_STACK_DUMP_MODULE)
}

/// Closure-call shims and other standard library frames between this
/// module's frames and its caller.
fn is_runtime_frame(name: &str) -> bool {
    let name = name.trim_start_matches('<');
    ["core::", "std::", "alloc::"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || name.contains(" as core::ops::function::")
}

/// Best available description of a thread that never called
/// `register_current_thread`: the name the OS kept for it, if any.
fn describe_unregistered_thread(dump_name: Option<&str>) -> String {
    match dump_name {
        Some(name) if !name.is_empty() => format!("(unregistered) name=\"{name}\""),
        _ => "(unregistered) name=(unknown - not registered)".to_string(),
    }
}

/// `HH:MM:SS.fff` of now, in UTC (std has no local time zone).
fn utc_time_of_day() -> String {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = since_epoch.as_secs() % 86_400;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        since_epoch.subsec_millis()
    )
}

/// Runs a future that never waits on anything external to completion. The
/// processor's API is async so a symbol server can be awaited; every symbol
/// and unwind lookup here is synchronous, so the first poll finishes.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::yield_now();
    }
}

/// Names frames in-process and unwinds with each module's CFI.
struct InProcessSymbols {
    unwinder: DebugInfoSymbolProvider,
}

#[async_trait::async_trait]
impl SymbolProvider for InProcessSymbols {
    async fn fill_symbol(
        &self,
        _module: &(dyn Module + Sync),
        frame: &mut (dyn FrameSymbolizer + Send),
    ) -> Result<(), FillSymbolError> {
        // Innermost (inlined) first, the function that contains them last.
        let mut symbols: Vec<(String, u64)> = Vec::new();
        let address = frame.get_instruction() as usize as *mut c_void;
        backtrace::resolve(address, |symbol| {
            if let Some(name) = symbol.name() {
                let base = symbol.addr().map_or(0, |addr| addr as usize as u64);
                symbols.push((format!("{name:#}"), base));
            }
        });
        let Some((function, base)) = symbols.pop() else {
            // Also what tells the stack scanner an address is no code.
            return Err(FillSymbolError {});
        };
        frame.set_function(&function, base, 0);
        // The processor takes inlined calls outermost first.
        for (inlined, _) in symbols.iter().rev() {
            frame.add_inline_frame(inlined, None, None);
        }
        Ok(())
    }

    async fn walk_frame(
        &self,
        module: &(dyn Module + Sync),
        walker: &mut (dyn FrameWalker + Send),
    ) -> Option<()> {
        self.unwinder.walk_frame(module, walker).await
    }

    async fn get_file_path(
        &self,
        _module: &(dyn Module + Sync),
        _file_kind: FileKind,
    ) -> Result<PathBuf, FileError> {
        Err(FileError::NotFound)
    }
}
