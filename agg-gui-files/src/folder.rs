//! A [`Folder`]: what a browser shows, listing links to other folders and items (`item`).
//!
//! A folder whose listing arrives after `load` returns raises its content-changed callbacks,
//! which the browser subscribes and takes back off ([`ContentChangedEvent`] is the invocation
//! list), and may carry header markdown; it answers `is_load_pending` while that listing is out,
//! which a search waits on. A folder that searches in sections names them; one that searches
//! itself answers `search_provider`, and one that reads its own items' pictures answers
//! `get_thumbnail`. A folder the user can change answers `as_writable` (`writable`). A folder
//! whose last listing failed says what kind of failure it was ([`ListingFailure`]): the server
//! refusing the sign-in the request carried, which signing in again recovers, or anything else.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use crate::item::{FolderLink, Item, PictureFuture};
use crate::place::PlaceId;
use crate::search::SearchProvider;
use crate::sort::SortBehavior;
use crate::writable::WritableFolder;

/// What kind of failure a folder's last listing ended in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListingFailure {
    /// The server refused the sign-in the request carried (an expired or revoked session):
    /// signing in again, or fresh tokens, list the folder.
    SignIn,
    /// Any other failure: the folder could not be listed (no answer, a refusal of the folder
    /// itself, an answer that could not be read).
    Other,
}

