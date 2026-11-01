//! The search across folders (`SearchResultsFolder`): its walk, sections, origins, the wait for a
//! folder whose listing comes later (and the timer bounding it), cancelling, failures, and the
//! content-file test. A folder with its own search is `StubFolder` with a `search` closure; a
//! load waiter is released by the folder's content-changed, so the late-listing stub raises it
//! when its listing arrives. The searches run on the tests' own executor (`support`).

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use agg_gui_files::{
    yield_now, ContentChangedEvent, ContentChangedListener, DynamicFolderLink, Folder, FolderLink,
    Item, MemoryFolder, MessageItem, Picture, PictureFuture, SearchCancel, SearchFuture,
    SearchProvider, SearchReport, SearchResultsFolder,
};
use support::TestSpawner;

/// A stub folder's own search: given the report and the token, a future.
type StubSearch = Rc<dyn Fn(SearchReport, SearchCancel) -> SearchFuture<'static>>;

struct StubItem(String);

impl Item for StubItem {
    fn id(&self) -> &str {
        &self.0
    }
    fn name(&self) -> &str {
        &self.0
    }
    fn is_protected(&self) -> bool {
        false
    }
    fn is_visible(&self) -> bool {
        true
    }
}

/// A minimal folder that knows nothing about searching, or the sectioned shape when
/// `sectioned`, or one whose listing comes after `load` when `pending` is set.
#[derive(Clone)]
struct StubFolder {
    name: String,
    items: Rc<RefCell<Vec<Rc<dyn Item>>>>,
    children: Rc<RefCell<Vec<Rc<dyn FolderLink>>>>,
    sectioned: bool,
    pending: Option<Rc<Cell<bool>>>,
    load_started: Rc<Cell<bool>>,
    content_changed: ContentChangedEvent,
    /// How many content-changed listeners are subscribed now.
    subscribers: Rc<Cell<i32>>,
    search: Option<StubSearch>,
    thumbnail: Option<(u32, u32)>,
}

impl StubFolder {
    fn new(name: &str, item_names: &[&str]) -> Self {
        Self {
            name: name.to_string(),
            items: Rc::new(RefCell::new(
                item_names
                    .iter()
                    .map(|n| Rc::new(StubItem(n.to_string())) as _)
                    .collect(),
            )),
            children: Rc::default(),
            sectioned: false,
            pending: None,
            load_started: Rc::default(),
            content_changed: ContentChangedEvent::default(),
            subscribers: Rc::default(),
            search: None,
            thumbnail: None,
        }
    }

    /// A folder that runs its own search.
    fn provider(
        name: &str,
        search: impl Fn(SearchReport, SearchCancel) -> SearchFuture<'static> + 'static,
    ) -> Self {
        let mut stub = Self::new(name, &[]);
        stub.search = Some(Rc::new(search));
        stub
    }

    fn sectioned(links: Vec<Rc<dyn FolderLink>>) -> Self {
        let mut stub = Self::new("Sectioned Stub", &[]);
        *stub.children.borrow_mut() = links;
        stub.sectioned = true;
        stub
    }

    fn async_load(name: &str) -> Self {
        let mut stub = Self::new(name, &[]);
        stub.items
            .borrow_mut()
            .push(Rc::new(MessageItem::loading()));
        stub.pending = Some(Rc::new(Cell::new(false)));
        stub
    }

    fn finish_load(&self, item_names: &[&str]) {
        *self.items.borrow_mut() = item_names
            .iter()
            .map(|n| Rc::new(StubItem(n.to_string())) as _)
            .collect();
        if let Some(pending) = &self.pending {
            pending.set(false);
        }
        self.content_changed.raise();
    }
}

impl SearchProvider for StubFolder {
    fn search_async<'a>(
        &'a self,
        _query: &str,
        report: SearchReport,
        cancel: SearchCancel,
    ) -> SearchFuture<'a> {
        (self.search.as_ref().expect("a provider stub"))(report, cancel)
    }
}

