//! [`folder_listing`]: a folder loaded and listed, awaited. A reader that needs a folder's whole
//! listing (a copy reading a folder from another place) calls this rather than `load`, which for
//! a folder that lists later (a server's) only starts the listing. It loads the folder, then
//! waits on the folder's content-changed until the folder no longer says its load is pending
//! (`Folder::is_load_pending`): the folder trait's contract is that a folder answering pending
//! raises content-changed when its listing, or its failure, lands. A folder that lists within
//! `load` answers at the first poll. The listener comes off the folder when the listing is
//! answered or the wait is dropped.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use crate::folder::{ContentChangedListener, Folder, ListingFailure};

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
}

/// Loads `folder` and answers it once listed, or the kind of failure its listing ended in
/// (`Folder::listing_failure`).
pub fn folder_listing(folder: Box<dyn Folder>) -> FolderListing {
    FolderListing {
        folder: Some(folder),
        heard: Rc::new(Heard::default()),
        listener: None,
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
        }
        let pending = self.folder.as_ref().is_none_or(|f| f.is_load_pending());
        if pending {
            return Poll::Pending;
        }
        self.stop_listening();
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
