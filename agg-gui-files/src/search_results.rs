//! [`SearchResultsFolder`]: the results of one search across folders, with its sections. A
//! folder, so a list view shows it unchanged: `items` holds every match found so far, flattened,
//! and grows while the search runs; `sections` groups the same matches by the source they came
//! from. A search root that presents itself in sections ([`Folder::search_sections`]) gets one
//! [`SearchSection`] per source; anything else is one section, itself. How each source is walked
//! is `search_walk`'s: its matching items, then its child folders, a folder that searches itself
//! left to do so, a folder whose listing comes later waited for. Every result remembers where it
//! was found ([`SearchOrigin`]), which is what reveals an item with no path of its own and what
//! asks that folder for the item's picture (`get_thumbnail`).
//!
//! Which names carry an extension worth dropping before matching (the file types the app opens,
//! so "stl" does not match every mesh) is the app's to say: [`SearchResultsFolder::with_content_files`]
//! hands the walk that test, and without it names are matched whole.
//!
//! The search is one local future on the [`Spawner`] it is given (the app's executor, pumped
//! each frame), so sections run one after another and need no lock, and content-changed is
//! raised as results land (already on the caller's thread, so there is nothing to coalesce). A
//! section that fails records its message and is logged through `Spawner::log_error`. The search
//! root is read when the search is made (its name, sections, items and child links): it is the
//! folder the browser is showing, already loaded and owned by the browser. Disposing (`dispose`,
//! or dropping the folder) cancels the search; what was found stays. A root that searches itself
//! runs its own search through the owned handle it hands out ([`SearchProvider::shared_search`]:
//! a server's query); one that hands out none is walked instead. A folder's parent is the path
//! above it (crate header), so a result's parent is [`SearchOrigin::parent`], and the results'
//! own parent (the root) is the navigator's to keep.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::folder::{ContentChangedEvent, ContentChangedListener, Folder};
use crate::item::{FolderLink, Item, PictureFuture};
use crate::search::{SearchCancel, SearchProvider};
use crate::search_walk::search_section;
use crate::sort::{SortBehavior, SortKey};
use crate::spawner::Spawner;
use crate::translate::tr;

/// One source's results, and whether it is still looking or gave up.
pub struct SearchSection {
    name: String,
    source_link: Option<Rc<dyn FolderLink>>,
    pub(crate) is_searching: Cell<bool>,
    pub(crate) error_message: RefCell<Option<String>>,
    pub(crate) items: RefCell<Vec<Rc<dyn Item>>>,
}

impl SearchSection {
    fn new(name: String, source_link: Option<Rc<dyn FolderLink>>) -> Rc<Self> {
        Rc::new(Self {
            name,
            source_link,
            is_searching: Cell::new(true),
            error_message: RefCell::new(None),
            items: RefCell::new(Vec::new()),
        })
    }

    /// The link's name, as a folder tree shows it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The link searched through, `None` when the root was searched directly.
    pub fn source_link(&self) -> Option<&Rc<dyn FolderLink>> {
        self.source_link.as_ref()
    }

    /// True until this section's search finishes, fails or is cancelled.
    pub fn is_searching(&self) -> bool {
        self.is_searching.get()
    }

    /// Why this section has no results, `None` if nothing went wrong.
    pub fn error_message(&self) -> Option<String> {
        self.error_message.borrow().clone()
    }

    /// Whether this section's search failed.
    pub fn failed(&self) -> bool {
        self.error_message.borrow().is_some()
    }

    /// The matches found so far, as a snapshot.
    pub fn items(&self) -> Vec<Rc<dyn Item>> {
        self.items.borrow().clone()
    }
}

/// Where a result was found. `path` runs from the first folder below the search root down to the
/// one the result came from; empty for the root's own items.
pub struct SearchOrigin {
    pub(crate) path: Vec<Rc<dyn Folder>>,
}

impl SearchOrigin {
    /// The folder the result was found in; `None` when it was the search root itself.
    pub fn container(&self) -> Option<&Rc<dyn Folder>> {
        self.path.last()
    }

    /// The folder above it; `None` when that is the search root (or the result came from the
    /// root itself).
    pub fn parent(&self) -> Option<&Rc<dyn Folder>> {
        self.path.len().checked_sub(2).map(|i| &self.path[i])
    }

    /// The walked folders from below the search root down to the origin: its breadcrumbs.
    pub fn path(&self) -> &[Rc<dyn Folder>] {
        &self.path
    }
}