impl Folder for StubFolder {
    fn id(&self) -> &str {
        &self.name
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn is_protected(&self) -> bool {
        false
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        self.children.borrow().clone()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.items.borrow().clone()
    }
    fn load(&mut self) {
        self.load_started.set(true);
        if let Some(pending) = &self.pending {
            pending.set(true);
        }
    }
    fn search_sections(&self) -> Option<Vec<Rc<dyn FolderLink>>> {
        self.sectioned.then(|| self.child_containers())
    }
    fn is_load_pending(&self) -> bool {
        self.pending.as_ref().is_some_and(|p| p.get())
    }
    fn subscribe_content_changed(&self, listener: ContentChangedListener) {
        self.subscribers.set(self.subscribers.get() + 1);
        self.content_changed.subscribe(listener);
    }
    fn unsubscribe_content_changed(&self, listener: &ContentChangedListener) {
        self.subscribers.set(self.subscribers.get() - 1);
        self.content_changed.unsubscribe(listener);
    }
    fn search_provider(&self) -> Option<&dyn SearchProvider> {
        self.search.is_some().then_some(self as &dyn SearchProvider)
    }
    /// Hands back the folder's own picture.
    fn get_thumbnail(&self, _item: &Rc<dyn Item>, _w: i32, _h: i32) -> Option<PictureFuture> {
        let (width, height) = self.thumbnail?;
        let rgba = vec![0u8; (width * height * 4) as usize];
        Some(Box::pin(std::future::ready(Ok(Some(Picture {
            width,
            height,
            rgba: rgba.into(),
        })))))
    }
}

/// A future that waits until `cancel` is set, and never ends on its own.
struct UntilCancelled(SearchCancel);

impl std::future::Future for UntilCancelled {
    type Output = ();
    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        if self.0.is_cancelled() {
            std::task::Poll::Ready(())
        } else {
            self.0.register(cx.waker());
            std::task::Poll::Pending
        }
    }
}

fn link_to(name: &str, make: impl Fn() -> Box<dyn Folder> + 'static) -> Rc<dyn FolderLink> {
    Rc::new(DynamicFolderLink::new(name, Rc::new(make)))
}

fn names(items: &[Rc<dyn Item>]) -> Vec<String> {
    let mut names: Vec<String> = items.iter().map(|i| i.name().to_string()).collect();
    names.sort();
    names
}

fn sorted(expected: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
    names.sort();
    names
}

/// Runs the search's executor until the search is over.
fn run_to_end(executor: &TestSpawner, results: &SearchResultsFolder) {
    executor.pump_until(Duration::from_secs(30), || results.is_finished());
}

/// A results folder searching `root` for `query` on `executor`, started.
fn started(root: &dyn Folder, query: &str, executor: &Rc<TestSpawner>) -> SearchResultsFolder {
    let mut results = SearchResultsFolder::new(root, query, executor.clone());
    results.start_search();
    results
}

/// A results folder searching `root` for `query` on its own executor, run to the end.
fn search_to_end(root: &dyn Folder, query: &str) -> SearchResultsFolder {
    let executor = TestSpawner::new();
    let results = started(root, query, &executor);
    run_to_end(&executor, &results);
    results
}

#[test]
fn search_from_a_sectioned_root_reports_one_section_per_source_and_maps_items_back_to_them() {
    let alpha = MemoryFolder::new("Alpha");
    alpha.add_file("alpha gear.stl", Vec::new());
    alpha
        .add_folder("nested")
        .expect("a folder")
        .add_file("alpha gear two.stl", Vec::new());
    alpha.add_file("alpha bracket.stl", Vec::new());
    let bravo = MemoryFolder::new("Bravo");
    bravo.add_file("bravo gear.stl", Vec::new());
    let search_root = StubFolder::sectioned(vec![alpha.link(), bravo.link()]);
    let results = search_to_end(&search_root, "gear");

    // The search task completing is the search being over.
    assert!(!results.is_searching());
    let sections = results.sections();
    // One section per searched source is the whole point of grouping the results.
    let section_names: Vec<&str> = sections.iter().map(|s| s.name()).collect();
    assert_eq!(section_names, ["Alpha", "Bravo"]);
    assert_eq!(
        names(&sections[0].items()),
        sorted(&["alpha gear.stl", "alpha gear two.stl"])
    );
    assert_eq!(names(&sections[1].items()), sorted(&["bravo gear.stl"]));
    // The flattened list is what an ordinary list view shows, so every match has to reach it.
    assert_eq!(
        names(&results.items()),
        sorted(&["alpha gear.stl", "alpha gear two.stl", "bravo gear.stl"])
    );
    // Open File Location has to know which source the selected result came from.
    let item_section = results
        .try_get_section(&sections[1].items()[0])
        .expect("section");
    assert!(Rc::ptr_eq(&item_section, &sections[1]));
}

