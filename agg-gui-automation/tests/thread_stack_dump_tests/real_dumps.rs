//! The half of agg-sharp's `ThreadStackDumpTests` that writes a real dump
//! of this process and walks it: the capture names the caller's frames,
//! registered threads keep their labels, a dump that cannot walk its
//! capturing thread is rejected, and the stack-pointer reader and repair
//! hold up on truncated and doctored dumps. Pulled into
//! `thread_stack_dump_tests.rs`, whose helpers it shares.
//!
//! Not in parallel with each other (C#'s `NotInParallel`): a capture reads
//! every thread of this process (on Linux it stops the process), which is
//! harmless to these tests and unkind to a timing-sensitive one beside it.

use std::fs;
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use agg_gui_automation::thread_stack_dump::{
    capture, current_os_thread_id, read_thread_stack_pointers, register_current_thread,
    report_from_dump, write_dump_of_this_process, CAPTURE_ATTEMPTS,
};

use super::temp_path;

/// One real capture at a time.
fn one_capture_at_a_time() -> MutexGuard<'static, ()> {
    static CAPTURES: Mutex<()> = Mutex::new(());
    CAPTURES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether this OS has a minidump writer; says why not when it does not, so
/// a skipped test never reads as a pass.
fn has_dump_writer(test: &str) -> bool {
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        return true;
    }
    eprintln!("{test}: skipped, there is no minidump writer on this OS (macOS and Linux only)");
    false
}

/// Threads parked until the returned sender is dropped, each having sent
/// its OS thread id first. Gives a dump threads whose stacks do not move.
fn park_threads(count: usize) -> (Vec<u64>, mpsc::Sender<()>, Vec<thread::JoinHandle<()>>) {
    let (release, released) = mpsc::channel::<()>();
    let released = std::sync::Arc::new(Mutex::new(released));
    let (started, ids) = mpsc::channel();
    let handles = (0..count)
        .map(|_| {
            let started = started.clone();
            let released = released.clone();
            thread::spawn(move || {
                started.send(current_os_thread_id()).expect("report the id");
                // Blocks until the sender is dropped.
                let _ = released.lock().map(|receiver| receiver.recv());
            })
        })
        .collect();
    let ids = (0..count)
        .map(|_| ids.recv().expect("a thread id"))
        .collect();
    (ids, release, handles)
}

/// Exists only so the assertion below has a frame name to look for that no
/// other code could produce. Inlining would erase it, which is the one thing
/// that would make this test lie; the `expect` after the call keeps it from
/// being a tail call, which would erase it too.
#[inline(never)]
fn capture_from_a_distinctively_named_frame() -> String {
    let report = capture("proving the dump helper works").expect("a capture succeeds");
    std::hint::black_box(report)
}

#[test]
fn capture_names_the_calling_threads_own_frames() {
    if !has_dump_writer("capture_names_the_calling_threads_own_frames") {
        return;
    }
    let _one = one_capture_at_a_time();

    let report = capture_from_a_distinctively_named_frame();

    // The frame below this one on the calling thread's stack. If the walker
    // works at all, this is the one line it cannot miss: it was on the stack
    // while the dump was being written.
    assert!(
        report.contains("capture_from_a_distinctively_named_frame"),
        "{report}"
    );
    assert!(report.contains("ALL THREAD STACKS"), "{report}");
    assert!(report.contains("proving the dump helper works"), "{report}");
}

