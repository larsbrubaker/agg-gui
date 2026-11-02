//! A folder's live refresh: its directory watch and its coalescing refresh. A
//! [`super::DiskFolder`] holds one and drives it from `activate`, `deactivate`, `dispose` and
//! `pump_live_refresh`.
//!
//! The watch (`monitor`) reports on its own thread; the report only sets a flag here, which the
//! UI thread turns into `directory_contents_changed` at its next pump (the callback may arrive on
//! any thread, so the folder marshals it itself). A change while the folder is active arms the
//! refresh interval; the folder relists once half a second has passed with no further change, so
//! a burst of changes (a copy of many files) relists once. The interval is the host's pump rather
//! than a UI-thread timer because this crate has no UI queue: the browser calls
//! `pump_live_refresh` for its showing folder each frame, while a pending refresh keeps it
//! waking. Teardown cancels a pending refresh, so a folder torn down inside the half second never
//! relists into a state nobody watches.
//!
//! A copy run as a task (`WritableFolder::copy_in`) writes through a folder of its own, so it
//! asks the folder it was started from to relist through a shared flag (`relist_handle`), which
//! the next pump answers at once.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use web_time::Instant;

use super::monitor::{directory_monitor, DirectoryWatch};

/// The half second of quiet before the relist.
const QUIET_MS: u64 = 500;

/// The milliseconds clock the interval reads.
fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    // Saturates only after 584 million years of one process.
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// The armed refresh interval. A clone still reads `active()` after the container cleared it, so
/// whoever captured the interval sees it stop.
#[derive(Clone)]
pub struct RefreshInterval {
    active: Rc<Cell<bool>>,
}

impl RefreshInterval {
    /// False once cleared.
    pub fn active(&self) -> bool {
        self.active.get()
    }

    fn clear(&self) {
        self.active.set(false);
    }
}

/// The watch, the cross-thread report flag and the pending refresh of one folder container.
#[derive(Default)]
pub(super) struct LiveRefresh {
    watch: Option<Box<dyn DirectoryWatch>>,
    /// Set by the watch's thread, taken by `take_report` on the UI thread.
    reported: Arc<AtomicBool>,
    is_active: bool,
    last_change_ms: u64,
    waiting_for_refresh: Option<RefreshInterval>,
    /// Set by a copy task writing into this folder through a folder of its own
    /// (`relist_handle`), answered by the next pump with a relist.
    relist: Rc<Cell<bool>>,
}

/// A cloned folder is a copy of the listing, not of its showing state: it starts inactive and
/// unwatched, as a freshly made one does.
impl Clone for LiveRefresh {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl LiveRefresh {
    /// Active, and watching when the folder exists and may be watched.
    pub(super) fn activate(&mut self, full_path: &Path, should_monitor: bool) {
        self.is_active = true;
        if self.watch.is_some() || !full_path.is_dir() || !should_monitor {
            return;
        }
        let Some(monitor) = directory_monitor() else {
            return;
        };
        let reported = Arc::clone(&self.reported);
        self.watch = monitor.watch(
            full_path,
            Arc::new(move || reported.store(true, Ordering::SeqCst)),
        );
    }

    /// For deactivate and dispose: no longer active, so no report can arm a refresh, then the
    /// watch is torn down.
    pub(super) fn teardown(&mut self) {
        self.is_active = false;
        if let Some(interval) = self.waiting_for_refresh.take() {
            interval.clear();
        }
        self.watch = None;
        self.reported.store(false, Ordering::SeqCst);
    }

    /// Turns the watch off and on around the container's own writes.
    pub(super) fn set_watch_enabled(&self, enabled: bool) {
        if let Some(watch) = &self.watch {
            watch.set_enabled(enabled);
        }
    }

    /// Whether the watch reported since the last call.
    pub(super) fn take_report(&self) -> bool {
        self.reported.swap(false, Ordering::SeqCst)
    }

    /// Notes the change and arms the interval while
    /// active.
    pub(super) fn contents_changed(&mut self) {
        self.last_change_ms = now_ms();
        if self.is_active && self.waiting_for_refresh.is_none() {
            self.waiting_for_refresh = Some(RefreshInterval {
                active: Rc::new(Cell::new(true)),
            });
        }
    }

    /// True (and the interval cleared) once the quiet half second has passed.
    pub(super) fn refresh_due(&mut self) -> bool {
        if self.waiting_for_refresh.is_none() || now_ms() <= self.last_change_ms + QUIET_MS {
            return false;
        }
        if let Some(interval) = self.waiting_for_refresh.take() {
            interval.clear();
        }
        true
    }

    pub(super) fn pending_refresh(&self) -> Option<RefreshInterval> {
        self.waiting_for_refresh.clone()
    }

    /// The flag a copy task sets once its writes have landed, asking this folder to relist.
    pub(super) fn relist_handle(&self) -> Rc<Cell<bool>> {
        Rc::clone(&self.relist)
    }

    /// Whether a relist was asked for since the last call.
    pub(super) fn take_relist(&self) -> bool {
        self.relist.replace(false)
    }
}