#[test]
fn folders_that_cannot_search_themselves_are_walked_item_by_item() {
    let child = StubFolder::new("Child", &["child gear", "child bracket"]);
    let search_root = StubFolder::new("Root", &["root gear", "root wheel"]);
    search_root
        .children
        .borrow_mut()
        .push(link_to("Child", move || Box::new(child.clone())));
    let results = search_to_end(&search_root, "gear");

    // The fallback walk has to cover the folder's own items and everything beneath it.
    assert_eq!(
        names(&results.items()),
        sorted(&["root gear", "child gear"])
    );
    // A root that does not present itself in sections is one section: itself.
    assert_eq!(results.sections().len(), 1);
}

#[test]
fn a_content_file_extension_is_dropped_only_when_the_app_says_so() {
    let root = StubFolder::new("Root", &["gear box.stl", "notes.txt"]);
    // Without the app's test, names are matched whole: "stl" is in the name.
    assert_eq!(
        names(&search_to_end(&root, "stl").items()),
        sorted(&["gear box.stl"])
    );

    let executor = TestSpawner::new();
    let mut results = SearchResultsFolder::new(&root, "stl", executor.clone())
        .with_content_files(Rc::new(|name: &str| name.ends_with(".stl")));
    results.start_search();
    run_to_end(&executor, &results);
    // A search for "stl" in a folder of meshes would otherwise match every one of them.
    assert!(results.items().is_empty());
}

#[test]
fn the_walk_waits_for_a_folder_that_fills_itself_in_after_load_returns() {
    let async_source = StubFolder::async_load("Async");
    let source = async_source.clone();
    let search_root =
        StubFolder::sectioned(vec![link_to("Async", move || Box::new(source.clone()))]);
    let executor = TestSpawner::new();
    let results = started(&search_root, "gear", &executor);
    executor.pump();
    assert!(async_source.load_started.get());

    // The folder is still loading at this point: only the placeholder is there to be found.
    assert_eq!(async_source.items().len(), 1);
    async_source.finish_load(&["async gear", "async bracket"]);
    run_to_end(&executor, &results);

    // A walk that does not wait for a late loading folder silently searches nothing at all.
    assert_eq!(names(&results.items()), sorted(&["async gear"]));
}

/// The wait takes its registrations off when it ends, so a search leaves nothing behind on a
/// folder the app keeps.
#[test]
fn the_walk_takes_its_listener_off_the_folder_it_waited_for() {
    let async_source = StubFolder::async_load("Async");
    let source = async_source.clone();
    let search_root =
        StubFolder::sectioned(vec![link_to("Async", move || Box::new(source.clone()))]);
    let executor = TestSpawner::new();
    let results = started(&search_root, "gear", &executor);
    executor.pump();
    assert_eq!(
        async_source.subscribers.get(),
        1,
        "the walk listens while it waits"
    );
    async_source.finish_load(&["async gear"]);
    run_to_end(&executor, &results);
    assert_eq!(names(&results.items()), sorted(&["async gear"]));
    assert_eq!(
        async_source.subscribers.get(),
        0,
        "the wait's listener is taken off when the wait ends"
    );
}

/// The native timer that bounds the wait for a late listing to 20 s ends as soon as the listing
/// arrives, rather than sleeping out the timeout. Native only: the browser starts no thread.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_walk_leaves_no_timer_thread_behind_once_the_listing_arrives() {
    use agg_gui_files::live_load_wait_timer_threads;

    let before = live_load_wait_timer_threads();
    let async_source = StubFolder::async_load("Async");
    let source = async_source.clone();
    let search_root =
        StubFolder::sectioned(vec![link_to("Async", move || Box::new(source.clone()))]);
    let executor = TestSpawner::new();
    let results = started(&search_root, "gear", &executor);
    executor.pump();
    assert!(
        live_load_wait_timer_threads() > before,
        "the wait is bounded by a timer"
    );
    async_source.finish_load(&["async gear"]);
    run_to_end(&executor, &results);

    // The timer ends right away, far inside the 20 s it would otherwise sleep.
    let started = std::time::Instant::now();
    while live_load_wait_timer_threads() > before && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(live_load_wait_timer_threads() <= before);
}

