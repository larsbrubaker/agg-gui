//! Spring-loaded folders: a drag held still over a folder for [`SPRING_DELAY`] opens it, so the
//! drag can reach a folder that wasn't open. Over a tree row with an expand arrow the row opens in
//! place (the tree asks [`spring_tree`] as it paints) and stays open after the drop; over a folder
//! row or tile in the list the folder opens in the list, as a double click would (the list asks
//! [`spring_list`] as it paints), and the drag carries on with the same items from the same source
//! folder. In a folder the list opened that way, a drop on the list's empty space lands in that
//! folder (`DropManager::dropped_into_showing`).
//!
//! The wait runs on agg-gui's UI clock (`agg_gui::clock`), which headless tests advance, measured
//! from the instant the drag reached the folder (never as time since the clock's epoch, which a
//! virtual clock set before it would hold at zero), and each wait asks for a draw when it is due
//! (`request_draw_after`): a pointer held perfectly still sends no events, so the paint that opens
//! the folder has to be asked for.
//!
//! A spring open is never the user's own open: a folder with nothing to give that acts instead (a
//! cloud folder's sign-in while signed out) stays shut and starts nothing. When the wait is over,
//! the folder now at the place it was waited over must still be that folder.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use agg_gui_files::FolderLink;
use web_time::Instant;

use super::{DropKey, DropTargetKind};
use crate::state::BrowserState;
use crate::tree::TreeModel;

/// How long a drag has to stay over a folder before it opens.
pub const SPRING_DELAY: Duration = Duration::from_millis(800);

/// The folder a drag is waiting over and the folders it opened.
#[derive(Default)]
pub struct SpringOpen {
    /// The folder under the drag (its target key and folder ID) and when the drag reached it.
    waiting: Option<(DropKey, String, Instant)>,
    /// The folder the last wait opened, which doesn't open again until the drag leaves it.
    opened_last: Option<(DropKey, String)>,
    /// The folder the list opened for this drag, with its depth in the browser's path.
    showing: Option<(usize, Rc<dyn FolderLink>)>,
}

impl SpringOpen {
    /// The drag is over `folder` (its target key and link) now, or over no folder. A new folder
    /// starts the wait and asks for the draw that ends it.
    pub fn hover(&mut self, folder: Option<(DropKey, &dyn FolderLink)>) {
        let Some((key, link)) = folder else {
            self.waiting = None;
            self.opened_last = None;
            return;
        };
        let same = |k: &DropKey, id: &str| *k == key && id == link.id();
        if self.opened_last.as_ref().is_some_and(|(k, id)| same(k, id)) {
            return;
        }
        self.opened_last = None;
        if self.waiting.as_ref().is_some_and(|(k, id, _)| same(k, id)) {
            return;
        }
        self.waiting = Some((key, link.id().to_string(), agg_gui::clock::now()));
        agg_gui::animation::request_draw_after(SPRING_DELAY);
    }

    /// The folder of `kind` whose wait is over (its key and folder ID), taken; while one is still
    /// waiting, the draw that ends its wait is asked for again.
    fn take_due(&mut self, kind: DropTargetKind) -> Option<(DropKey, String)> {
        let (key, id, since) = self.waiting.as_ref()?;
        if key.kind != kind {
            return None;
        }
        let waited = agg_gui::clock::since(*since);
        if waited < SPRING_DELAY {
            agg_gui::animation::request_draw_after(SPRING_DELAY - waited);
            return None;
        }
        let (key, id) = (*key, id.clone());
        self.waiting = None;
        self.opened_last = Some((key, id.clone()));
        Some((key, id))
    }

    /// The folder the list opened for this drag, while it is the one showing at path depth
    /// `showing`.
    pub fn showing_folder(&self, showing: usize) -> Option<Rc<dyn FolderLink>> {
        self.showing
            .as_ref()
            .filter(|(depth, _)| *depth == showing)
            .map(|(_, link)| Rc::clone(link))
    }

    /// Forgets the waits and the folders opened, as a drag ends.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// The list's paint: a folder in the list the drag has waited over opens there, when it is still
/// the folder at that place and it has a folder to give.
pub fn spring_list(state: &Rc<RefCell<BrowserState>>) {
    let mut s = state.borrow_mut();
    if !s.drop.is_dragging() {
        return;
    }
    let Some((key, id)) = s.drop.spring.take_due(DropTargetKind::Content) else {
        return;
    };
    let Some(link) = s.drop.target_link(key).filter(|link| link.id() == id) else {
        return;
    };
    // Not `BrowserState::open`, whose failure path acts for a link with nothing to give.
    let Ok(folder) = link.get_container() else {
        return;
    };
    crate::trace(|| format!("spring opens {} in the list", link.name()));
    let before = s.depth();
    s.push_container(folder);
    if s.depth() > before {
        let showing = s.depth() - 1;
        s.drop.spring.showing = Some((showing, link));
    }
    agg_gui::animation::request_layout();
}

/// The tree's paint: a closed row with an expand arrow the drag has waited over opens, when it is
/// still the folder at that row.
pub fn spring_tree(state: &Rc<RefCell<BrowserState>>, model: &Rc<RefCell<TreeModel>>) {
    let due = {
        let mut s = state.borrow_mut();
        if !s.drop.is_dragging() {
            return;
        }
        s.drop.spring.take_due(DropTargetKind::Tree)
    };
    let Some((key, id)) = due else {
        return;
    };
    let mut model = model.borrow_mut();
    let closed = model.rows().get(key.index).is_some_and(|row| {
        row.has_arrow()
            && !row.is_expanded()
            && row.path().last().is_some_and(|link| link.id() == id)
    });
    if closed {
        crate::trace(|| format!("spring opens tree row {}", key.index));
        // Not as the user's own open (`by_user`), which would act for a folder with nothing to
        // give.
        model.toggle(key.index, false);
        agg_gui::animation::request_layout();
    }
}
