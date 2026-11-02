//! A temporary folder for one test, removed when dropped, and the process's test trash: every
//! remove in this binary goes to one temporary trash folder (`set_trash_directory`), where items
//! get unique names, so tests running side by side never collide and never fill the real Trash.
//! Also the lock that runs the tests registering a process-wide directory monitor one at a
//! time, and `block_on`, which drives a local future to its end on the test's thread.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, Once};
use std::task::{Context, Poll, Waker};

use agg_gui_files::disk::{set_trash_directory, DiskRules};

static DIRECTORY_MONITOR: Mutex<()> = Mutex::new(());

/// Holds the process-wide monitor registration for one test; a test that failed while holding
/// it does not fail the next one.
pub fn hold_directory_monitor() -> MutexGuard<'static, ()> {
    DIRECTORY_MONITOR
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Polls `future` until it finishes. The disk work here never waits on anything outside the
/// thread (a search only yields), so polling again is all it needs.
pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
}

static NEXT: AtomicU64 = AtomicU64::new(0);
static TRASH: Once = Once::new();

/// A fresh, empty folder under the system's temporary folder.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        TRASH.call_once(|| set_trash_directory(Some(unique_path("trash"))));
        let path = unique_path("dir");
        std::fs::create_dir_all(&path).expect("the temporary folder is made");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Writes `bytes` to `name` inside the folder and answers its path.
    pub fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).expect("the test file is written");
        path
    }

    /// Makes the folder `name` inside the folder and answers its path.
    pub fn folder(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(&path).expect("the test folder is made");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A path no other test in this process uses.
fn unique_path(kind: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "agg-gui-files-disk-{kind}-{}-{n}",
        std::process::id()
    ))
}

/// Rules listing only `.stl` files, read-only when asked.
pub struct StlOnly {
    pub read_only: bool,
}

impl DiskRules for StlOnly {
    fn lists_file(&self, name: &str) -> bool {
        name.to_lowercase().ends_with(".stl")
    }
    fn read_only(&self) -> bool {
        self.read_only
    }
}

/// Writable `.stl`-only rules.
pub fn stl_only() -> Rc<dyn DiskRules> {
    Rc::new(StlOnly { read_only: false })
}

/// Read-only `.stl`-only rules.
pub fn stl_only_read_only() -> Rc<dyn DiskRules> {
    Rc::new(StlOnly { read_only: true })
}
