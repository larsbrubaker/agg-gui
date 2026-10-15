//! Remote image loads and loaders scoped to the UI thread that paints them
//! (`agg_gui::remote_images_in_flight_on_this_thread`,
//! `agg_gui::set_remote_image_loader_on_this_thread`,
//! `widgets/markdown/image_loader.rs`).
//!
//! Several Apps run side by side in one process, each on a UI thread bound to
//! a queue of its own, as parallel headless tests do. Each thread's count of
//! loads in flight sees only its own loads, however they settle (completed on
//! a worker thread, failed, or dropped uncompleted), and a loader installed
//! for one thread never serves another thread's views. Fake loaders hold each
//! load's completion for the test to settle; no socket is opened.
//!
//! Every UI thread here binds a queue of its own
//! (`ui_thread::mark_current_thread_as_ui_thread_with_own_queue`), never the
//! main queue, which counts every unbound thread's loads too.
//!
//! In a test binary of its own because one test here installs the
//! process-wide default loader, which the library's unit tests must not see.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, Mutex};

use agg_gui::{
    clear_remote_image_loader_on_this_thread, remote_image_loader_on_this_thread,
    remote_images_in_flight, remote_images_in_flight_on_this_thread,
    set_default_remote_image_loader, set_remote_image_loader_on_this_thread, App, Font,
    Framebuffer, GfxCtx, MarkdownView, RemoteImageDone, RemoteImageLoader, Size,
};

const TEST_FONT: &[u8] = include_bytes!("../../demo/assets/LiberationSans-Regular.ttf");
const VIEWPORT: Size = Size {
    width: 300.0,
    height: 300.0,
};

/// Records each requested url and holds its completion for the test.
#[derive(Default)]
struct HoldingLoader {
    requests: Mutex<Vec<String>>,
    pending: Mutex<Vec<RemoteImageDone>>,
}

impl RemoteImageLoader for HoldingLoader {
    fn load(&self, url: &str, done: RemoteImageDone) {
        self.requests
            .lock()
            .expect("requests")
            .push(url.to_string());
        self.pending.lock().expect("pending").push(done);
    }
}

impl HoldingLoader {
    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests").clone()
    }

    /// The most recent completion still held.
    fn take(&self) -> Option<RemoteImageDone> {
        self.pending.lock().expect("pending").pop()
    }
}

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

/// One frame: layout and paint, which starts the loads of visible images.
fn frame(app: &mut App) {
    app.layout(VIEWPORT);
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

/// Markdown showing one picture per url, one per line.
fn pictures(urls: &[String]) -> String {
    urls.iter()
        .map(|url| format!("![picture]({url})\n\n"))
        .collect()
}

/// The urls `count` pictures of thread `name` load from.
fn urls(name: &str, count: usize) -> Vec<String> {
    (0..count)
        .map(|i| format!("https://example.com/{name}/{i}.png"))
        .collect()
}

/// What one UI thread saw at each step of
/// [`each_ui_thread_counts_only_its_own_remote_image_loads`].
struct Seen {
    requests: Vec<String>,
    /// This thread's count before painting, after both threads started their
    /// loads, after a worker completed one, after each further settle.
    counts: Vec<usize>,
    /// The process-wide count while both threads' loads were in flight.
    process_count: usize,
}

#[test]
fn each_ui_thread_counts_only_its_own_remote_image_loads() {
    // Both threads run each step before either reads its count. Neither
    // asserts until both are done, so a wrong count fails the test instead
    // of leaving the other thread waiting at the barrier.
    let barrier = Arc::new(Barrier::new(2));

    let run = |name: &'static str, loads: usize, barrier: Arc<Barrier>| {
        std::thread::spawn(move || {
            agg_gui::ui_thread::mark_current_thread_as_ui_thread_with_own_queue();
            let loader = Arc::new(HoldingLoader::default());
            let view = MarkdownView::new(pictures(&urls(name, loads)), font())
                .with_remote_image_loader(loader.clone());
            let mut app = App::new(Box::new(view));
            let mut counts = vec![remote_images_in_flight_on_this_thread()];

            frame(&mut app);
            barrier.wait();
            counts.push(remote_images_in_flight_on_this_thread());
            let process_count = remote_images_in_flight();
            barrier.wait();

            // A worker thread (bound to no queue) completes one load.
            if let Some(done) = loader.take() {
                std::thread::spawn(move || done.deliver_rgba([255u8, 0, 0, 255].repeat(4), 2, 2))
                    .join()
                    .expect("worker delivers");
            }
            barrier.wait();
            counts.push(remote_images_in_flight_on_this_thread());
            barrier.wait();

            // The rest settle on this thread, alternately dropped uncompleted
            // and failed, one at a time.
            let mut drop_next = true;
            while let Some(done) = loader.take() {
                if drop_next {
                    drop(done);
                } else {
                    done.fail();
                }
                drop_next = !drop_next;
                counts.push(remote_images_in_flight_on_this_thread());
            }
            barrier.wait();
            agg_gui::ui_thread::unbind_current_thread();
            Seen {
                requests: loader.requests(),
                counts,
                process_count,
            }
        })
    };

    let one = run("one", 1, Arc::clone(&barrier));
    let three = run("three", 3, Arc::clone(&barrier));
    let one = one.join().expect("thread one");
    let three = three.join().expect("thread three");

    assert_eq!(one.requests, urls("one", 1), "one asked for its picture");
    assert_eq!(
        three.requests,
        urls("three", 3),
        "three asked for its pictures"
    );
    // Each count sees only that thread's loads: a worker's completion and a
    // dropped or failed completion each settle the starting thread's count.
    assert_eq!(one.counts, vec![0, 1, 0], "one's own loads");
    assert_eq!(three.counts, vec![0, 3, 2, 1, 0], "three's own loads");
    // The process-wide count still sees both threads' loads (and any the
    // other test in this binary has in flight).
    assert!(
        one.process_count >= 4,
        "process count {}",
        one.process_count
    );
    assert!(
        three.process_count >= 4,
        "process count {}",
        three.process_count
    );
}