/// A capture can come back whole but with the capturing thread walked to a
/// single frame or spliced with another thread's frames: its register
/// context did not match its stack. The capturing thread is the one thread
/// known to be inside `thread_stack_dump` while the dump is written, so a
/// dump that cannot walk it back there is rejected, which makes `capture`
/// take a new one instead of returning a report missing the caller.
#[test]
fn a_dump_that_cannot_walk_its_capturing_thread_is_rejected() {
    if !has_dump_writer("a_dump_that_cannot_walk_its_capturing_thread_is_rejected") {
        return;
    }
    let _one = one_capture_at_a_time();
    let (bystanders, release, handles) = park_threads(1);
    let bystander = bystanders[0];
    let dump_path = temp_path("dmp");
    let writer = current_os_thread_id();

    // A dump is sometimes written with contexts that do not match the
    // stacks (the case this check exists for) and `capture` retakes it; so
    // does this test, until it has a good dump to name the bystander in.
    let mut report = None;
    for _ in 0..CAPTURE_ATTEMPTS {
        write_dump_of_this_process(&dump_path).expect("the dump is written");
        match report_from_dump(
            "the writer is walked",
            &dump_path,
            Duration::ZERO,
            Instant::now(),
            Some(writer),
        ) {
            Ok(walked) => {
                report = Some(walked);
                break;
            }
            Err(_) => {
                let _ = fs::remove_file(&dump_path);
            }
        }
    }

    // The bystander never entered thread_stack_dump: named as the capturer,
    // it reads as a dump whose contexts are wrong for the thread that took it.
    let rejected = report_from_dump(
        "the capturer is not walked",
        &dump_path,
        Duration::ZERO,
        Instant::now(),
        Some(bystander),
    );

    drop(release);
    for handle in handles {
        handle.join().expect("the bystander exits");
    }
    let _ = fs::remove_file(&dump_path);

    let report = report.expect("one of the dumps walks its writer");
    assert!(report.contains("END THREAD STACKS"), "{report}");
    let rejected = rejected.expect_err("the bystander is not the capturer");
    assert!(
        rejected.to_string().contains(&format!("os {bystander}")),
        "{rejected}"
    );
}

#[test]
fn registered_threads_are_labelled_in_the_dump() {
    if !has_dump_writer("registered_threads_are_labelled_in_the_dump") {
        return;
    }
    let _one = one_capture_at_a_time();

    // The watchdog's whole value rests on this mapping: the dump only knows
    // thread ids, so a role registered from a live thread has to come back
    // out attached to the right one.
    let (registered, registered_id) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let thread = thread::Builder::new()
        .name("PretendUiThread".to_string())
        .spawn(move || {
            register_current_thread("<<< UI THREAD (message pump)");
            registered
                .send(current_os_thread_id())
                .expect("report the id");
            let _ = released.recv();
        })
        .expect("spawn the pretend UI thread");
    let registered_id = registered_id.recv().expect("the thread registered");

    let report = capture("proving registered threads are labelled");

    drop(release);
    thread.join().expect("the pretend UI thread exits");

    let report = report.expect("a capture succeeds");
    assert!(
        report.contains(&format!("os={registered_id} <<< UI THREAD (message pump)")),
        "{report}"
    );
    assert!(report.contains("name=\"PretendUiThread\""), "{report}");
}

/// A dump cut off inside a thread's entry is left for the walker to judge:
/// reading the stack pointers stops at the truncated entry instead of
/// reading past the end of the file.
#[test]
fn a_truncated_thread_command_is_not_read() {
    let mut dump = Vec::new();
    let mut put = |value: u32| dump.extend_from_slice(&value.to_le_bytes());
    put(0x504D_444D); // MDMP
    put(0xA793); // version
    put(2); // stream count
    put(32); // stream directory RVA
    put(0); // checksum
    put(0); // time stamp
    put(0); // flags (low)
    put(0); // flags (high)
    put(7); // SystemInfoStream
    put(4); // its size
    put(56); // its RVA
    put(3); // ThreadListStream
    put(4 + 48); // its size: a count, then one 48-byte thread entry
    put(60); // its RVA
    put(12); // processor architecture ARM64, then processor level 0
    put(1); // one thread
    put(1234); // thread id
    put(0); // suspend count
    put(0); // priority class: the entry is cut off here, far short of its 48 bytes

    let threads = read_thread_stack_pointers(&dump);

    assert_eq!(threads.len(), 0);
}

/// Where in a `MINIDUMP_THREAD` its context's location descriptor sits:
/// the minidump format, restated so the test does not lean on the code it
/// tests.
const THREAD_CONTEXT_LOCATION: usize = 40;

fn context_range(dump: &[u8], entry_offset: u64) -> std::ops::Range<usize> {
    let at = entry_offset as usize + THREAD_CONTEXT_LOCATION;
    let size = u32::from_le_bytes(dump[at..at + 4].try_into().expect("4 bytes")) as usize;
    let rva = u32::from_le_bytes(dump[at + 4..at + 8].try_into().expect("4 bytes")) as usize;
    rva..rva + size
}

/// Overwrites the register context of the thread at `entry_offset` with
/// zeros: what a writer that could not read a thread's state writes.
fn zero_register_state(dump: &mut [u8], entry_offset: u64) {
    let range = context_range(dump, entry_offset);
    dump[range].fill(0);
}

