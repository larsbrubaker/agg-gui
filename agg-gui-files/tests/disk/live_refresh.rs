//! The directory watch seam (`DirectoryMonitor`) as a folder on disk uses it: a [`DiskFolder`]
//! asks the registered monitor to watch its folder while active, reacts to what the monitor
//! reports, hands the watch back on teardown (by dropping it), pauses it around its own writes,
//! and works the same with no monitor registered (the browser and every headless run). The
//! callback only flags the report (it may run on the watch's thread) and the UI thread's turn,
//! `pump_live_refresh`, turns it into `directory_contents_changed`, so each report below is
//! followed by that turn. Every test here registers a process-wide monitor, so they run one at
//! a time (`hold_directory_monitor`).

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use agg_gui_files::disk::{
    directory_monitor, set_directory_monitor, DirectoryChanged, DirectoryMonitor, DirectoryWatch,
    DiskFolder, DiskFolderLink,
};
use agg_gui_files::{Folder, Item, WritableFolder};

use super::temp::{hold_directory_monitor, stl_only, TempDir};

/// What the test can see of one fake watch after the container took it.
#[derive(Default)]
struct FakeWatchState {
    /// Whether the watch is turned off; a watch starts enabled.
    disabled: AtomicBool,
    disposed: AtomicBool,
    /// How many times the watch was turned off.
    pauses: AtomicUsize,
}

impl FakeWatchState {
    fn enabled(&self) -> bool {
        !self.disabled.load(Ordering::SeqCst)
    }

    fn disposed(&self) -> bool {
        self.disposed.load(Ordering::SeqCst)
    }
}

struct FakeWatch {
    state: Arc<FakeWatchState>,
}

impl DirectoryWatch for FakeWatch {
    fn enabled(&self) -> bool {
        self.state.enabled()
    }

    fn set_enabled(&self, enabled: bool) {
        if !enabled {
            self.state.pauses.fetch_add(1, Ordering::SeqCst);
        }
        self.state.disabled.store(!enabled, Ordering::SeqCst);
    }
}

