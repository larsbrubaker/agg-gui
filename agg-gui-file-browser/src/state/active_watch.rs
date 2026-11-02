//! The browser's hold on the folder it shows. Only the showing folder is activated (a folder that
//! follows something while shown starts following it) and listened to for content changes; the
//! one the browser leaves is deactivated and its listener taken back off. The folders above it on
//! the path stay loaded for the breadcrumbs but are neither activated nor listened to, so a change
//! in a folder does not reload the one behind it. `BrowserState` (`state/mod.rs`) calls `leave`
//! before the showing folder changes and `listen` (before a new folder's load) or `enter` (for one
//! already loaded, an ancestor shown again) after.
//!
//! A folder that changed behind the browser's back without saying so (a file another program
//! wrote, in a folder no directory monitor follows) is listed again on the user's ask
//! (`BrowserState::relist`: F5 in a dialog): it loads again and the list shows it anew; a folder
//! that loads asynchronously raises its content change once the listing arrives, which shows it
//! again then.
//!
//! A dialog hands the showing folder over to the work it closes on (`take_active_container`);
//! it asks first whether it can (`can_hand_over_active_container`: below the root, and nothing
//! else holding the folder), so a folder still held elsewhere stays showing and the user can be
//! told to try again.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui_files::{ContentChangedListener, Folder};

use super::{BrowserState, OpenFolder};

impl BrowserState {
    /// Loads the showing folder again and shows the list anew (see the module header).
    pub fn relist(&mut self) {
        if let Some(showing) = self.path.last() {
            showing.borrow_mut().load();
        }
        self.redisplay();
    }

    /// Whether `take_active_container` would hand over the showing folder alone: it is below the
    /// root and nothing else (a hook given the path) still holds it.
    pub fn can_hand_over_active_container(&self) -> bool {
        self.path.len() >= 2 && self.path.last().is_some_and(|f| Rc::strong_count(f) == 1)
    }
}

/// The counters the content-changed listener shares with the browser.
#[derive(Default)]
struct WatchCells {
    /// Bumped by the showing folder's content changes.
    ticks: Cell<u64>,
    /// Which listener is the live one: each `listen` takes the next number, and `leave` moves past
    /// it. `leave` takes the listener off its folder, but a folder that keeps its listeners without
    /// overriding `unsubscribe_content_changed` still holds it: it keeps its number and does
    /// nothing once it is not the live one.
    live: Cell<u64>,
}

/// The browser's subscription to its showing folder (see the module header).
#[derive(Default)]
pub(super) struct ActiveWatch {
    cells: Rc<WatchCells>,
    /// The listener on the showing folder, which `leave` takes back off.
    listener: RefCell<Option<ContentChangedListener>>,
}

impl ActiveWatch {
    /// How many content changes the showing folders have raised.
    pub(super) fn ticks(&self) -> u64 {
        self.cells.ticks.get()
    }

    /// Listens for content changes in `folder`, the one about to show. Called before its load
    /// starts, so an answer that arrives during `load` is not missed.
    pub(super) fn listen(&self, folder: &dyn Folder) {
        let id = self.cells.live.get() + 1;
        self.cells.live.set(id);
        let cells = Rc::downgrade(&self.cells);
        let listener: ContentChangedListener = Rc::new(move || {
            if let Some(cells) = cells.upgrade().filter(|c| c.live.get() == id) {
                cells.ticks.set(cells.ticks.get() + 1);
                agg_gui::animation::request_draw();
            }
        });
        folder.subscribe_content_changed(Rc::clone(&listener));
        *self.listener.borrow_mut() = Some(listener);
    }

    /// Deactivates the showing folder (the last on `path`) and takes the listener off it, before
    /// the browser moves off it.
    pub(super) fn leave(&self, path: &[OpenFolder]) {
        let listener = self.listener.borrow_mut().take();
        if let Some(showing) = path.last() {
            let mut showing = showing.borrow_mut();
            showing.deactivate();
            if let Some(listener) = &listener {
                showing.unsubscribe_content_changed(listener);
            }
        }
        self.cells.live.set(self.cells.live.get() + 1);
    }

    /// Activates and listens to the last folder on `path`, one already loaded (an ancestor shown
    /// again).
    pub(super) fn enter(&self, path: &[OpenFolder]) {
        if let Some(showing) = path.last() {
            let mut showing = showing.borrow_mut();
            showing.activate();
            self.listen(showing.as_ref());
        }
    }
}