#[test]
fn a_thread_loader_serves_only_that_threads_views() {
    let process = Arc::new(HoldingLoader::default());
    set_default_remote_image_loader(Some(process.clone()));

    // Both threads' overrides are in place before either paints.
    let barrier = Arc::new(Barrier::new(2));
    let spawn = |name: &'static str, own: Option<Arc<HoldingLoader>>| {
        let barrier = Arc::clone(&barrier);
        let process = Arc::clone(&process);
        std::thread::spawn(move || {
            agg_gui::ui_thread::mark_current_thread_as_ui_thread_with_own_queue();
            // `None` here: this thread loads nothing, whatever the default.
            let own_dyn = own.clone().map(|l| l as Arc<dyn RemoteImageLoader>);
            set_remote_image_loader_on_this_thread(own_dyn);
            barrier.wait();
            let wanted = [
                format!("https://example.com/{name}.png"),
                format!("file:///pictures/{name}.png"),
            ];
            let mut app = App::new(Box::new(MarkdownView::new(pictures(&wanted), font())));
            frame(&mut app);
            let started = remote_images_in_flight_on_this_thread();
            let has_loader = remote_image_loader_on_this_thread().is_some();

            // Back to the process default: a new view loads through it.
            clear_remote_image_loader_on_this_thread();
            let after = [format!("https://example.com/{name}-after.png")];
            let mut app = App::new(Box::new(MarkdownView::new(pictures(&after), font())));
            frame(&mut app);
            barrier.wait();
            // Every load settles before this thread lets its queue go: both
            // threads' loads through the process default (held together,
            // started before the barrier), and this thread's own loader's.
            process.pending.lock().expect("pending").clear();
            let own_requests = own.map(|loader| {
                loader.pending.lock().expect("pending").clear();
                loader.requests()
            });
            agg_gui::ui_thread::unbind_current_thread();
            (started, has_loader, own_requests)
        })
    };

    let own = spawn("own", Some(Arc::new(HoldingLoader::default())));
    let none = spawn("none", None);
    let own = own.join().expect("thread own");
    let none = none.join().expect("thread none");
    let mut asked = process.requests();
    asked.sort();
    set_default_remote_image_loader(None);

    // "own" loaded its web and its file:// picture through its own loader.
    let own_urls = vec![
        "https://example.com/own.png".to_string(),
        "file:///pictures/own.png".to_string(),
    ];
    assert_eq!(own, (2, true, Some(own_urls)));
    // "none" loaded nothing while its override said so.
    assert_eq!(none, (0, false, None));
    assert_eq!(
        asked,
        vec![
            "https://example.com/none-after.png".to_string(),
            "https://example.com/own-after.png".to_string(),
        ],
        "the process default served only views on threads without an override"
    );
}

