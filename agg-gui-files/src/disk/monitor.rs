//! The directory watch seam: the monitor and watch traits, with a process-wide registration
//! ([`set_directory_monitor`]). An app that can watch registers a monitor at startup; with none
//! registered (the default, and the browser) nothing is watched and a folder never
//! live-refreshes, so every write path relists explicitly. A [`super::DiskFolder`] asks for a
//! watch while it is the showing folder (`live_refresh`).
//!
//! [`PollingDirectoryMonitor`] is the desktop monitor. Pure Rust has no operating-system change
//! notification without a `-sys` crate, so each watch is a thread that lists the directory's
//! immediate level every [`POLL_INTERVAL`] and reports when the listing (names, kinds, sizes,
//! modification times) differs from the last one: a file or folder created, deleted, renamed or
//! written to all show up as one "changed". Dropping the watch stops its thread. In the browser
//! it watches nothing (no threads, no disk).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// The callback a watch reports through. It may run on any thread; callers marshal it.
pub type DirectoryChanged = Arc<dyn Fn() + Send + Sync>;

/// One live watch on one directory. Dropping it stops the watch for good.
pub trait DirectoryWatch {
    /// Whether changes are reported. A watch starts enabled.
    fn enabled(&self) -> bool;
    /// Turns reports off around work the owner does to the directory itself, and back on.
    fn set_enabled(&self, enabled: bool);
}

/// Something that can watch a directory for outside changes.
pub trait DirectoryMonitor: Send + Sync {
    /// Starts watching `directory_path`'s immediate level, calling `directory_changed` when a file
    /// or folder in it is created, deleted, renamed or written to. `None` when the directory
    /// cannot be watched, an ordinary answer rather than a failure. Never panics.
    fn watch(
        &self,
        directory_path: &Path,
        directory_changed: DirectoryChanged,
    ) -> Option<Box<dyn DirectoryWatch>>;
}

static MONITOR: RwLock<Option<Arc<dyn DirectoryMonitor>>> = RwLock::new(None);

/// Registers the process-wide monitor; `None` watches nothing (the default).
pub fn set_directory_monitor(monitor: Option<Arc<dyn DirectoryMonitor>>) {
    *MONITOR.write().unwrap_or_else(|p| p.into_inner()) = monitor;
}

/// The registered monitor, if any.
pub fn directory_monitor() -> Option<Arc<dyn DirectoryMonitor>> {
    MONITOR.read().unwrap_or_else(|p| p.into_inner()).clone()
}

/// How often a polling watch lists its directory.
pub const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// The desktop monitor: one polling thread per watch.
#[derive(Default)]
pub struct PollingDirectoryMonitor;

impl DirectoryMonitor for PollingDirectoryMonitor {
    fn watch(
        &self,
        directory_path: &Path,
        directory_changed: DirectoryChanged,
    ) -> Option<Box<dyn DirectoryWatch>> {
        // The browser has no threads to poll from and no disk to poll.
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        let first = snapshot(directory_path)?;
        let shared = Arc::new(WatchShared {
            stopped: AtomicBool::new(false),
            enabled: AtomicBool::new(true),
            resnapshot: AtomicBool::new(false),
        });
        let thread_shared = Arc::clone(&shared);
        let path = directory_path.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("directory-watch".to_string())
            .spawn(move || poll(path, first, thread_shared, directory_changed));
        // A watcher that cannot start is not an error: the listing just does not live-refresh.
        spawned.ok()?;
        Some(Box::new(PollingWatch { shared }))
    }
}

struct WatchShared {
    stopped: AtomicBool,
    enabled: AtomicBool,
    /// Set when reports are turned back on: the next listing is taken as the new baseline, so
    /// the owner's own changes made while reports were off are not reported back to it.
    resnapshot: AtomicBool,
}

struct PollingWatch {
    shared: Arc<WatchShared>,
}

impl DirectoryWatch for PollingWatch {
    fn enabled(&self) -> bool {
        self.shared.enabled.load(Ordering::SeqCst)
    }

    fn set_enabled(&self, enabled: bool) {
        if enabled {
            self.shared.resnapshot.store(true, Ordering::SeqCst);
        }
        self.shared.enabled.store(enabled, Ordering::SeqCst);
    }
}

impl Drop for PollingWatch {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
    }
}

/// One entry of a directory listing: what a change to it alters.
#[derive(PartialEq, Eq)]
struct Entry {
    name: std::ffi::OsString,
    is_dir: bool,
    len: u64,
    modified: Option<std::time::SystemTime>,
}

/// The directory's immediate level, sorted by name; `None` when it cannot be read.
fn snapshot(path: &Path) -> Option<Vec<Entry>> {
    let mut entries: Vec<Entry> = std::fs::read_dir(path)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| {
            let metadata = entry.metadata().ok();
            Entry {
                name: entry.file_name(),
                is_dir: metadata.as_ref().is_some_and(std::fs::Metadata::is_dir),
                len: metadata.as_ref().map_or(0, std::fs::Metadata::len),
                modified: metadata.and_then(|m| m.modified().ok()),
            }
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Some(entries)
}

fn poll(path: PathBuf, mut last: Vec<Entry>, shared: Arc<WatchShared>, changed: DirectoryChanged) {
    while !shared.stopped.load(Ordering::SeqCst) {
        std::thread::sleep(POLL_INTERVAL);
        if shared.stopped.load(Ordering::SeqCst) {
            return;
        }
        // A directory that went away reports once (its listing is now empty) and keeps watching.
        let now = snapshot(&path).unwrap_or_default();
        let rebaseline = shared.resnapshot.swap(false, Ordering::SeqCst);
        if now != last {
            last = now;
            if !rebaseline && shared.enabled.load(Ordering::SeqCst) {
                changed();
            }
        }
    }
}
