//! The desktop directory monitor (`PollingDirectoryMonitor`) and the process-wide registration
//! (`directory_monitor()`). Every wait is on the callback itself; the timeout is only a safety
//! net that fails the test, never the thing that paces it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use agg_gui_files::disk::{
    directory_monitor, DirectoryChanged, DirectoryMonitor, PollingDirectoryMonitor,
};

use super::temp::{hold_directory_monitor, TempDir};

const SAFETY_TIMEOUT: Duration = Duration::from_secs(30);

/// A callback that signals the returned receiver each time it is called.
fn signal() -> (DirectoryChanged, mpsc::Receiver<()>) {
    let (sender, receiver) = mpsc::channel();
    let sender = std::sync::Mutex::new(sender);
    let changed: DirectoryChanged = Arc::new(move || {
        let _ = sender.lock().unwrap().send(());
    });
    (changed, receiver)
}

fn reported(signal: &mpsc::Receiver<()>) -> bool {
    signal.recv_timeout(SAFETY_TIMEOUT).is_ok()
}

#[test]
fn creating_a_subfolder_reports() {
    let folder_dir = TempDir::new();
    let folder = folder_dir.path().to_path_buf();
    let (changed, signal) = signal();
    let watch = PollingDirectoryMonitor.watch(&folder, changed);
    assert!(watch.is_some());

    std::fs::create_dir_all(folder.join("sub")).unwrap();

    let was_reported = reported(&signal);
    drop(watch);
    assert!(was_reported);
}

#[test]
fn creating_a_file_reports() {
    let folder_dir = TempDir::new();
    let folder = folder_dir.path().to_path_buf();
    let (changed, signal) = signal();
    let watch = PollingDirectoryMonitor.watch(&folder, changed);
    assert!(watch.is_some());

    std::fs::write(folder.join("file.txt"), "hello").unwrap();

    let was_reported = reported(&signal);
    drop(watch);
    assert!(was_reported);
}

#[test]
fn disposed_watch_stays_silent() {
    let disposed_folder_dir = TempDir::new();
    let disposed_folder = disposed_folder_dir.path().to_path_buf();
    let sentinel_folder_dir = TempDir::new();
    let sentinel_folder = sentinel_folder_dir.path().to_path_buf();
    let monitor = PollingDirectoryMonitor;

    let disposed_calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&disposed_calls);
    let disposed_watch = monitor.watch(
        &disposed_folder,
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    );
    assert!(disposed_watch.is_some());
    drop(disposed_watch);

    let (changed, sentinel) = signal();
    let sentinel_watch = monitor.watch(&sentinel_folder, changed);
    assert!(sentinel_watch.is_some());

    // The change to the disposed folder goes first, so by the time the sentinel's later change
    // has been delivered, any report the disposed watch was going to make would have been too.
    std::fs::create_dir_all(disposed_folder.join("sub")).unwrap();
    std::fs::create_dir_all(sentinel_folder.join("sub")).unwrap();

    let was_reported = reported(&sentinel);
    let calls = disposed_calls.load(Ordering::SeqCst);
    drop(sentinel_watch);
    assert!(was_reported);
    assert_eq!(calls, 0);
}

#[test]
fn missing_directory_answers_null() {
    let dir = TempDir::new();
    let missing = dir.path().join("missing");

    let watch = PollingDirectoryMonitor.watch(&missing, Arc::new(|| {}));

    assert!(watch.is_none());
}

#[test]
fn default_monitor_watches_nothing() {
    let _serial = hold_directory_monitor();
    let folder_dir = TempDir::new();
    let folder = folder_dir.path().to_path_buf();
    // No monitor is registered by default, so a watch returns nothing and nothing is watched.
    let watch = directory_monitor().and_then(|monitor| monitor.watch(&folder, Arc::new(|| {})));
    let watched = watch.is_some();
    drop(watch);
    assert!(!watched);
}
