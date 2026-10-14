//! Thread labels for the stack dump: the port of agg-sharp's
//! `ThreadStackDump.RegisterCurrentThread` and its `threadRoles` table.
//!
//! A dump knows each thread only by its OS id (and, where the OS keeps one,
//! a name), and there is no way to enumerate other threads' `Thread` handles
//! from inside the process, so anything that wants to be recognisable in the
//! dump has to say so from its own thread while it is still running. The
//! walker (`walker.rs`) looks each dumped thread's id up here.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// What each registered thread is *for*, keyed by [`current_os_thread_id`].
fn thread_roles() -> &'static Mutex<HashMap<u64, String>> {
    static ROLES: OnceLock<Mutex<HashMap<u64, String>>> = OnceLock::new();
    ROLES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Labels the calling thread so it can be picked out of a later stack dump
/// by role rather than by id. `role` is a short description of what the
/// thread does, e.g. `"UI THREAD (message pump)"`.
///
/// C# also recorded `threadPool=` and `background=`; a Rust thread has
/// neither, so the label is the role and the thread's name.
pub fn register_current_thread(role: &str) {
    let current = std::thread::current();
    let name = current.name().unwrap_or("(unnamed)");
    let label = format!("{role} name=\"{name}\"");
    if let Ok(mut roles) = thread_roles().lock() {
        roles.insert(current_os_thread_id(), label);
    }
}

/// The label [`register_current_thread`] recorded for the thread `os_id`.
pub(super) fn registered_role(os_id: u64) -> Option<String> {
    thread_roles()
        .lock()
        .ok()
        .and_then(|roles| roles.get(&os_id).cloned())
}

/// The calling thread's id as a dump records it (the `os=` of the report):
/// the thread's Mach port name in this task on macOS, its kernel tid on
/// Linux, 0 where there is no dump writer.
pub fn current_os_thread_id() -> u64 {
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
