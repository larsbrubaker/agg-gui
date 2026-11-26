//! `agg_os::thread_id::current_thread_os_id`: non-zero and stable on one
//! thread, different on another, where the OS has the id (macOS, Linux);
//! 0 elsewhere.

#![cfg(feature = "thread-id")]

use agg_os::thread_id::current_thread_os_id;

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn the_id_is_non_zero_and_stable_on_one_thread() {
    let first = current_thread_os_id();
    assert_ne!(first, 0);
    assert_eq!(current_thread_os_id(), first);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn another_thread_has_another_id() {
    let here = current_thread_os_id();
    let there = std::thread::spawn(current_thread_os_id)
        .join()
        .expect("the spawned thread ran");
    assert_ne!(there, 0);
    assert_ne!(there, here);
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
#[test]
fn the_id_is_zero_where_there_is_none() {
    assert_eq!(current_thread_os_id(), 0);
}