/// A folder, listing folders and items.
pub trait Folder {
    /// Stable across sessions.
    fn id(&self) -> &str;
    /// The breadcrumb's text.
    fn name(&self) -> &str;
    /// Cannot be renamed or removed.
    fn is_protected(&self) -> bool;
    /// Whether this folder is the user's Trash (see [`FolderLink::is_trash`]); false by default,
    /// for every folder that is not.
    fn is_trash(&self) -> bool {
        false
    }
    /// The links to child folders, in the order the folder lists them: a snapshot, so a folder
    /// that fills itself in later can be read while it does.
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>>;
    /// The items, in the order the folder lists them (a snapshot, as `child_containers`).
    fn items(&self) -> Vec<Rc<dyn Item>>;
    /// Fills `child_containers` and `items`. A folder that loads asynchronously only starts the
    /// load here and shows a "Loading..." placeholder; it raises its content-changed callbacks
    /// when the listing arrives.
    fn load(&mut self);
    /// Markdown the list view shows above the rows (a repository's `index.md`, or why a folder
    /// could not be read). Empty for none.
    fn header_markdown(&self) -> String {
        String::new()
    }
    /// The order the list switches to whenever this folder shows (before a sort the user
    /// remembered for it). `None` keeps the list's current order.
    fn default_sort(&self) -> Option<SortBehavior> {
        None
    }
    /// `listener` runs each time the folder's rows or header change after `load` returned.
    /// Folders whose content is fixed once loaded never raise it, which is what the default
    /// (keeping no listener) means.
    fn subscribe_content_changed(&self, listener: ContentChangedListener) {
        let _ = listener;
    }
    /// Takes `listener` (the same `Rc` given to `subscribe_content_changed`, compared by
    /// identity) back off. A folder that keeps listeners overrides this; one that keeps none has
    /// nothing to take off, which is what the default means.
    fn unsubscribe_content_changed(&self, listener: &ContentChangedListener) {
        let _ = listener;
    }
    /// The browser starts showing this folder (after `load`). Most folders do nothing; one
    /// that follows a source while shown (a Home following its favorites) starts here.
    fn activate(&mut self) {}
    /// The browser stops showing it.
    fn deactivate(&mut self) {}
    /// The browser's per-frame turn for its showing folder: timed work such as a folder's
    /// coalesced live refresh. The model has no UI timer, so the browser's frame drives it.
    /// Answers whether more is pending, so the browser keeps waking. Most folders have nothing
    /// to do.
    fn pump_live_refresh(&mut self) -> bool {
        false
    }
    /// The children a search covers, one result section each, for a folder that presents itself
    /// in sections (Home); `None` for one searched as a single section, walked through its own
    /// child folders.
    fn search_sections(&self) -> Option<Vec<Rc<dyn FolderLink>>> {
        None
    }
    /// Whether the listing the last `load` started is still on its way, so a reader straight
    /// after `load` would see only the "Loading..." placeholder. A search waits (bounded) while
    /// this holds, woken by the folder's content-changed: a folder that answers true raises it
    /// when the listing (or its failure) lands. Folders that list within `load` never are.
    fn is_load_pending(&self) -> bool {
        false
    }
    /// Whether this folder (made, not loaded) lists any visible child folders, found the
    /// cheapest correct way it knows; `None` when that is not known yet. The default loads it
    /// and looks, unknown while that load is still on its way. A folder whose full listing
    /// costs much more than finding one sub-folder (a folder on disk reading every file)
    /// answers more cheaply. Asked only through a link that opted in to answering
    /// ([`FolderLink::has_child_containers`]), for folders that list in memory or from local
    /// disk.
    fn has_child_containers(&mut self) -> Option<bool> {
        self.load();
        if self.is_load_pending() {
            return None;
        }
        Some(self.child_containers().iter().any(|link| link.is_visible()))
    }
    /// Whether the last listing ended without an answer (offline, refused, an error page), so
    /// its empty rows say nothing about what the folder holds. Folders that list within `load`
    /// never fail that way, which is what the default means.
    fn last_load_failed(&self) -> bool {
        false
    }
    /// What kind of failure the last listing ended in, `None` while it did not fail. The
    /// default knows no more than that it failed (`last_load_failed`).
    fn listing_failure(&self) -> Option<ListingFailure> {
        self.last_load_failed().then_some(ListingFailure::Other)
    }
    /// A folder that searches itself (a folder on disk walks its directories); `None` for one a
    /// search walks item by item.
    fn search_provider(&self) -> Option<&dyn SearchProvider> {
        None
    }
    /// The concrete folder, for code that acts on one folder type; `None` by default.
    fn as_any(&self) -> Option<&dyn Any> {
        None
    }
    /// The folder's add, remove and rename, for one the user can change; `None` (the default)
    /// for a read-only one.
    fn as_writable(&self) -> Option<&dyn WritableFolder> {
        None
    }
    /// `as_writable`, to change the folder.
    fn as_writable_mut(&mut self) -> Option<&mut dyn WritableFolder> {
        None
    }
    /// This folder's own reading of `item`'s picture, the first thing a picture loader asks the
    /// folder it is showing; `None` (the default) when the folder has nothing to add and the
    /// generic steps decide.
    fn get_thumbnail(&self, item: &Rc<dyn Item>, width: i32, height: i32) -> Option<PictureFuture> {
        let _ = (item, width, height);
        None
    }
    /// The storage this folder lives in ([`PlaceId`]), as [`FolderLink::place`] says for its
    /// link; `None` (the default) for one in no place items move or copy between.
    fn place(&self) -> Option<PlaceId> {
        None
    }
    /// Where this folder is, as [`FolderLink::location_key`] says for its link; `None` (the
    /// default) when it has no such identity.
    fn location_key(&self) -> Option<String> {
        None
    }
}

/// A content-changed callback.
pub type ContentChangedListener = Rc<dyn Fn()>;

/// The content-changed callbacks subscribed to a folder, for folders that raise it. Listeners
/// are compared by identity (`Rc::ptr_eq`), so the `Rc` subscribed is the one to take back off.
#[derive(Clone, Default)]
pub struct ContentChangedEvent {
    listeners: Rc<RefCell<Vec<ContentChangedListener>>>,
}

impl ContentChangedEvent {
    /// Adds `listener`.
    pub fn subscribe(&self, listener: ContentChangedListener) {
        self.listeners.borrow_mut().push(listener);
    }

    /// Removes the last subscription of `listener` (compared by identity), so a listener
    /// subscribed twice needs two removals; nothing when it is not subscribed.
    pub fn unsubscribe(&self, listener: &ContentChangedListener) {
        let mut listeners = self.listeners.borrow_mut();
        if let Some(at) = listeners.iter().rposition(|l| Rc::ptr_eq(l, listener)) {
            listeners.remove(at);
        }
    }

    /// Runs every listener, in subscription order. The list is copied first, so a listener may
    /// subscribe another without a borrow conflict.
    pub fn raise(&self) {
        let listeners = self.listeners.borrow().clone();
        for listener in listeners {
            listener();
        }
    }
}
