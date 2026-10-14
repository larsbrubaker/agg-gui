//! Each dumped thread's stack pointer, and the repair of a dump whose thread
//! contexts repeat one: the port of agg-sharp's
//! `ThreadStackDump.ReadThreadStackPointers` and
//! `DropThreadContextsWithRepeatedStackPointers`.
//!
//! C# read the Mach-O core createdump writes; the dumps here are minidumps
//! (`write_minidump.rs`), so this reads a minidump's thread list. The
//! reasoning carries over unchanged: at most one of two register contexts
//! with the same stack pointer can be its thread's real registers (the usual
//! source is a writer that could not read a thread's state and wrote zeros,
//! and two such threads share stack pointer 0), and walking the other would
//! print one thread's frames under another thread's label. One thread with
//! no frames beats a report that misattributes a stack, so every context
//! after the first with a given stack pointer is dropped before the walk.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::Path;

/// `MDMP`, little-endian.
const MINIDUMP_SIGNATURE: u32 = 0x504d_444d;
const THREAD_LIST_STREAM: u32 = 3;
const SYSTEM_INFO_STREAM: u32 = 7;
const PROCESSOR_ARCHITECTURE_AMD64: u16 = 9;
const PROCESSOR_ARCHITECTURE_ARM64: u16 = 12;
/// Breakpad's pre-Microsoft ARM64 code, which `minidump-writer` writes.
const PROCESSOR_ARCHITECTURE_ARM64_OLD: u16 = 0x8003;
/// `MINIDUMP_THREAD`: id, suspend count, priority class, priority, TEB,
/// the stack's memory descriptor, then the context's location descriptor.
const THREAD_ENTRY_SIZE: u64 = 48;
/// Where in a `MINIDUMP_THREAD` its context's `MINIDUMP_LOCATION_DESCRIPTOR`
/// (data size, then RVA) sits.
const THREAD_CONTEXT_LOCATION: u64 = 40;

/// One thread of a dump's thread list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadStackPointer {
    /// File offset of the thread's `MINIDUMP_THREAD` entry.
    pub entry_offset: u64,
    /// The stack pointer its register context holds; 0 when it has no
    /// readable context of the dump's CPU.
    pub stack_pointer: u64,
}

fn read_u16(dump: &[u8], offset: u64) -> Option<u16> {
    let start = usize::try_from(offset).ok()?;
    let bytes = dump.get(start..start.checked_add(2)?)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(dump: &[u8], offset: u64) -> Option<u32> {
    let start = usize::try_from(offset).ok()?;
    let bytes = dump.get(start..start.checked_add(4)?)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn read_u64(dump: &[u8], offset: u64) -> Option<u64> {
    let start = usize::try_from(offset).ok()?;
    let bytes = dump.get(start..start.checked_add(8)?)?;
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}

/// The RVA of the first stream of `stream_type`, from the stream directory.
fn find_stream(dump: &[u8], stream_type: u32) -> Option<u64> {
    let count = read_u32(dump, 8)?;
    let directory = u64::from(read_u32(dump, 12)?);
    (0..u64::from(count)).find_map(|index| {
        let entry = directory + index * 12;
        (read_u32(dump, entry)? == stream_type).then(|| read_u32(dump, entry + 8).map(u64::from))?
    })
}

/// Offset of the stack pointer inside a register context of the dump's CPU,
/// or `None` for a CPU this does not read.
fn stack_pointer_offset(dump: &[u8]) -> Option<u64> {
    let system_info = find_stream(dump, SYSTEM_INFO_STREAM)?;
    match read_u16(dump, system_info)? {
        // CONTEXT_AMD64: six home slots, flags and MxCsr, six segment
        // registers, EFlags, six debug registers, rax, rcx, rdx, rbx, then rsp.
        PROCESSOR_ARCHITECTURE_AMD64 => Some(0x98),
        // CONTEXT_ARM64 (u32 flags and cpsr) and the old layout (u64 flags)
        // both put x0-x28, fp and lr in the next 31 slots, then sp.
        PROCESSOR_ARCHITECTURE_ARM64 | PROCESSOR_ARCHITECTURE_ARM64_OLD => Some(8 + 31 * 8),
        _ => None,
    }
}

/// Each thread of the minidump `dump` with the stack pointer the walker will
/// unwind it from. Empty for anything that is not a minidump of a CPU this
/// reads. A thread whose context is too small or lies past the end of the
/// file reads as stack pointer 0, as C# read a context of the wrong flavor.
///
/// A thread list cut off inside an entry stops at that entry instead of
/// reading past the end of the file: the dump is left for the walker to
/// judge.
pub fn read_thread_stack_pointers(dump: &[u8]) -> Vec<ThreadStackPointer> {
    let mut threads = Vec::new();
    if read_u32(dump, 0) != Some(MINIDUMP_SIGNATURE) {
        return threads;
    }
    let (Some(stack_pointer_offset), Some(thread_list)) = (
        stack_pointer_offset(dump),
        find_stream(dump, THREAD_LIST_STREAM),
    ) else {
        return threads;
    };
    let Some(count) = read_u32(dump, thread_list) else {
        return threads;
    };

    let file_length = dump.len() as u64;
    for index in 0..u64::from(count) {
        let entry_offset = thread_list + 4 + index * THREAD_ENTRY_SIZE;
        if entry_offset + THREAD_ENTRY_SIZE > file_length {
            // Truncated: the entry runs past the end of the file.
            break;
        }
        let context_size = read_u32(dump, entry_offset + THREAD_CONTEXT_LOCATION).unwrap_or(0);
        let context_rva = read_u32(dump, entry_offset + THREAD_CONTEXT_LOCATION + 4).unwrap_or(0);
        let stack_pointer = if u64::from(context_size) >= stack_pointer_offset + 8 {
            read_u64(dump, u64::from(context_rva) + stack_pointer_offset).unwrap_or(0)
        } else {
            0
        };
        threads.push(ThreadStackPointer {
            entry_offset,
            stack_pointer,
        });
    }
    threads
}

/// Drops the register context of every thread after the first whose stack
/// pointer an earlier thread already had, by clearing the context's location
/// in its thread entry (the walker then shows that thread with no frames).
/// Returns how many were dropped; does nothing to a file that is not a
/// minidump this reads.
pub fn drop_thread_contexts_with_repeated_stack_pointers(dump_path: &Path) -> io::Result<usize> {
    let dump = std::fs::read(dump_path)?;
    let mut seen = HashSet::new();
    let repeated: Vec<u64> = read_thread_stack_pointers(&dump)
        .into_iter()
        .filter(|thread| !seen.insert(thread.stack_pointer))
        .map(|thread| thread.entry_offset)
        .collect();
    if repeated.is_empty() {
        return Ok(0);
    }

    let mut file = OpenOptions::new().write(true).open(dump_path)?;
    for entry_offset in &repeated {
        file.seek(SeekFrom::Start(entry_offset + THREAD_CONTEXT_LOCATION))?;
        // An empty location: data size 0 at RVA 0.
        file.write_all(&[0; 8])?;
    }
    file.flush()?;
    Ok(repeated.len())
}