#[test]
fn disposing_a_results_folder_ends_a_running_search_without_disposing_what_it_was_searching() {
    let search_reached = Rc::new(Cell::new(false));
    let reached = Rc::clone(&search_reached);
    let slow_source = StubFolder::provider("Slow", move |_report, cancel| {
        reached.set(true);
        // Runs until cancelled, which is the state this test is about.
        Box::pin(async move {
            UntilCancelled(cancel).await;
            Ok(())
        })
    });
    let source = slow_source.clone();
    let search_root =
        StubFolder::sectioned(vec![link_to("Slow", move || Box::new(source.clone()))]);
    let executor = TestSpawner::new();
    let results = started(&search_root, "gear", &executor);
    executor.pump();
    assert!(search_reached.get());
    assert!(results.is_searching());

    results.dispose();
    run_to_end(&executor, &results);

    // A disposed results folder is not still searching.
    assert!(!results.is_searching());
    // The search must not tear down a folder it only borrowed: it is still whole.
    assert_eq!(slow_source.name(), "Slow");
    // A cancelled search is the user moving on, not a failure.
    assert!(executor.logs.borrow().is_empty());
}

#[test]
fn a_section_whose_search_fails_records_the_error_logs_it_and_leaves_the_other_sections_alone() {
    let broken_source = StubFolder::provider("Broken", |_report, _cancel| {
        Box::pin(async { Err("the drive went away".to_string()) })
    });
    let working_item: Rc<dyn Item> = Rc::new(StubItem("working gear".to_string()));
    let working_source = StubFolder::provider("Working", move |report, _cancel| {
        report(vec![Rc::clone(&working_item)]);
        Box::pin(async { Ok(()) })
    });
    let search_root = StubFolder::sectioned(vec![
        link_to("Broken", move || Box::new(broken_source.clone())),
        link_to("Working", move || Box::new(working_source.clone())),
    ]);
    let executor = TestSpawner::new();
    let results = started(&search_root, "gear", &executor);
    run_to_end(&executor, &results);

    let sections = results.sections();
    let broken = sections
        .iter()
        .find(|s| s.name() == "Broken")
        .expect("broken");
    let working = sections
        .iter()
        .find(|s| s.name() == "Working")
        .expect("working");
    assert!(broken.failed());
    assert!(broken
        .error_message()
        .is_some_and(|m| m.contains("the drive went away")));
    // A section that failed has stopped searching, and a view has to be able to say so.
    assert!(!broken.is_searching());
    assert_eq!(names(&working.items()), sorted(&["working gear"]));
    assert!(!results.is_searching());
    // The app's log gets the failure, under the search's category.
    assert_eq!(
        *executor.logs.borrow(),
        ["[LibrarySearch] Search of 'Broken' failed: the drive went away"]
    );
    // It was handled where it happened: nothing reports it as a fault nobody saw.
    assert!(executor.faults.borrow().is_empty());
}

#[test]
fn results_reported_by_two_sections_at_once_are_all_kept() {
    const BATCHES_PER_SECTION: usize = 200;
    fn chatty(name: &'static str) -> StubFolder {
        StubFolder::provider(name, move |report, _cancel| {
            Box::pin(async move {
                for i in 0..BATCHES_PER_SECTION {
                    report(vec![Rc::new(StubItem(format!("{name} gear {i}")))]);
                    // Hands the executor back between batches.
                    yield_now().await;
                }
                Ok(())
            })
        })
    }
    let search_root = StubFolder::sectioned(vec![
        link_to("Alpha", || Box::new(chatty("Alpha"))),
        link_to("Bravo", || Box::new(chatty("Bravo"))),
    ]);
    let results = search_to_end(&search_root, "gear");

    // Every batch reported has to reach the flattened list, however many sections report.
    assert_eq!(results.items().len(), BATCHES_PER_SECTION * 2);
    for section in results.sections() {
        assert_eq!(section.items().len(), BATCHES_PER_SECTION);
    }
}