/// Dropping a watch hands it back to the monitor.
impl Drop for FakeWatch {
    fn drop(&mut self) {
        self.state.disposed.store(true, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct FakeDirectoryMonitor {
    watch_count: AtomicUsize,
    watched_path: Mutex<Option<PathBuf>>,
    report_change: Mutex<Option<DirectoryChanged>>,
    last_watch: Mutex<Option<Arc<FakeWatchState>>>,
}

impl FakeDirectoryMonitor {
    fn watch_count(&self) -> usize {
        self.watch_count.load(Ordering::SeqCst)
    }

    fn watched_path(&self) -> Option<PathBuf> {
        self.watched_path.lock().unwrap().clone()
    }

    fn last_watch(&self) -> Arc<FakeWatchState> {
        self.last_watch
            .lock()
            .unwrap()
            .clone()
            .expect("the container asked for a watch")
    }

    /// Reports a change exactly as a real monitor would - by calling what it was handed.
    fn report_change(&self) {
        let report = self.report_change.lock().unwrap().clone();
        (report.expect("the container asked for a watch"))();
    }
}

impl DirectoryMonitor for FakeDirectoryMonitor {
    fn watch(
        &self,
        directory_path: &Path,
        directory_changed: DirectoryChanged,
    ) -> Option<Box<dyn DirectoryWatch>> {
        self.watch_count.fetch_add(1, Ordering::SeqCst);
        *self.watched_path.lock().unwrap() = Some(directory_path.to_path_buf());
        *self.report_change.lock().unwrap() = Some(directory_changed);
        let state = Arc::new(FakeWatchState::default());
        *self.last_watch.lock().unwrap() = Some(Arc::clone(&state));
        Some(Box::new(FakeWatch { state }))
    }
}

/// Registers `monitor` for the duration of `test`, handing it a fresh temporary directory, and
/// puts the process-wide registration (and the directory) back after, even when `test` fails.
fn with_registered_monitor(monitor: Arc<FakeDirectoryMonitor>, test: impl FnOnce(&Path)) {
    let _serial = hold_directory_monitor();
    let directory = TempDir::new();

    struct Restore {
        saved: Option<Arc<dyn DirectoryMonitor>>,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            set_directory_monitor(self.saved.take());
        }
    }
    let _restore = Restore {
        saved: directory_monitor(),
    };
    set_directory_monitor(Some(monitor));

    test(directory.path());
}

/// The UI thread's turn after a report, which turns the flagged report into a refresh.
fn marshal_reports(container: &mut DiskFolder) {
    container.pump_live_refresh();
}

#[test]
fn activating_a_container_asks_the_platform_to_watch_its_directory() {
    let monitor = Arc::new(FakeDirectoryMonitor::default());

    with_registered_monitor(Arc::clone(&monitor), |directory| {
        let mut container = DiskFolder::new(directory, stl_only());

        assert_eq!(
            monitor.watch_count(),
            0,
            "a container that has not been activated is not showing anything to keep fresh"
        );

        container.activate();

        assert_eq!(monitor.watch_count(), 1);
        assert_eq!(monitor.watched_path().as_deref(), Some(directory));

        container.dispose();
    });
}

#[test]
fn a_change_reported_through_the_seam_arms_the_containers_refresh() {
    let monitor = Arc::new(FakeDirectoryMonitor::default());

    with_registered_monitor(Arc::clone(&monitor), |directory| {
        let mut container = DiskFolder::new(directory, stl_only());
        container.activate();

        assert!(
            container.pending_refresh().is_none(),
            "nothing has changed yet, so there is nothing to reload"
        );

        monitor.report_change();
        marshal_reports(&mut container);

        assert!(
            container.pending_refresh().is_some(),
            "a change reported through the seam is what arms the coalescing reload"
        );

        // Also cancels the interval just armed, so it cannot fire into a later test.
        container.dispose();
    });
}

#[test]
fn tearing_down_a_container_disposes_the_watch_it_was_given() {
    let monitor = Arc::new(FakeDirectoryMonitor::default());

    with_registered_monitor(Arc::clone(&monitor), |directory| {
        let mut container = DiskFolder::new(directory, stl_only());
        container.activate();

        let watch = monitor.last_watch();

        assert!(!watch.disposed());

        container.deactivate();

        assert!(
            watch.disposed(),
            "a container nobody is looking at must not leave a live watch behind"
        );
    });
}

/// Add pauses the watch around its own disk writes so they do not echo back as outside changes.
/// A write that fails must still hand the watch back, or the folder silently stops
/// live-refreshing for good.
#[test]
fn a_failed_add_still_turns_the_watch_back_on() {
    let monitor = Arc::new(FakeDirectoryMonitor::default());

    with_registered_monitor(Arc::clone(&monitor), |directory| {
        // Moving a folder into its own subfolder fails on every platform - a deterministic
        // failing Add.
        let source = directory.join("source");
        let container_path = source.join("inner");
        std::fs::create_dir_all(&container_path).unwrap();

        let mut container = DiskFolder::new(&container_path, stl_only());
        container.activate();

        let watch = monitor.last_watch();

        let items: Vec<Rc<dyn Item>> = vec![Rc::new(DiskFolderLink::new(&source, stl_only()))];
        assert!(WritableFolder::add(&mut container, &items).is_err());

        assert!(
            watch.enabled(),
            "a failed add must not leave the folder's live refresh switched off"
        );

        container.dispose();
    });
}

/// A platform watch reports on its own thread, so a report can arrive after the container was
/// torn down on the UI thread. It must not arm a reload of a container nobody is looking at.
#[test]
fn a_change_reported_after_teardown_arms_no_refresh() {
    let monitor = Arc::new(FakeDirectoryMonitor::default());

    with_registered_monitor(Arc::clone(&monitor), |directory| {
        let mut deactivated = DiskFolder::new(directory, stl_only());
        deactivated.activate();
        deactivated.deactivate();

        monitor.report_change();
        marshal_reports(&mut deactivated);

        assert!(
            deactivated.pending_refresh().is_none(),
            "a report arriving after Deactivate must not arm a reload"
        );

        let mut disposed = DiskFolder::new(directory, stl_only());
        disposed.activate();
        disposed.dispose();

        monitor.report_change();
        marshal_reports(&mut disposed);

        assert!(
            disposed.pending_refresh().is_none(),
            "a report arriving after Dispose must not arm a reload"
        );
    });
}

/// The browser and every headless run land here: no monitor registered, so the container lists
/// its folder and never hears about outside edits. It must do that silently - this is the path
/// that would otherwise have to be a null check at every call site.
#[test]
fn with_no_monitor_registered_the_container_still_activates_and_loads() {
    let _serial = hold_directory_monitor();
    let temp = TempDir::new();
    let directory = temp.path().to_path_buf();
    std::fs::write(directory.join("part.stl"), b"solid part\nendsolid part\n").unwrap();

    let saved = directory_monitor();
    set_directory_monitor(None);

    // With no monitor registered at all, there is no watch.
    let watch = directory_monitor().and_then(|monitor| monitor.watch(&directory, Arc::new(|| {})));
    let no_watch = watch.is_none();

    let mut container = DiskFolder::new(&directory, stl_only());
    container.activate();
    container.load();
    let items = container.items();

    container.deactivate();
    container.dispose();
    set_directory_monitor(saved);

    assert!(
        no_watch,
        "the default monitor hands out no watches at all, on any directory"
    );
    assert_eq!(items.len(), 1, "the container lists its folder");
}

/// A folder's own writes run with its watch off and hand it back on, so they neither echo back
/// as outside changes nor leave the folder deaf to the next outside change.
#[test]
fn the_folders_own_writes_hand_the_watch_back_on() {
    let monitor = Arc::new(FakeDirectoryMonitor::default());

    with_registered_monitor(Arc::clone(&monitor), |directory| {
        std::fs::write(directory.join("part.stl"), b"p").unwrap();
        let mut folder = DiskFolder::new(directory, stl_only());
        folder.activate();
        folder.load();
        let watch = monitor.last_watch();
        let part = folder.items()[0].clone();
        let (_, renamed) = folder.rename_on_disk(&part, "renamed");
        renamed.unwrap();
        assert!(watch.enabled());
        assert_eq!(
            watch.pauses.load(Ordering::SeqCst),
            1,
            "the rename ran unwatched"
        );
        let made: Rc<dyn Item> = Rc::new(agg_gui_files::CreateFolderItem::new("Made"));
        WritableFolder::add(&mut folder, &[made]).unwrap();
        assert!(watch.enabled());
        assert_eq!(
            watch.pauses.load(Ordering::SeqCst),
            2,
            "the add ran unwatched"
        );
        let renamed = folder.items()[0].clone();
        let (_, removed) = folder.remove_to_trash(&[renamed]);
        removed.unwrap();
        assert!(watch.enabled());
        assert_eq!(
            watch.pauses.load(Ordering::SeqCst),
            3,
            "the remove ran unwatched"
        );
        folder.dispose();
    });
}

/// A copy run as a task writes through a folder of its own; the folder the user is looking at
/// relists at its next turn once the copy has landed, with no watch needed.
#[test]
fn a_copy_task_has_the_showing_folder_relist_at_its_next_turn() {
    let _serial = hold_directory_monitor();
    let saved = directory_monitor();
    set_directory_monitor(None);
    let dir = TempDir::new();
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    folder.activate();
    folder.load();
    let elsewhere = agg_gui_files::MemoryFolder::new("Elsewhere");
    let gear = elsewhere.add_file("gear.stl", b"gear".to_vec());
    let outcome = Rc::new(std::cell::RefCell::new(None));
    let heard = Rc::clone(&outcome);
    let task = folder
        .copy_in(
            vec![gear],
            Box::new(move |done| *heard.borrow_mut() = Some(done.result)),
        )
        .expect("a folder on disk runs its own copy");
    assert!(
        folder.items().is_empty(),
        "nothing is copied before the task runs"
    );
    super::temp::block_on(task);
    assert_eq!(outcome.borrow_mut().take(), Some(Ok(())));
    let changed = Rc::new(std::cell::Cell::new(false));
    let flag = Rc::clone(&changed);
    folder.subscribe_content_changed(Rc::new(move || flag.set(true)));
    folder.pump_live_refresh();
    assert!(changed.get(), "the next turn relists");
    let names: Vec<String> = folder
        .items()
        .iter()
        .map(|i| i.name().to_string())
        .collect();
    assert_eq!(names, ["gear.stl"]);
    folder.dispose();
    set_directory_monitor(saved);
}

/// End to end through the desktop monitor (`PollingDirectoryMonitor`): a folder created outside
/// the app shows up in an open folder without the user refreshing it. The per-frame turn is
/// pumped until the folder reports its reload; the deadline only bounds a failure.
#[test]
fn a_folder_created_outside_the_app_appears_in_the_open_folder() {
    let _serial = hold_directory_monitor();
    let dir = TempDir::new();
    let saved = directory_monitor();
    set_directory_monitor(Some(Arc::new(agg_gui_files::disk::PollingDirectoryMonitor)));

    let mut folder = DiskFolder::new(dir.path(), stl_only());
    folder.activate();
    folder.load();
    assert_eq!(folder.child_containers().len(), 0);

    let reloaded = Rc::new(std::cell::Cell::new(false));
    let flag = Rc::clone(&reloaded);
    folder.subscribe_content_changed(Rc::new(move || flag.set(true)));

    std::fs::create_dir_all(dir.path().join("Made Elsewhere")).unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !reloaded.get() && std::time::Instant::now() < deadline {
        folder.pump_live_refresh();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let names: Vec<String> = folder
        .child_containers()
        .iter()
        .map(|c| c.name().to_string())
        .collect();
    folder.deactivate();
    folder.dispose();
    set_directory_monitor(saved);

    assert!(
        reloaded.get(),
        "the watch reports the new folder and the folder relists"
    );
    assert!(names.contains(&"Made Elsewhere".to_string()), "{names:?}");
}
