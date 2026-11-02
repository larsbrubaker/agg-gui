//! [`SearchController`]: search for one browser view. It turns what the user types into a
//! `SearchResultsFolder` shown as the browser's showing folder
//! (`BrowserState::push_search_results`, whose results list in sections as they arrive), and puts
//! the view back where it was when the search is cleared or abandoned. The search box itself is `search_box.rs`.
//!
//! Typing is debounced: a keystroke sets a deadline [`DEBOUNCE`] out on agg-gui's UI clock and
//! queues a laid-out frame for then, and the view calls [`SearchController::poll`] from its
//! layout, which runs the search once the deadline has passed. The wake comes due on the same
//! clock as the deadline, so the frame it brings always finds the deadline passed. Layout, not
//! paint, because a frame can lay the browser out without painting it. A later keystroke moves the
//! deadline and queues its own wake, so only the last keystroke's search runs. Enter runs what is
//! typed at once ([`SearchController::flush`]).
//!
//! A new search replaces the one before and disposes it, so a search still running for a query
//! the user has typed past stops reading and never reports into the view; refining a query
//! searches the folder the results came from again, not the last results. Clearing the search
//! returns to the folder searched with what was selected there when the search began. The state's
//! container-changed listener ends the search when the user walks away from its results (a
//! breadcrumb, opening a folder) and the controller isn't the one navigating.
//!
//! What a search is made with is the controller's [`SearchMaker`] (by default
//! `SearchResultsFolder::new`; an app gives its own to tell the search which names are content
//! files), run on the config's spawner.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use agg_gui_files::{Folder, SearchResultsFolder, Spawner};
use web_time::Instant;

use crate::state::BrowserState;

/// How long typing has to stop before the search runs: long enough that typing a word does not
/// start a search per letter, short enough that the results follow the keyboard.
pub const DEBOUNCE: Duration = Duration::from_millis(300);

/// Runs when the search stops being what the view shows.
pub type SearchEndedListener = Rc<dyn Fn()>;

/// Makes the search of `root` for `query`, its work run on the spawner.
pub type SearchMaker = Rc<dyn Fn(&dyn Folder, &str, Rc<dyn Spawner>) -> SearchResultsFolder>;

#[derive(Default)]
struct Inner {
    /// The depth in the browser's path of the folder the current search covers: where the user
    /// was when they started typing.
    search_root_depth: Option<usize>,
    /// What was selected in the folder searched when the search began (identity keys), selected
    /// again when the search is cleared back to it.
    root_selection: Vec<String>,
    results: Option<Rc<SearchResultsFolder>>,
    pending_query: Option<String>,
    /// When the pending query runs, on agg-gui's UI clock, unless Enter or another keystroke
    /// comes first.
    deadline: Option<Instant>,
    search_ended: Vec<SearchEndedListener>,
    disposed: bool,
}

/// Searches one browser view and puts it back when the search ends (see the module header).
pub struct SearchController {
    state: Rc<RefCell<BrowserState>>,
    inner: Rc<RefCell<Inner>>,
    maker: SearchMaker,
    /// Set while this controller changes the showing folder, so its own navigation is not taken
    /// for the user walking away from the search.
    navigating: Rc<Cell<bool>>,
}

impl SearchController {
    /// A controller over the browser `state`, making plain searches.
    pub fn new(state: &Rc<RefCell<BrowserState>>) -> Rc<Self> {
        Self::with_maker(
            state,
            Rc::new(|root: &dyn Folder, query: &str, spawner| {
                SearchResultsFolder::new(root, query, spawner)
            }),
        )
    }

    /// A controller over the browser `state`, making its searches with `maker`.
    pub fn with_maker(state: &Rc<RefCell<BrowserState>>, maker: SearchMaker) -> Rc<Self> {
        let inner = Rc::new(RefCell::new(Inner::default()));
        let navigating = Rc::new(Cell::new(false));
        let weak_inner: Weak<RefCell<Inner>> = Rc::downgrade(&inner);
        let flag = Rc::clone(&navigating);
        state
            .borrow_mut()
            .subscribe_container_changed(Rc::new(move |active_id: &str| {
                let Some(inner) = weak_inner.upgrade() else {
                    return false;
                };
                if !flag.get() {
                    container_changed(&inner, active_id);
                }
                true
            }));
        Rc::new(Self {
            state: Rc::clone(state),
            inner,
            maker,
            navigating,
        })
    }

    /// `listener` runs when the search stops being what the view shows: cleared, or walked away
    /// from. The search box blanks itself on this.
    pub fn subscribe_search_ended(&self, listener: SearchEndedListener) {
        self.inner.borrow_mut().search_ended.push(listener);
    }

    /// Whether a search's results are what the view shows.
    pub fn is_search_active(&self) -> bool {
        self.inner.borrow().results.is_some()
    }

    /// The current search's results, if any.
    pub fn current_results(&self) -> Option<Rc<SearchResultsFolder>> {
        self.inner.borrow().results.clone()
    }

    /// Takes what is typed now. Blank clears the search at once; anything else runs after a
    /// short pause in typing (`poll`).
    pub fn set_query(&self, text: &str) {
        let query = text.trim();
        if query.is_empty() {
            self.clear();
            return;
        }
        let mut inner = self.inner.borrow_mut();
        inner.pending_query = Some(query.to_string());
        inner.deadline = Some(wake_layout_after(DEBOUNCE));
    }

