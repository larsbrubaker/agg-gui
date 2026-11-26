//! The calling thread's id as the operating system knows it (feature
//! `thread-id`).
//!
//! agg-gui-automation's stack dump keys its thread labels by this number
//! (`thread_stack_dump/registry.rs`), because it is the id a minidump records
//! for each thread.

/// The calling thread's id as a dump records it: the thread's Mach port name
/// in this task on macOS, its kernel tid on Linux, 0 elsewhere.
pub fn current_thread_os_id() -> u64 {
    os_thread_id()
}

#[cfg(target_os = "macos")]
fn os_thread_id() -> u64 {
    // SAFETY: pthread_self() is always the calling thread's valid handle, and
    // pthread_mach_thread_np only reads it; it returns the task's existing
    // port name for the thread without adding a port right to release.
    u64::from(unsafe { libc::pthread_mach_thread_np(libc::pthread_self()) })
}

#[cfg(target_os = "linux")]
fn os_thread_id() -> u64 {
    // SAFETY: gettid has no preconditions and cannot fail.
    let tid = unsafe { libc::gettid() };
    // A tid is a positive pid_t.
    u64::try_from(tid).unwrap_or(0)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn os_thread_id() -> u64 {
    0
}
