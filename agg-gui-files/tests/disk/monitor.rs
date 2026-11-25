//! The desktop directory monitor (`PollingDirectoryMonitor`, its `PollingWatch`) and the
//! process-wide registration (`directory_monitor()`). Asking for a watch never waits on the
//! disk, so each test that changes a folder first waits for the watch's first listing (its
//! baseline) and then changes it. Every wait is on the watch or the callback itself; the timeout
//! is only a safety net that fails the test, never the thing that paces it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use agg_gui_files::disk::{
    directory_monitor, DirectoryChanged, DirectoryMonitor, PollingDirectoryMonitor, PollingWatch,
};

use super::temp::{hold_directory_monitor, TempDir};

const SAFETY_TIMEOUT: Duration = Duration::from_secs(30);

/// A callback that signals the returned receiver each time it is called.
fn signal() -> (DirectoryChanged, mpsc::Receiver<()>) {
    let (sender, receiver) = mpsc::channel();
    let sender = Mutex::new(sender);
    let changed: DirectoryChanged = Arc::new(move || {
        let _ = sender.lock().unwrap().send(());
    });
    (changed, receiver)
}

fn reported(signal: &mpsc::Receiver<()>) -> bool {
    signal.recv_timeout(SAFETY_TIMEOUT).is_ok()
}

/// A watch on `folder` whose baseline has been taken, so every change after this reports.
fn listed_watch(folder: &std::path::Path, changed: DirectoryChanged) -> PollingWatch {
    let watch = PollingDirectoryMonitor
        .watch_directory(folder, changed)
        .expect("a desktop watch starts");
    assert!(watch.wait_for_first_listing(SAFETY_TIMEOUT));
    watch
}

#[test]
fn creating_a_subfolder_reports() {
    let folder_dir = TempDir::new();
    let folder = folder_dir.path().to_path_buf();
    let (changed, signal) = signal();
    let watch = listed_watch(&folder, changed);

    std::fs::create_dir_all(folder.join("sub")).unwrap();

    let was_reported = reported(&signal);
    drop(watch);
    assert!(was_reported);
}

/// A folder that already holds a file reports the file added after the watch started, and not
/// the file it held: every report notes whether the new file existed when it was made, and the
/// first one must have seen it.
#[test]
fn a_new_file_reports_and_the_first_listing_does_not() {
    let folder_dir = TempDir::new();
    folder_dir.file("already.txt", b"there");
    let folder = folder_dir.path().to_path_buf();
    let new_file = folder.join("file.txt");
    let (sender, signal) = mpsc::channel();
    let sender = Mutex::new(sender);
    let seen = new_file.clone();
    let changed: DirectoryChanged = Arc::new(move || {
        let _ = sender.lock().unwrap().send(seen.exists());
    });
    let watch = listed_watch(&folder, changed);

    std::fs::write(&new_file, "hello").unwrap();

    let first_report = signal.recv_timeout(SAFETY_TIMEOUT);
    drop(watch);
    assert_eq!(
        first_report,
        Ok(true),
        "the first report is the new file, not the folder's starting contents"
    );
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
    let sentinel_watch = listed_watch(&sentinel_folder, changed);

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

/// A folder that is not there when the watch starts (a network share that is away) is watched
/// all the same, and reports when it appears, so the caller needs no retry of its own.
#[test]
fn a_missing_folder_is_watched_and_reports_when_it_appears() {
    let dir = TempDir::new();
    let missing = dir.path().join("missing");

    let through_trait = PollingDirectoryMonitor.watch(&missing, Arc::new(|| {}));
    assert!(
        through_trait.is_some(),
        "a missing folder still gets a watch"
    );
    drop(through_trait);

    let (changed, signal) = signal();
    let watch = listed_watch(&missing, changed);
    std::fs::create_dir_all(&missing).unwrap();

    let was_reported = reported(&signal);
    drop(watch);
    assert!(was_reported, "the folder coming back is reported");
}

/// Starting a watch does not wait for its first listing: the listing here blocks until the test
/// lets it go, and the watch is already in hand, with no first listing yet, before it does.
#[test]
fn a_watch_returns_before_its_first_listing() {
    let (release, released) = mpsc::channel::<()>();
    let mut first = true;
    let list = move || {
        if first {
            first = false;
            released.recv_timeout(SAFETY_TIMEOUT).ok()?;
        }
        Some(0_u32)
    };

    let watch = PollingWatch::start(list, Arc::new(|| {})).expect("the watch starts");
    let listed_before_release = watch.has_first_listing();
    release.send(()).unwrap();
    let listed_after_release = watch.wait_for_first_listing(SAFETY_TIMEOUT);
    drop(watch);

    assert!(!listed_before_release, "the watch came back before listing");
    assert!(listed_after_release);
}

/// A listing that cannot be read at first reports once it can, and a listing readable from the
/// start that never changes never reports, checked once its first listing has been compared with
/// a later one.
#[test]
fn an_unreadable_start_reports_once_readable_and_a_steady_one_never_does() {
    let mut unreadable_listings = 0;
    let comes_back = move || {
        unreadable_listings += 1;
        (unreadable_listings > 1).then_some(7_u32)
    };
    let (changed, came_back) = signal();
    let returning = PollingWatch::start(comes_back, changed).expect("the watch starts");

    let steady_calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&steady_calls);
    let steady_listings = Arc::new(AtomicUsize::new(0));
    let listings = Arc::clone(&steady_listings);
    let steady = PollingWatch::start(
        move || {
            listings.fetch_add(1, Ordering::SeqCst);
            Some(7_u32)
        },
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .expect("the watch starts");

    let was_reported = reported(&came_back);
    drop(returning);
    // Once the steady watch has started its third listing, its second has been compared with
    // its baseline (and reported, had it differed): wait on its own count, bounded by the
    // safety net.
    let deadline = std::time::Instant::now() + SAFETY_TIMEOUT;
    while steady_listings.load(Ordering::SeqCst) < 3 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    let compared = steady_listings.load(Ordering::SeqCst) >= 3;
    let steady_reports = steady_calls.load(Ordering::SeqCst);
    drop(steady);

    assert!(was_reported, "the first readable listing reports");
    assert!(compared);
    assert_eq!(
        steady_reports, 0,
        "a listing that never changes never reports"
    );
}

/// A watch dropped while a listing is running (a slow share) never reports what that listing
/// finds. The second listing waits until the test has dropped the watch and then answers
/// differently; the listing's own channel closes when the thread ends, which is when the test
/// looks at the count.
#[test]
fn a_watch_dropped_during_a_listing_never_reports_it() {
    let (in_listing, listing_started) = mpsc::channel::<()>();
    let (release, released) = mpsc::channel::<()>();
    let mut listings = 0_u32;
    let list = move || {
        listings += 1;
        if listings == 2 {
            in_listing.send(()).ok()?;
            released.recv_timeout(SAFETY_TIMEOUT).ok()?;
        }
        Some(listings)
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let watch = PollingWatch::start(
        list,
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .expect("the watch starts");

    assert!(listing_started.recv_timeout(SAFETY_TIMEOUT).is_ok());
    drop(watch);
    release.send(()).unwrap();
    // The thread drops `list`, and with it the sender, when it ends.
    let ended = matches!(
        listing_started.recv_timeout(SAFETY_TIMEOUT),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );

    assert!(ended);
    assert_eq!(calls.load(Ordering::SeqCst), 0, "a dropped watch is silent");
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
