//! A remembered location being found again, shown while it is ([`BrowserState::restoring_trail`]):
//! a folder that lists slowly (a remote folder waiting on the sign-in or the server) would
//! otherwise leave the root's rows on screen, to be swapped away a moment later under a user who
//! had started to find their own way. When the app's memory names the remembered folders
//! (`BrowserMemory::last_location_trail`), the browser shows their breadcrumbs at once and a
//! "Loading..." line in place of the list (the showing folder's entries are only the loading
//! message, so nothing can be selected or opened under it).
//!
//! A crumb clicked meanwhile is where the user is going ([`BrowserState::go_to_crumb`]): the
//! root ends the restore there and then, a folder crumb shortens it to that folder (once found,
//! only the folders down to it open). Back ([`BrowserState::go_back`]) shortens it by one folder,
//! ending it at the root. Any navigation of the browser's own (a folder opened from the tree, a
//! search) ends it: the user is showing a place of their own, which the loading line must not
//! cover and the late answer must not replace. A location that can't be reached ends it at the
//! root without a word. [`BrowserState::retry_restore`] runs the last restore again from the
//! root (an app calls it once the user signs in). Part of `state/mod.rs`, whose fields it reads;
//! `state/navigation.rs` starts it (`restore_location`).
//!
//! When a restore's search answers, the listeners added with
//! [`BrowserState::add_restore_finished`] hear how it went ([`RestoreOutcome`]): the folders
//! opened (all of them, or down to the crumb the user clicked meanwhile; only the last is read,
//! the ones above it when the user goes back up to them), the location could not be reached, the user ended the restore (went elsewhere
//! meanwhile), or a newer restore began. They are told outside any borrow of the
//! state, so they may read it. A key the memory remembers nothing under starts no restore and
//! tells nothing.

use std::rc::Rc;

use agg_gui_files::MessageItem;

use super::{BrowserEntry, BrowserState};

/// How a restore's answer went (see the module header).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// The remembered folders opened, all of them.
    Landed,
    /// The remembered folders opened down to the folder crumb the user clicked meanwhile: a
    /// place the user chose.
    LandedShortened,
    /// The memory could not reach the location: the browser stays at its root.
    Failed,
    /// The answer came after the user ended the restore (went elsewhere meanwhile); it opened
    /// nothing.
    Abandoned,
    /// The answer came after a newer restore began (a retry); it opened nothing, and the newer
    /// one's answer is told in its turn.
    Superseded,
}

/// Hears each restore's outcome.
pub type RestoreFinishedListener = Rc<dyn Fn(RestoreOutcome)>;

/// A restore still finding its folder.
#[derive(Clone, Debug)]
pub(super) struct Pending {
    /// The breadcrumbs shown meanwhile, the root first; the root alone (or nothing) when the
    /// memory names none, and then the root's rows show meanwhile.
    trail: Vec<String>,
    /// How many folders below the root open once found: all of them, until the user clicks a
    /// folder crumb on the way.
    keep: Option<usize>,
    /// Which restore this is, so a late answer to an earlier one ends nothing.
    serial: u64,
}

impl Pending {
    /// Whether the trail shows in place of the root.
    fn shows(&self) -> bool {
        self.trail.len() > 1
    }
}

impl BrowserState {
    /// The breadcrumbs of the remembered location being found, the root first, while they show
    /// in place of the root (see the module header).
    pub fn restoring_trail(&self) -> Option<&[String]> {
        self.shown_restore().map(|p| p.trail.as_slice())
    }

    /// The navigation count the last restore's folders opened at: where a `Landed` outcome put
    /// the browser, however far the user has moved since (its listeners may be told a turn
    /// later). `None` until a restore lands.
    pub fn restore_landed_at(&self) -> Option<u64> {
        self.restore_landed_at
    }

    /// Whether a remembered location's trail shows over a loading line now.
    pub fn is_restoring(&self) -> bool {
        self.shown_restore().is_some()
    }

    fn shown_restore(&self) -> Option<&Pending> {
        self.restore.as_ref().filter(|p| p.shows())
    }