/// Every 8-byte slot of the thread's context that holds the stack pointer's
/// value, found by value so the test does not restate the production code's
/// register layout. All of them, not the first: another register can hold
/// the same value (on ARM64 fp, before sp in the context, often equals sp).
fn stack_pointer_value_positions(dump: &[u8], entry_offset: u64, stack_pointer: u64) -> Vec<usize> {
    let range = context_range(dump, entry_offset);
    let positions: Vec<usize> = (range.start..range.end.saturating_sub(7))
        .step_by(8)
        .filter(|&at| {
            u64::from_le_bytes(dump[at..at + 8].try_into().expect("8 bytes")) == stack_pointer
        })
        .collect();
    assert!(
        !positions.is_empty(),
        "stack pointer not found in its own thread context"
    );
    positions
}

/// Two thread contexts on one stack pointer cannot both be their threads'
/// registers, and C#'s reader rejected such a dump outright. This builds
/// that dump on purpose, from a real one, and requires a report from it
/// anyway, with the repeats dropped and counted.
#[test]
fn a_dump_with_two_threads_on_one_stack_pointer_still_reports() {
    if !has_dump_writer("a_dump_with_two_threads_on_one_stack_pointer_still_reports") {
        return;
    }
    let _one = one_capture_at_a_time();
    // A .NET process always has runtime threads to doctor; a Rust test
    // process may have only two, so park four with stacks that stay put.
    let (_ids, release, handles) = park_threads(4);
    let dump_path = temp_path("dmp");

    let result = doctor_and_report(&dump_path);

    drop(release);
    for handle in handles {
        handle.join().expect("a parked thread exits");
    }
    let _ = fs::remove_file(&dump_path);

    let (natural_repeats, report) = result;
    assert!(report.contains("END THREAD STACKS"), "{report}");
    assert!(
        report.contains(&format!(
            "({} thread register context(s) repeated an earlier thread's stack pointer",
            natural_repeats + 1
        )),
        "{report}"
    );
    assert!(report.contains("--- thread os="), "{report}");
}

fn doctor_and_report(dump_path: &Path) -> (usize, String) {
    write_dump_of_this_process(dump_path).expect("the dump is written");
    let mut dump = fs::read(dump_path).expect("read the dump");
    let threads = read_thread_stack_pointers(&dump);
    assert!(threads.len() > 3, "{} threads", threads.len());

    // Make the zeroed-context state certain rather than luck: zero the last
    // two threads' register contexts, so every run has repeats before this
    // test adds its own.
    zero_register_state(&mut dump, threads[threads.len() - 1].entry_offset);
    zero_register_state(&mut dump, threads[threads.len() - 2].entry_offset);
    let threads = read_thread_stack_pointers(&dump);
    let distinct: std::collections::HashSet<u64> =
        threads.iter().map(|thread| thread.stack_pointer).collect();
    let natural_repeats = threads.len() - distinct.len();
    assert!(natural_repeats >= 1);

    // Give a thread the first one's stack pointer, in place. The target has
    // to own a real stack pointer no other context shares, so the rewrite
    // adds exactly one repeat.
    let source = threads[0];
    let target_index = (1..threads.len())
        .find(|&index| {
            let stack_pointer = threads[index].stack_pointer;
            stack_pointer != 0
                && stack_pointer != source.stack_pointer
                && threads
                    .iter()
                    .filter(|other| other.stack_pointer == stack_pointer)
                    .count()
                    == 1
        })
        .expect("a thread with a stack pointer of its own");
    let target = threads[target_index];
    for at in stack_pointer_value_positions(&dump, target.entry_offset, target.stack_pointer) {
        dump[at..at + 8].copy_from_slice(&source.stack_pointer.to_le_bytes());
    }

    // The production reader must now see the repeat, or the rest of this
    // test proves nothing.
    let rewritten = read_thread_stack_pointers(&dump);
    assert_eq!(rewritten[target_index].stack_pointer, source.stack_pointer);
    fs::write(dump_path, &dump).expect("write the doctored dump");

    let report = report_from_dump(
        "proving a repeated stack pointer is survivable",
        dump_path,
        Duration::ZERO,
        Instant::now(),
        None,
    )
    .expect("the doctored dump still reports");
    (natural_repeats, report)
}