/// What the search root looked like when the search was made.
pub(crate) struct RootSnapshot {
    pub(crate) name: String,
    pub(crate) sections: Option<Vec<Rc<dyn FolderLink>>>,
    pub(crate) items: Vec<Rc<dyn Item>>,
    pub(crate) child_containers: Vec<Rc<dyn FolderLink>>,
    /// The root's own search, when it answers for itself and hands one out.
    pub(crate) provider: Option<Rc<dyn SearchProvider>>,
}

/// Whether a name is a file the app opens (`gear box.stl`), whose extension a search drops.
pub type ContentFileTest = Rc<dyn Fn(&str) -> bool>;

/// One result and where it was found.
type ItemOrigin = (Rc<dyn Item>, Rc<SearchOrigin>);

/// The state the running search and the folder share.
pub(crate) struct SearchShared {
    pub(crate) query: String,
    /// Which names are content files, whose extension a match drops (`with_content_files`).
    pub(crate) content_file: Option<ContentFileTest>,
    items: RefCell<Vec<Rc<dyn Item>>>,
    sections: RefCell<Vec<Rc<SearchSection>>>,
    is_searching: Cell<bool>,
    started: Cell<bool>,
    /// Where each result was found, by item identity.
    origins: RefCell<Vec<ItemOrigin>>,
    /// Set when the search is cancelled.
    pub(crate) cancel: SearchCancel,
    /// Set once disposed: no more content-changed once the view has let go.
    disposed: Cell<bool>,
    content_changed: ContentChangedEvent,
    /// How many of this search's wait timer threads are running (`search_walk`'s `WaitForLoad`);
    /// shared with the threads, which count themselves out as they end.
    pub(crate) load_wait_timers: Arc<AtomicUsize>,
}

impl SearchShared {
    /// Raises content-changed unless disposed.
    pub(crate) fn notify(&self) {
        if !self.disposed.get() {
            self.content_changed.raise();
        }
    }

    /// Files a batch under its section and appends it to the flattened list.
    pub(crate) fn add_results(
        &self,
        section: &SearchSection,
        origin: &Rc<SearchOrigin>,
        batch: Vec<Rc<dyn Item>>,
    ) {
        if batch.is_empty() {
            return;
        }
        section.items.borrow_mut().extend(batch.iter().cloned());
        self.origins.borrow_mut().extend(
            batch
                .iter()
                .map(|item| (Rc::clone(item), Rc::clone(origin))),
        );
        self.items.borrow_mut().extend(batch);
        self.notify();
    }
}

/// The results of one search, as a folder.
pub struct SearchResultsFolder {
    id: String,
    name: String,
    root: Option<RootSnapshot>,
    spawner: Rc<dyn Spawner>,
    shared: Rc<SearchShared>,
}

