//! [`folder_listing`]: a folder loaded and listed, awaited. A reader that needs a folder's whole
//! listing (a copy reading a folder from another place) calls this rather than `load`, which for
//! a folder that lists later (a server's) only starts the listing. It loads the folder, then
//! waits on the folder's content-changed until the folder no longer says its load is pending
//! (`Folder::is_load_pending`): the folder trait's contract is that a folder answering pending
//! raises content-changed when its listing, or its failure, lands. A folder that lists within
//! `load` answers at the first poll. The listener comes off the folder when the listing is
//! answered or the wait is dropped.
//!
//! The wait is bounded ([`FOLDER_LISTING_TIMEOUT`], or the limit given to
//! [`folder_listing_within`]): a server that never answers would otherwise hold a copy open for
//! ever. Past the limit the wait answers [`ListingFailure::Other`]; a [`WakeTimer`] wakes it then
//! (in the browser there is none, and a fetch there always ends in an answer or a failure).

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use web_time::Instant;

use crate::folder::{ContentChangedListener, Folder, ListingFailure};
use crate::wake_timer::WakeTimer;

/// How long [`folder_listing`] waits for a folder's listing. Generous, because a copy that gives
/// up names the folder as not copied: a large server folder on a slow connection may take tens of
/// seconds to list, while a search (which only skips the folder) waits a third of this.
pub const FOLDER_LISTING_TIMEOUT: Duration = Duration::from_secs(60);

/// How many listing-wait timer threads are running (native): each lives only while its wait does.
static LIVE_LISTING_TIMERS: AtomicUsize = AtomicUsize::new(0);

/// The number of listing-wait timer threads still running (always 0 in the browser, which has
/// none).
pub fn live_folder_listing_timer_threads() -> usize {
    LIVE_LISTING_TIMERS.load(Ordering::SeqCst)
}

/// What the wait shares with the content-changed listener: the waker to wake when the folder
/// says its content changed.
#[derive(Default)]
struct Heard {
    waker: RefCell<Option<Waker>>,
}

/// The future [`folder_listing`] answers.
pub struct FolderListing {
    folder: Option<Box<dyn Folder>>,
    heard: Rc<Heard>,
    listener: Option<ContentChangedListener>,
    timeout: Duration,
    /// Set at the first poll, when the listing starts.
    deadline: Option<Instant>,
    /// Started with the listener; dropped with the wait, which ends its thread.
    timer: Option<WakeTimer>,
}

/// Loads `folder` and answers it once listed, or the kind of failure its listing ended in
/// (`Folder::listing_failure`), waiting at most [`FOLDER_LISTING_TIMEOUT`].
pub fn folder_listing(folder: Box<dyn Folder>) -> FolderListing {
    folder_listing_within(folder, FOLDER_LISTING_TIMEOUT)
}

/// [`folder_listing`], giving up with [`ListingFailure::Other`] once `timeout` has passed since
/// the listing started.
pub fn folder_listing_within(folder: Box<dyn Folder>, timeout: Duration) -> FolderListing {
    FolderListing {
        folder: Some(folder),
        heard: Rc::new(Heard::default()),
        listener: None,
        timeout,
        deadline: None,
        timer: None,
    }
}

impl FolderListing {
    /// Takes the listener back off the folder.
    fn stop_listening(&mut self) {
        if let (Some(folder), Some(listener)) = (&self.folder, self.listener.take()) {
            folder.unsubscribe_content_changed(&listener);
        }
    }
}

impl Future for FolderListing {
    type Output = Result<Box<dyn Folder>, ListingFailure>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Stored before anything can raise, so a listing landing during `load` is not missed.
        *self.heard.waker.borrow_mut() = Some(cx.waker().clone());
        if self.listener.is_none() {
            let heard = Rc::clone(&self.heard);
            let listener: ContentChangedListener = Rc::new(move || {
                if let Some(waker) = heard.waker.borrow().as_ref() {
                    waker.wake_by_ref();
                }
            });
            let Some(folder) = self.folder.as_mut() else {
                return Poll::Pending;
            };
            folder.subscribe_content_changed(Rc::clone(&listener));
            folder.load();
            self.listener = Some(listener);
            self.deadline = Some(Instant::now() + self.timeout);
        }
        let pending = self.folder.as_ref().is_none_or(|f| f.is_load_pending());
        let timed_out = self.deadline.is_some_and(|d| Instant::now() >= d);
        if pending && timed_out {
            self.stop_listening();
            self.timer = None;
            self.folder = None;
            return Poll::Ready(Err(ListingFailure::Other));
        }
        if pending {
            if self.timer.is_none() {
                self.timer = Some(WakeTimer::start(
                    cx.waker().clone(),
                    self.timeout,
                    &LIVE_LISTING_TIMERS,
                ));
            }
            return Poll::Pending;
        }
        self.stop_listening();
        self.timer = None;
        let Some(folder) = self.folder.take() else {
            return Poll::Pending;
        };
        match folder.listing_failure() {
            Some(failure) => Poll::Ready(Err(failure)),
            None => Poll::Ready(Ok(folder)),
        }
    }
}

impl Drop for FolderListing {
    fn drop(&mut self) {
        self.stop_listening();
    }
}
