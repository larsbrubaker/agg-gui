//! A folder that searches itself. A search across folders asks each folder it reaches for its
//! [`crate::Folder::search_provider`]; one that answers searches itself and everything beneath it
//! with whatever it is best at (a directory walk for a folder on disk, one query for a server);
//! the rest are walked item by item, matched with `search_terms`. Cancelling is [`SearchCancel`]:
//! a flag the caller sets and the search checks, which also wakes the futures waiting on it so a
//! cancelled search unwinds at once.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use crate::item::Item;

/// Takes one batch of matches.
pub type SearchReport = Rc<dyn Fn(Vec<Rc<dyn Item>>)>;

/// A provider's search running: resolves when it is done, or to the error that stopped it (which
/// a results folder records against the section).
pub type SearchFuture<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + 'a>>;

/// Shared by a search and whoever may stop it.
#[derive(Clone, Default)]
pub struct SearchCancel {
    cancelled: Rc<Cell<bool>>,
    wakers: Rc<RefCell<Vec<Waker>>>,
}

impl SearchCancel {
    /// A token nobody has cancelled yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the search has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.get()
    }

    /// Sets the flag and wakes every future waiting on it.
    pub fn cancel(&self) {
        if self.cancelled.replace(true) {
            return;
        }
        let wakers = std::mem::take(&mut *self.wakers.borrow_mut());
        for waker in wakers {
            waker.wake();
        }
    }

    /// `waker` is woken when the token is cancelled.
    pub fn register(&self, waker: &Waker) {
        let mut wakers = self.wakers.borrow_mut();
        if !wakers.iter().any(|known| known.will_wake(waker)) {
            wakers.push(waker.clone());
        }
    }
}

/// A folder's own search of itself and everything beneath it.
pub trait SearchProvider {
    /// Whether this instance answers for itself. A provider may decline for some of its
    /// instances; a decliner is walked like any other folder, not skipped.
    fn searches_itself(&self) -> bool {
        true
    }

    /// Searches this folder and its descendants for `query`, handing back matches through
    /// `report` in batches as they are found, and stopping promptly once `cancel` is set. Leaves
    /// the folder's own items untouched. A match may be a folder row as well as an item: a
    /// folder whose name matches, reported as its [`crate::FolderLink`] (which answers
    /// `as_container_link` and `into_container_link`), so the user can open it from the results.
    fn search_async<'a>(
        &'a self,
        query: &str,
        report: SearchReport,
        cancel: SearchCancel,
    ) -> SearchFuture<'a>;

    /// An owned handle on this same search, for a search that starts here: it reads its root
    /// when it is made and cannot keep borrowing it (the root is the browser's). A provider whose
    /// search is a walk of what it lists answers `None` (the default), and its root is walked the
    /// same way instead; a server's search is one query, and answers itself.
    fn shared_search(&self) -> Option<Rc<dyn SearchProvider>> {
        None
    }
}

/// One trip back to the executor: lets a long search on the UI thread's executor give the frame
/// (and the work it waits for) a turn between steps, so a long search never stalls the UI.
pub fn yield_now() -> impl Future<Output = ()> {
    YieldOnce(false)
}

/// Pending once (waking itself at once), then ready.
struct YieldOnce(bool);

impl Future for YieldOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}
