//! How a [`crate::SearchResultsFolder`] walks one section (`search_results`). Each source is
//! walked: its matching items (`search_terms`), then its child folders, at most
//! [`MAX_FALLBACK_DEPTH`] levels down. A folder whose listing arrives after `load`
//! ([`Folder::is_load_pending`]: a server folder) is waited for, at most [`ASYNC_LOAD_TIMEOUT`]
//! (20 s), then searched as it stands; the wait's deadline is a [`WakeTimer`] (`wake_timer`). A
//! folder that searches itself ([`Folder::search_provider`]: a folder on disk) is not walked: it
//! reports its own matches. A section that fails records its message, and the spawner logs it
//! (when the spawner is still there: the walk holds it weakly, as it runs on it);
//! the other sections go on.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use web_time::Instant;

use crate::folder::{ContentChangedListener, Folder};
use crate::item::Item;
use crate::message_item::MessageItem;
use crate::search::{SearchCancel, SearchReport};
use crate::search_results::{RootSnapshot, SearchOrigin, SearchSection, SearchShared};
use crate::search_terms::name_matches;
use crate::spawner::Spawner;
use crate::wake_timer::WakeTimer;

/// How far the walk goes below a section's folder. A blind walk of an unknown folder is
/// expensive and can loop, so it stops well short of forever.
pub const MAX_FALLBACK_DEPTH: usize = 5;

/// How long the walk waits for a folder that only starts its load in `load`. Long enough for a
/// cold server listing over a slow connection; on timeout the walk uses whatever arrived.
pub const ASYNC_LOAD_TIMEOUT: Duration = Duration::from_millis(20000);

/// The log category a failed section is logged under.
const LOG_CATEGORY: &str = "LibrarySearch";

/// Resolves and searches one section, recording what goes wrong rather than failing the other
/// sections.
pub(crate) async fn search_section(
    shared: &Rc<SearchShared>,
    section: &Rc<SearchSection>,
    root: &RootSnapshot,
    spawner: &Weak<dyn Spawner>,
) {
    let outcome = match section.source_link() {
        Some(link) => match link.get_container() {
            Ok(folder) => search_container(shared, section, folder, Vec::new(), 0).await,
            Err(message) => Err(message),
        },
        // The root searches itself: everything it reports was found in the root.
        None if root.provider.is_some() => {
            let report_shared = Rc::clone(shared);
            let report_section = Rc::clone(section);
            let report: SearchReport = Rc::new(move |batch| {
                let origin = SEARCH_ROOT_ORIGIN.with(Rc::clone);
                report_shared.add_results(&report_section, &origin, batch);
            });
            match &root.provider {
                Some(provider) => {
                    provider
                        .search_async(&shared.query, report, shared.cancel.clone())
                        .await
                }
                None => Ok(()),
            }
        }
        None => {
            // The root is the browser's, already loaded: its items, then its children.
            let matches = matching(&root.items, shared);
            shared.add_results(section, &SEARCH_ROOT_ORIGIN.with(Rc::clone), matches);
            let mut outcome = Ok(());
            for link in &root.child_containers {
                if shared.cancel.is_cancelled() {
                    break;
                }
                if let Ok(child) = link.get_container() {
                    outcome = search_container(shared, section, child, Vec::new(), 1).await;
                    if outcome.is_err() {
                        break;
                    }
                }
            }
            outcome
        }
    };
    // A cancelled search is the user moving on: not an error, and nothing to report.
    if let Err(message) = outcome {
        if !shared.cancel.is_cancelled() {
            // The results folder holds the spawner while it is not disposed, and a disposed search
            // is cancelled, so the spawner is there here; should it be gone, there is no log.
            if let Some(spawner) = spawner.upgrade() {
                spawner.log_error(
                    LOG_CATEGORY,
                    &format!("Search of '{}' failed: {message}", section.name()),
                );
            }
            *section.error_message.borrow_mut() = Some(message);
        }
    }
    section.is_searching.set(false);
    shared.notify();
}

thread_local! {
    /// The origin of everything found in the search root's own items.
    static SEARCH_ROOT_ORIGIN: Rc<SearchOrigin> = Rc::new(SearchOrigin { path: Vec::new() });
}

/// The items of `items` whose names match the search's query, a content file's extension
/// dropped.
fn matching(items: &[Rc<dyn Item>], shared: &SearchShared) -> Vec<Rc<dyn Item>> {
    let content_file = shared.content_file.as_deref();
    items
        .iter()
        .filter(|item| name_matches(item.name(), shared.query.as_str(), content_file))
        .cloned()
        .collect()
}