    /// The breadcrumbs the browser shows: the location being found while restoring, else the
    /// open path's names.
    pub fn shown_trail(&self) -> Vec<String> {
        match self.restoring_trail() {
            Some(trail) => trail.to_vec(),
            None => self.path_names(),
        }
    }

    /// The crumb at `depth` (0 = the root) clicked: while restoring the restore is shortened to
    /// it (or ended, at the root); otherwise the browser goes there (`go_to`).
    pub fn go_to_crumb(&mut self, depth: usize) {
        let Some(pending) = self.restore.as_mut().filter(|p| p.shows()) else {
            self.go_to(depth);
            return;
        };
        if depth == 0 {
            self.end_restore();
            return;
        }
        if depth + 1 < pending.trail.len() {
            pending.trail.truncate(depth + 1);
            pending.keep = Some(depth);
            self.changed();
        }
    }

    /// Up one folder, as the up and the mouse's back buttons go: along the trail being found
    /// while restoring, else along the open path. Nothing at the root.
    pub fn go_back(&mut self) {
        if let Some(pending) = self.shown_restore() {
            let depth = pending.trail.len().saturating_sub(1);
            self.go_to_crumb(depth.saturating_sub(1));
            return;
        }
        let depth = self.depth();
        if depth > 1 {
            self.go_to(depth - 2);
        }
    }

    /// Runs the last restore again (an app's call once the user signs in, so a remote folder
    /// that could not be listed before is found now). Nothing when no restore was asked for, or
    /// the browser has left its root.
    pub fn retry_restore(state: &Rc<std::cell::RefCell<Self>>) {
        let key = {
            let s = state.borrow();
            s.restore_key.clone().filter(|_| s.depth() == 1)
        };
        if let Some(key) = key {
            Self::restore_location(state, &key);
        }
    }

    /// `listener` hears how each later restore's answer went (see the module header).
    pub fn add_restore_finished(&mut self, listener: RestoreFinishedListener) {
        self.restore_finished.push(listener);
    }

    /// Tells the restore-finished listeners `outcome`; `state` is not borrowed while they run.
    pub(super) fn tell_restore_finished(
        state: &Rc<std::cell::RefCell<Self>>,
        outcome: RestoreOutcome,
    ) {
        let listeners = state.borrow().restore_finished.clone();
        for listener in listeners {
            listener(outcome);
        }
    }

    /// Starts a restore showing `trail` (when it names more than the root) while it finds its
    /// folder; answers its number, for `finish_restore`.
    pub(super) fn begin_restore(&mut self, trail: Option<Vec<String>>) -> u64 {
        self.end_restore();
        self.restore_serial += 1;
        let pending = Pending {
            trail: trail.unwrap_or_default(),
            keep: None,
            serial: self.restore_serial,
        };
        if pending.shows() {
            self.list_resets += 1;
            self.changed();
        }
        self.restore = Some(pending);
        self.restore_serial
    }

    /// The restore numbered `serial` answered: `None` when it no longer stands (ended by the
    /// user, or a newer one began), else how many folders below the root it opens (`None`: all).
    /// Ends it.
    pub(super) fn finish_restore(&mut self, serial: u64) -> Option<Option<usize>> {
        let keep = self.restore.as_ref().filter(|p| p.serial == serial)?.keep;
        self.end_restore();
        Some(keep)
    }

    /// Whether a restore newer than the one numbered `serial` has begun.
    pub(super) fn superseded(&self, serial: u64) -> bool {
        self.restore_serial != serial
    }

    /// The restore is over (found, ended by the user, or replaced): the open path shows again.
    pub(super) fn end_restore(&mut self) {
        if let Some(pending) = self.restore.take() {
            if pending.shows() {
                self.list_resets += 1;
                self.changed();
            }
        }
    }

    /// While restoring, the showing folder's entries are the loading message alone.
    pub(super) fn restoring_entries(&self) -> Option<Vec<BrowserEntry>> {
        self.shown_restore()
            .map(|_| vec![BrowserEntry::Item(Rc::new(MessageItem::loading()))])
    }
}
