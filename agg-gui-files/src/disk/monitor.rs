//! The directory watch seam: the monitor and watch traits, with a process-wide registration
//! ([`set_directory_monitor`]). An app that can watch registers a monitor at startup; with none
//! registered (the default, and the browser) nothing is watched and a folder never
//! live-refreshes, so every write path relists explicitly. A [`super::DiskFolder`] asks for a
//! watch while it is the showing folder (`live_refresh`).
//!
//! [`PollingDirectoryMonitor`] is the desktop monitor. Pure Rust has no operating-system change
//! notification without a `-sys` crate, so each watch is a thread ([`PollingWatch`]) that lists
//! the directory's immediate level every [`POLL_INTERVAL`] and reports when the listing (names,
//! kinds, sizes, modification times) differs from the last one: a file or folder created,
//! deleted, renamed or written to all show up as one "changed". Dropping the watch stops its
//! thread. In the browser it watches nothing (no threads, no disk).
//!
//! Asking for a watch never touches the disk: the first listing, the baseline later listings are
//! compared with, is taken on the watch's own thread, because a folder on a network share that
//! is away can take tens of seconds to answer and the caller is usually the UI thread. A folder
//! that cannot be read is watched all the same: being unreadable is one more state of its
//! listing, so the watch reports when the folder goes away and when it can be read again,
//! whether it was away from the start or went away later. A folder that is readable from the
//! start does not report its first listing.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
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
    /// or folder in it is created, deleted, renamed or written to. Returns at once: a monitor
    /// does its disk work on its own thread, so a folder that is slow to answer never holds up
    /// the caller. `None` when the directory cannot be watched, an ordinary answer rather than a
    /// failure. Never panics.
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

impl PollingDirectoryMonitor {
    /// [`DirectoryMonitor::watch`] as the concrete [`PollingWatch`], which can also say when its
    /// first listing has been taken. Returns at once without reading the disk; a directory that
    /// is missing or unreadable is still watched and reports once it can be read. `None` only
    /// where nothing can be watched: in the browser, or when the operating system refuses the
    /// watch its thread.
    pub fn watch_directory(
        &self,
        directory_path: &Path,
        directory_changed: DirectoryChanged,
    ) -> Option<PollingWatch> {
        // The browser has no threads to poll from and no disk to poll.
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        let path: PathBuf = directory_path.to_path_buf();
        PollingWatch::start(move || snapshot(&path), directory_changed)
    }
}

impl DirectoryMonitor for PollingDirectoryMonitor {
    /// `Some` for every directory, readable or not, except where
    /// [`PollingDirectoryMonitor::watch_directory`] answers `None`.
    fn watch(
        &self,
        directory_path: &Path,
        directory_changed: DirectoryChanged,
    ) -> Option<Box<dyn DirectoryWatch>> {
        let watch = self.watch_directory(directory_path, directory_changed)?;
        Some(Box::new(watch))
    }
}

struct WatchShared {
    stopped: AtomicBool,
    enabled: AtomicBool,
    /// Set when reports are turned back on: the next listing is taken as the new baseline, so
    /// the owner's own changes made while reports were off are not reported back to it.
    resnapshot: AtomicBool,
    /// Whether the first listing has been taken, with the condition its waiters sleep on.
    first_listing: Mutex<bool>,
    first_listing_taken: Condvar,
}

/// A watch that lists something on its own thread every [`POLL_INTERVAL`] and reports when the
/// listing differs from the last one. [`PollingDirectoryMonitor`] lists a directory with it;
/// [`PollingWatch::start`] polls any listing. Dropping it stops the thread.
pub struct PollingWatch {
    shared: Arc<WatchShared>,
}

impl PollingWatch {
    /// Starts a thread that calls `list` at once and then every [`POLL_INTERVAL`], calling
    /// `changed` when a listing differs from the one before it. `None` from `list` means "cannot
    /// be read now", a state of its own: a change from or to it reports like any other. The
    /// first listing is the baseline and reports nothing, so when it is unreadable the first
    /// readable one reports. Returns at once; `None` only when the thread cannot be started.
    pub fn start<T, L>(list: L, changed: DirectoryChanged) -> Option<PollingWatch>
    where
        T: PartialEq + 'static,
        L: FnMut() -> Option<T> + Send + 'static,
    {
        let shared = Arc::new(WatchShared {
            stopped: AtomicBool::new(false),
            enabled: AtomicBool::new(true),
            resnapshot: AtomicBool::new(false),
            first_listing: Mutex::new(false),
            first_listing_taken: Condvar::new(),
        });
        let thread_shared = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("directory-watch".to_string())
            .spawn(move || poll(list, thread_shared, changed));
        // A watcher that cannot start is not an error: the listing just does not live-refresh.
        spawned.ok()?;
        Some(PollingWatch { shared })
    }

    /// Whether the first listing, the baseline later changes are measured from, has been taken.
    /// A change made before it is part of the baseline and is not reported.
    pub fn has_first_listing(&self) -> bool {
        *self
            .shared
            .first_listing
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    /// Waits up to `timeout` for the first listing; true once it has been taken. For a caller,
    /// off the UI thread, that is about to change the folder itself and wants the change
    /// reported.
    pub fn wait_for_first_listing(&self, timeout: Duration) -> bool {
        let taken = self
            .shared
            .first_listing
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (taken, _) = self
            .shared
            .first_listing_taken
            .wait_timeout_while(taken, timeout, |taken| !*taken)
            .unwrap_or_else(|p| p.into_inner());
        *taken
    }
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

/// The watch's thread: the first listing as the baseline, then one listing per interval. A
/// listing can take as long as the disk does (a share's timeout), so the watch may be dropped
/// while one runs: `stopped` is checked again after each listing, and a dropped watch never
/// reports.
fn poll<T, L>(mut list: L, shared: Arc<WatchShared>, changed: DirectoryChanged)
where
    T: PartialEq,
    L: FnMut() -> Option<T>,
{
    if shared.stopped.load(Ordering::SeqCst) {
        return;
    }
    let mut last = list();
    {
        let mut taken = shared
            .first_listing
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        *taken = true;
        shared.first_listing_taken.notify_all();
    }
    while !shared.stopped.load(Ordering::SeqCst) {
        std::thread::sleep(POLL_INTERVAL);
        if shared.stopped.load(Ordering::SeqCst) {
            return;
        }
        let now = list();
        if shared.stopped.load(Ordering::SeqCst) {
            return;
        }
        let rebaseline = shared.resnapshot.swap(false, Ordering::SeqCst);
        if now != last {
            last = now;
            if !rebaseline && shared.enabled.load(Ordering::SeqCst) {
                changed();
            }
        }
    }
}