/// A folder that searches itself (and says that search answers for this instance) does; any
/// other is loaded if empty, waited for if its listing is still coming, its matching items
/// taken, and its children walked (a sectioned folder's sections only: a computer's top level
/// would otherwise walk every drive). `parents` are the walked folders above it, below the search
/// root: what gives a result's origin its breadcrumb trail.
fn search_container<'a>(
    shared: &'a Rc<SearchShared>,
    section: &'a Rc<SearchSection>,
    mut folder: Box<dyn Folder>,
    parents: Vec<Rc<dyn Folder>>,
    depth: usize,
) -> Pin<Box<dyn Future<Output = Result<(), String>> + 'a>> {
    Box::pin(async move {
        if shared.cancel.is_cancelled() {
            return Ok(());
        }
        let searches_itself = folder
            .search_provider()
            .is_some_and(|provider| provider.searches_itself());
        if !searches_itself {
            if depth > MAX_FALLBACK_DEPTH {
                return Ok(());
            }
            if !already_has_content(folder.as_ref()) {
                folder.load();
            }
        }
        let folder: Rc<dyn Folder> = Rc::from(folder);
        let mut path = parents;
        path.push(Rc::clone(&folder));
        let origin = Rc::new(SearchOrigin { path: path.clone() });
        if searches_itself {
            if let Some(provider) = folder.search_provider() {
                // The folder that searched itself is the origin of everything it reports.
                let report_shared = Rc::clone(shared);
                let report_section = Rc::clone(section);
                let report: SearchReport = Rc::new(move |batch| {
                    report_shared.add_results(&report_section, &origin, batch);
                });
                return provider
                    .search_async(&shared.query, report, shared.cancel.clone())
                    .await;
            }
        }
        // Waited on even when this walk did not start the load: reading straight after it finds
        // only the "Loading..." placeholder.
        if folder.is_load_pending() {
            WaitForLoad::new(folder.as_ref(), &shared.cancel).await;
        }
        shared.add_results(section, &origin, matching(&folder.items(), shared));
        // Snapshotted before the walk: resolving a child can reload the folder that listed it.
        let child_links = folder
            .search_sections()
            .unwrap_or_else(|| folder.child_containers());
        for link in child_links {
            if shared.cancel.is_cancelled() {
                return Ok(());
            }
            // A child that cannot be opened has no results in it; the walk goes on.
            if let Ok(child) = link.get_container() {
                search_container(shared, section, child, path.clone(), depth + 1).await?;
            }
        }
        Ok(())
    })
}

/// Placeholders ("Loading...", "Cannot access directory") are not content, or the folders this
/// walk most needs to load would never be.
fn already_has_content(folder: &dyn Folder) -> bool {
    !folder.child_containers().is_empty()
        || folder
            .items()
            .iter()
            .any(|item| item.as_any().is_none_or(|any| !any.is::<MessageItem>()))
}

/// Waits until the folder's listing arrives, the search is cancelled, or [`ASYNC_LOAD_TIMEOUT`]
/// passes. It sleeps between checks: the folder's content-changed (raised when a listing or its
/// failure lands) and the cancel token wake it, and a [`WakeTimer`] wakes it at the deadline (in
/// the browser there is none; a fetch there ends in success or failure, either of which raises
/// content-changed). The content-changed listener comes off the folder and the timer thread ends
/// when the wait ends, however it ends.
struct WaitForLoad<'a> {
    folder: &'a dyn Folder,
    cancel: &'a SearchCancel,
    deadline: Instant,
    /// The waker the folder's content-changed wakes; shared weakly with the listener, so a raise
    /// after the wait is gone finds nothing to wake.
    slot: Rc<RefCell<Option<Waker>>>,
    /// The listener subscribed to the folder's content-changed once the wait first sleeps.
    listener: Option<ContentChangedListener>,
    /// Started with the listener; dropped with the wait, which ends its thread.
    timer: Option<WakeTimer>,
}

/// How many async-load wait timer threads are running (native): each lives only while its wait
/// does.
static LIVE_LOAD_WAIT_TIMERS: AtomicUsize = AtomicUsize::new(0);

/// The number of async-load wait timer threads still running (always 0 in the browser, which
/// has none).
pub fn live_load_wait_timer_threads() -> usize {
    LIVE_LOAD_WAIT_TIMERS.load(Ordering::SeqCst)
}

impl<'a> WaitForLoad<'a> {
    fn new(folder: &'a dyn Folder, cancel: &'a SearchCancel) -> Self {
        Self {
            folder,
            cancel,
            deadline: Instant::now() + ASYNC_LOAD_TIMEOUT,
            slot: Rc::default(),
            listener: None,
            timer: None,
        }
    }
}

impl Drop for WaitForLoad<'_> {
    /// Unsubscribes: the wait leaves nothing behind on a folder the app keeps.
    fn drop(&mut self) {
        if let Some(listener) = self.listener.take() {
            self.folder.unsubscribe_content_changed(&listener);
        }
    }
}

impl Future for WaitForLoad<'_> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if !self.folder.is_load_pending()
            || self.cancel.is_cancelled()
            || Instant::now() >= self.deadline
        {
            return Poll::Ready(());
        }
        *self.slot.borrow_mut() = Some(cx.waker().clone());
        self.cancel.register(cx.waker());
        if self.listener.is_none() {
            let slot = Rc::downgrade(&self.slot);
            let listener: ContentChangedListener = Rc::new(move || {
                if let Some(waker) = slot.upgrade().and_then(|slot| slot.borrow_mut().take()) {
                    waker.wake();
                }
            });
            self.folder.subscribe_content_changed(Rc::clone(&listener));
            self.listener = Some(listener);
            self.timer = Some(WakeTimer::start(
                cx.waker().clone(),
                ASYNC_LOAD_TIMEOUT,
                &LIVE_LOAD_WAIT_TIMERS,
            ));
        }
        Poll::Pending
    }
}