    /// Runs the pending query once typing has paused for [`DEBOUNCE`]; the view calls this from
    /// its layout.
    pub fn poll(&self) {
        let due = self
            .inner
            .borrow()
            .deadline
            .is_some_and(|at| agg_gui::clock::now() >= at);
        if due {
            self.run_pending_search();
        }
    }

    /// Runs what is typed now without waiting out the pause: what Enter does.
    pub fn flush(&self) {
        let pending = self
            .inner
            .borrow()
            .pending_query
            .as_ref()
            .is_some_and(|q| !q.trim().is_empty());
        if pending {
            self.run_pending_search();
        }
    }

    /// Ends the search and returns the view to the folder that was searched.
    pub fn clear(&self) {
        self.end_search(true);
    }

    /// Ends the search without telling the listeners (they go with the view).
    pub fn dispose(&self) {
        self.end_search(false);
        self.inner.borrow_mut().disposed = true;
    }

    /// Ends the search (a view still showing its results goes back to the searched folder) and
    /// tells the listeners when `notify`.
    fn end_search(&self, notify: bool) {
        let (closing, root_depth, selection, listeners) = {
            let mut inner = self.inner.borrow_mut();
            // Anything still queued is for a query the user has taken back.
            inner.pending_query = None;
            inner.deadline = None;
            let closing = inner.results.take();
            let root_depth = inner.search_root_depth.take();
            let selection = std::mem::take(&mut inner.root_selection);
            (closing, root_depth, selection, inner.search_ended.clone())
        };
        let Some(closing) = closing else {
            return;
        };
        if let (Some(depth), Ok(mut state)) = (root_depth, self.state.try_borrow_mut()) {
            let showing = state
                .showing_search_results()
                .is_some_and(|r| Rc::ptr_eq(r, &closing));
            if showing {
                self.navigating.set(true);
                state.go_to(depth);
                self.navigating.set(false);
                // Back where the search began, with what was selected there.
                if !selection.is_empty() {
                    state.select_when_listed(selection);
                }
            }
        }
        // Disposed, not just cancelled: that is what stops a search still unwinding from
        // raising content changes at a view that has moved on.
        closing.dispose();
        crate::trace(|| format!("search for '{}' ended", closing.query()));
        if notify {
            for listener in listeners {
                listener();
            }
        }
    }

    /// Runs the query typed so far.
    fn run_pending_search(&self) {
        let query = {
            let mut inner = self.inner.borrow_mut();
            if inner.disposed {
                return;
            }
            inner.deadline = None;
            // Taken, so Enter twice on unchanged text does not run it all again.
            inner.pending_query.take()
        };
        let Some(query) = query.filter(|q| !q.trim().is_empty()) else {
            self.clear();
            return;
        };
        let previous = self.inner.borrow().results.clone();
        let spawner = Rc::clone(&self.state.borrow().config().spawner);
        let results = {
            let mut state = self.state.borrow_mut();
            // The search root: where the user is, unless that is a set of search results, in
            // which case the folder they came from. Refining narrows the same search.
            self.navigating.set(true);
            let refining = state.showing_search_results().is_some();
            if refining {
                let depth = state.depth();
                state.go_to(depth.saturating_sub(2));
            } else {
                let keys = state
                    .selected_entries()
                    .iter()
                    .map(|e| e.identity_key())
                    .collect();
                self.inner.borrow_mut().root_selection = keys;
            }
            let root_depth = state.depth() - 1;
            let made = state
                .active_container()
                .map(|root| (self.maker)(&*root, &query, Rc::clone(&spawner)));
            let Some(mut results) = made else {
                self.navigating.set(false);
                return;
            };
            results.start_search();
            let results = Rc::new(results);
            state.push_search_results(Rc::clone(&results));
            self.navigating.set(false);
            let mut inner = self.inner.borrow_mut();
            inner.search_root_depth = Some(root_depth);
            inner.results = Some(Rc::clone(&results));
            results
        };
        crate::trace(|| format!("search for '{query}' runs"));
        // The refined query replaces the one before: the abandoned search stops reading.
        if let Some(previous) = previous {
            if !Rc::ptr_eq(&previous, &results) {
                previous.dispose();
            }
        }
    }
}

impl Drop for SearchController {
    fn drop(&mut self) {
        self.dispose();
    }
}

/// Queues a laid-out frame `delay` from now, for `poll` to run the pending query in, and answers
/// when it comes due on agg-gui's UI clock: the query's deadline. A timed draw would not do: a
/// shell lays a frame out only when asked to, and a delayed idle action runs at the start of the
/// frame it comes due in, before the shell decides on layout.
fn wake_layout_after(delay: Duration) -> Instant {
    let at = agg_gui::clock::now() + delay;
    agg_gui::ui_thread::run_on_idle_after(delay, agg_gui::animation::request_layout);
    at
}

/// The showing folder changed: the user walked away from the results, so the search stops here
/// rather than running on against a view that has moved on.
fn container_changed(inner: &Rc<RefCell<Inner>>, active_id: &str) {
    let (closing, listeners) = {
        let Ok(mut inner) = inner.try_borrow_mut() else {
            return;
        };
        match &inner.results {
            None => return,
            Some(results) if results.id() == active_id => return,
            Some(_) => {}
        }
        inner.pending_query = None;
        inner.deadline = None;
        inner.search_root_depth = None;
        (inner.results.take(), inner.search_ended.clone())
    };
    if let Some(closing) = closing {
        closing.dispose();
    }
    for listener in listeners {
        listener();
    }
}