#[test]
fn own_queue_threads_never_see_unbound_threads_posts_or_loads() {
    // The two UI threads bind, then a thread bound to no queue posts work
    // and starts a load, then the UI threads drain and count.
    let barrier = Arc::new(Barrier::new(3));
    let ui = |barrier: Arc<Barrier>, ran: Arc<AtomicBool>| {
        std::thread::spawn(move || {
            let queue = agg_gui::ui_thread::mark_current_thread_as_ui_thread_with_own_queue();
            let bound_here = agg_gui::ui_thread::current_queue().same_queue(&queue);
            barrier.wait();
            barrier.wait();
            agg_gui::ui_thread::invoke_pending_actions();
            let seen = (
                bound_here,
                agg_gui::ui_thread::is_ui_thread(),
                ran.load(Ordering::SeqCst),
                remote_images_in_flight_on_this_thread(),
            );
            barrier.wait();
            agg_gui::ui_thread::unbind_current_thread();
            (queue, seen)
        })
    };
    let ran = Arc::new(AtomicBool::new(false));
    let first = ui(Arc::clone(&barrier), Arc::clone(&ran));
    let second = ui(Arc::clone(&barrier), Arc::clone(&ran));

    let poster = {
        let barrier = Arc::clone(&barrier);
        let ran = Arc::clone(&ran);
        std::thread::spawn(move || {
            barrier.wait();
            agg_gui::ui_thread::run_on_idle(move || ran.store(true, Ordering::SeqCst));
            let loader = Arc::new(HoldingLoader::default());
            let view = MarkdownView::new(pictures(&urls("unbound", 1)), font())
                .with_remote_image_loader(loader.clone());
            frame(&mut App::new(Box::new(view)));
            let main = agg_gui::ui_thread::current_queue();
            let main_count = main.remote_images_in_flight();
            barrier.wait();
            barrier.wait();
            drop(loader.take());
            let main_after = main.remote_images_in_flight();
            (main, main_count, main_after)
        })
    };

    let (first_queue, first_seen) = first.join().expect("first UI thread");
    let (second_queue, second_seen) = second.join().expect("second UI thread");
    let (main, main_count, main_after) = poster.join().expect("poster");

    // Bound to its own queue, a UI thread, and neither the unbound post nor
    // the unbound load reached it.
    assert_eq!(first_seen, (true, true, false, 0));
    assert_eq!(second_seen, (true, true, false, 0));
    assert!(!first_queue.same_queue(&second_queue));
    assert!(!first_queue.same_queue(&main) && !second_queue.same_queue(&main));
    // The unbound load counted on the main queue until it settled.
    assert!(main_count >= 1, "main queue count {main_count}");
    assert_eq!(main_after, main_count - 1);
}

/// Completes every load inside `load`, noting this thread's count just
/// before and just after completing.
#[derive(Default)]
struct ImmediateLoader {
    counts: Mutex<Vec<(usize, usize)>>,
}

impl RemoteImageLoader for ImmediateLoader {
    fn load(&self, _url: &str, done: RemoteImageDone) {
        let before = remote_images_in_flight_on_this_thread();
        done.deliver_rgba([0u8, 0, 255, 255].repeat(4), 2, 2);
        let after = remote_images_in_flight_on_this_thread();
        self.counts.lock().expect("counts").push((before, after));
    }
}

#[test]
fn a_load_completed_inside_load_counts_while_it_runs_and_not_after() {
    let counts = std::thread::spawn(|| {
        agg_gui::ui_thread::mark_current_thread_as_ui_thread_with_own_queue();
        let loader = Arc::new(ImmediateLoader::default());
        let view = MarkdownView::new(pictures(&urls("immediate", 2)), font())
            .with_remote_image_loader(loader.clone());
        let mut app = App::new(Box::new(view));
        frame(&mut app);
        let left = remote_images_in_flight_on_this_thread();
        agg_gui::ui_thread::unbind_current_thread();
        let counts = loader.counts.lock().expect("counts").clone();
        (counts, left)
    })
    .join()
    .expect("UI thread");
    // Counted from before the loader runs, so a completion inside `load`
    // never takes the count below zero.
    assert_eq!(counts, (vec![(1, 0), (1, 0)], 0));
}

#[test]
fn a_load_settled_after_its_thread_ended_settles_that_threads_queue() {
    let (queue, loader, counts) = std::thread::spawn(|| {
        let queue = agg_gui::ui_thread::mark_current_thread_as_ui_thread_with_own_queue();
        let loader = Arc::new(HoldingLoader::default());
        let view = MarkdownView::new(pictures(&urls("late", 1)), font())
            .with_remote_image_loader(loader.clone());
        frame(&mut App::new(Box::new(view)));
        let started = remote_images_in_flight_on_this_thread();
        agg_gui::ui_thread::unbind_current_thread();
        let unbound = queue.remote_images_in_flight();
        (queue, loader, (started, unbound))
    })
    .join()
    .expect("UI thread");
    assert_eq!(counts, (1, 1), "the load stays counted after unbinding");
    assert_eq!(queue.remote_images_in_flight(), 1);
    // Completed on this (another, unbound) thread after the UI thread ended.
    loader
        .take()
        .expect("held load")
        .deliver_rgba([255u8, 0, 0, 255].repeat(4), 2, 2);
    assert_eq!(queue.remote_images_in_flight(), 0);
}