#[test]
fn a_walked_result_remembers_the_folder_it_came_from_and_that_folders_parent() {
    let child = StubFolder::new("Child", &["child gear", "child bracket"]);
    let search_root = StubFolder::new("Root", &["root gear"]);
    let child_for_link = child.clone();
    search_root
        .children
        .borrow_mut()
        .push(link_to("Child", move || Box::new(child_for_link.clone())));
    let results = search_to_end(&search_root, "gear");

    let items = results.items();
    let child_result = items
        .iter()
        .find(|i| i.name() == "child gear")
        .expect("child");
    let root_result = items
        .iter()
        .find(|i| i.name() == "root gear")
        .expect("root");

    // Revealing the item means opening the folder it is actually in, not the section root.
    let child_origin = results.try_get_origin(child_result).expect("child origin");
    let origin_folder = child_origin.container().expect("a walked folder");
    assert_eq!(origin_folder.id(), child.id());
    assert!(Rc::ptr_eq(&origin_folder.items()[0], &child.items()[0]));
    // The root's own items came from the search root itself.
    let root_origin = results.try_get_origin(root_result).expect("root origin");
    assert!(root_origin.container().is_none());
    // The child's parent is the search root, which is the start of every origin's path.
    assert!(child_origin.parent().is_none());
    assert_eq!(child_origin.path().len(), 1);
    // Origin is additional to the section, not a replacement for it.
    let section = results.try_get_section(child_result).expect("section");
    assert_eq!(section.name(), "Root");
}

#[test]
fn thumbnail_requests_go_to_the_folder_the_result_came_from() {
    let mut child = StubFolder::new("Child", &["child gear"]);
    child.thumbnail = Some((8, 8));
    let search_root = StubFolder::new("Root", &[]);
    let child_for_link = child.clone();
    search_root
        .children
        .borrow_mut()
        .push(link_to("Child", move || Box::new(child_for_link.clone())));
    let results = search_to_end(&search_root, "gear");

    let items = results.items();
    let child_result = items
        .iter()
        .find(|i| i.name() == "child gear")
        .expect("child");
    let mut future = results.get_thumbnail(child_result, 64, 64).expect("routed");
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    let std::task::Poll::Ready(picture) = future.as_mut().poll(&mut cx) else {
        panic!("a ready picture");
    };
    let picture = picture.expect("no fault").expect("a picture");

    // Only the folder an item came from knows how to read that item's picture.
    assert_eq!((picture.width, picture.height), (8, 8));
}

#[test]
fn the_results_say_what_they_are_and_sort_by_name() {
    let results =
        SearchResultsFolder::new(&StubFolder::new("Root", &[]), "gear", TestSpawner::new());
    assert_eq!(results.name(), "Search Results");
    assert_eq!(results.query(), "gear");
    assert!(results.id().starts_with("librarySearchResults-"));
    assert!(
        !results.is_finished(),
        "a search not started has not finished"
    );
    assert!(results.default_sort().is_some());
}

/// The running search lives on its executor, so it holds the executor weakly: a strong hold
/// would keep both alive for as long as the search waited.
#[test]
fn a_waiting_search_does_not_keep_its_executor_alive() {
    let slow_source = StubFolder::provider("Slow", |_report, cancel| {
        Box::pin(async move {
            UntilCancelled(cancel).await;
            Ok(())
        })
    });
    let search_root =
        StubFolder::sectioned(vec![link_to("Slow", move || Box::new(slow_source.clone()))]);
    let executor = TestSpawner::new();
    let results = started(&search_root, "gear", &executor);
    executor.pump();
    assert!(results.is_searching(), "the search is waiting");
    // The test's handle and the results folder's: the waiting task adds none.
    assert_eq!(Rc::strong_count(&executor), 2);
    results.dispose();
    run_to_end(&executor, &results);
}

/// The content-file test can't reach a search already running, so a late call is a mistake a
/// debug build stops on.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "before the search starts")]
fn the_content_file_test_given_after_the_search_started_stops_a_debug_build() {
    let root = StubFolder::new("Root", &["gear box.stl"]);
    let results = started(&root, "stl", &TestSpawner::new());
    let _ = results.with_content_files(Rc::new(|_: &str| true));
}