/// Each results folder's ID is its own in this run (it is never stored).
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl SearchResultsFolder {
    /// A search of `search_root` (read now) for `query`, run on `spawner` once started.
    pub fn new(search_root: &dyn Folder, query: &str, spawner: Rc<dyn Spawner>) -> Self {
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            id: format!("librarySearchResults-{id}"),
            name: tr("Search Results"),
            root: Some(RootSnapshot {
                name: search_root.name().to_string(),
                sections: search_root.search_sections(),
                items: search_root.items(),
                child_containers: search_root.child_containers(),
                provider: search_root
                    .search_provider()
                    .filter(|provider| provider.searches_itself())
                    .and_then(|provider| provider.shared_search()),
            }),
            spawner,
            shared: Rc::new(SearchShared {
                query: query.to_string(),
                content_file: None,
                items: RefCell::new(Vec::new()),
                sections: RefCell::new(Vec::new()),
                is_searching: Cell::new(false),
                started: Cell::new(false),
                origins: RefCell::new(Vec::new()),
                cancel: SearchCancel::new(),
                disposed: Cell::new(false),
                content_changed: ContentChangedEvent::default(),
                load_wait_timers: Arc::default(),
            }),
        }
    }

    /// The search, with `is_content_file` naming the files whose extension a match drops (see
    /// [`crate::name_matches`]).
    ///
    /// Call it before `start_search` (or `load`): a debug build stops on a call after, and a
    /// release build leaves the running search as it began.
    pub fn with_content_files(mut self, is_content_file: ContentFileTest) -> Self {
        debug_assert!(
            !self.shared.started.get(),
            "with_content_files is called before the search starts"
        );
        // Only the folder holds the shared state until the search starts.
        if let Some(shared) = Rc::get_mut(&mut self.shared) {
            shared.content_file = Some(is_content_file);
        }
        self
    }

    /// The query searched for.
    pub fn query(&self) -> &str {
        &self.shared.query
    }

    /// How many timer threads bounding this search's waits for a late listing are running
    /// (always 0 in the browser, which has none). Each ends with its wait, so a search that has
    /// run to its end soon has none. Only this search's: other searches do not move it, unlike
    /// [`crate::live_load_wait_timer_threads`], which counts the whole process.
    pub fn load_wait_timer_threads(&self) -> usize {
        self.shared.load_wait_timers.load(Ordering::SeqCst)
    }

    /// From the start until the last section finishes.
    pub fn is_searching(&self) -> bool {
        self.shared.is_searching.get()
    }

    /// Whether the search has run to its end (finished, or cancelled and unwound).
    pub fn is_finished(&self) -> bool {
        self.shared.started.get() && !self.shared.is_searching.get()
    }

    /// One per searched source, in the order the root listed them.
    pub fn sections(&self) -> Vec<Rc<SearchSection>> {
        self.shared.sections.borrow().clone()
    }

    /// The section `item` was filed under, by identity, then by ID.
    pub fn try_get_section(&self, item: &Rc<dyn Item>) -> Option<Rc<SearchSection>> {
        let sections = self.shared.sections.borrow();
        let by_identity = sections.iter().find(|section| {
            section
                .items
                .borrow()
                .iter()
                .any(|known| std::ptr::addr_eq(Rc::as_ptr(known), Rc::as_ptr(item)))
        });
        if let Some(section) = by_identity {
            return Some(Rc::clone(section));
        }
        if item.id().is_empty() {
            return None;
        }
        sections
            .iter()
            .find(|section| section.items.borrow().iter().any(|k| k.id() == item.id()))
            .cloned()
    }

    /// Starts the search, or does nothing when it already started.
    pub fn start_search(&mut self) {
        if self.shared.started.replace(true) {
            return;
        }
        let Some(root) = self.root.take() else {
            return;
        };
        let sections = match &root.sections {
            Some(links) => links
                .iter()
                .map(|link| SearchSection::new(link.name().to_string(), Some(link.clone())))
                .collect(),
            None => vec![SearchSection::new(root.name.clone(), None)],
        };
        *self.shared.sections.borrow_mut() = sections.clone();
        self.shared.is_searching.set(true);
        let shared = Rc::clone(&self.shared);
        // Weak: the executor keeps this task, so a strong handle on the executor here would keep
        // both alive while a search waits.
        let spawner = Rc::downgrade(&self.spawner);
        self.spawner.spawn_task(Box::pin(async move {
            for section in sections {
                search_section(&shared, &section, &root, &spawner).await;
            }
            shared.is_searching.set(false);
            shared.notify();
        }));
    }

    /// Stops the search; what was already found stays.
    pub fn cancel(&self) {
        self.shared.cancel.cancel();
    }

    /// Where `item` (by identity) was found.
    pub fn try_get_origin(&self, item: &Rc<dyn Item>) -> Option<Rc<SearchOrigin>> {
        self.shared
            .origins
            .borrow()
            .iter()
            .find(|(known, _)| std::ptr::addr_eq(Rc::as_ptr(known), Rc::as_ptr(item)))
            .map(|(_, origin)| Rc::clone(origin))
    }

    /// Cancels, and no more content-changed reaches a view that has let go. The folders the
    /// search walked are not touched (a link may hand out the app's own).
    pub fn dispose(&self) {
        self.shared.disposed.set(true);
        self.cancel();
    }
}

impl Drop for SearchResultsFolder {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl Folder for SearchResultsFolder {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_protected(&self) -> bool {
        true
    }

    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        Vec::new()
    }

    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.shared.items.borrow().clone()
    }

    /// Starts the search the first time.
    fn load(&mut self) {
        self.start_search();
    }

    /// Results arrive in the order they are found, which is no order to a user reading a list.
    fn default_sort(&self) -> Option<SortBehavior> {
        Some(SortBehavior::new(SortKey::Name, true))
    }

    fn subscribe_content_changed(&self, listener: ContentChangedListener) {
        self.shared.content_changed.subscribe(listener);
    }

    fn unsubscribe_content_changed(&self, listener: &ContentChangedListener) {
        self.shared.content_changed.unsubscribe(listener);
    }

    /// Lets a caller tell a search's results from other folders.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    /// Asks the folder the item was found in, the only one that knows how to read its items'
    /// pictures (a server folder's picture addresses). An item from the search root itself has
    /// no folder here to ask (`None`): the browser asks the root as it shows it.
    fn get_thumbnail(&self, item: &Rc<dyn Item>, width: i32, height: i32) -> Option<PictureFuture> {
        let origin = self.try_get_origin(item)?;
        origin.container()?.get_thumbnail(item, width, height)
    }
}
