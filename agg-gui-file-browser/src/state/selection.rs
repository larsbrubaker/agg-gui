//! The browser's selection: the selected rows, the clicks that change it (a click and a right
//! press), and what showing the list again does with it (`redisplay`). The selection is the
//! selected rows' entries, compared by object, so a re-sort can never leave it on another item.
//! Showing the list anew empties it and puts the list back at the top (`list_resets`). Rows the
//! app wants selected once they are listed (the copy a "Make a copy" made, `select_when_listed`)
//! are carried by their identity key (`item_identity_key`), selected when the folder lists them
//! and scrolled into view (`take_reveal_selection`). Part of `state/mod.rs`, whose fields it
//! reads.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::Modifiers;
use agg_gui_files::{Item, MessageItem};

use super::browser_entry::item_identity_key;
use super::{BrowserEntry, BrowserState};

/// Whether `entries` is only a message the folder shows in the place of rows ("Loading...", why
/// it could not be listed): one `MessageItem` that is not a missing-file row. The message is the
/// folder's state, not something the user can select, open, rename or drag.
pub fn is_only_a_message(entries: &[BrowserEntry]) -> bool {
    let [BrowserEntry::Item(item)] = entries else {
        return false;
    };
    item.as_any()
        .and_then(|any| any.downcast_ref::<MessageItem>())
        .is_some_and(|message| !message.is_missing_file())
}

impl BrowserState {
    /// The list is being built from `entries()` as they are now: from here a row's index means a
    /// row of this listing (`listed_entry`). A folder whose listing is only a message
    /// ([`is_only_a_message`]) lists no rows, so Select All and the arrow keys find nothing.
    pub fn mark_listed(&mut self) {
        let entries = self.entries();
        let only_a_message = self.showing_search_results().is_none() && is_only_a_message(&entries);
        self.listed = Some(if only_a_message { Vec::new() } else { entries });
    }

    /// The row at `index` of the list the user sees: the listing the list was last built from
    /// (`mark_listed`), or `entries()` before the first build. A click that lands after the
    /// showing folder's content changed but before the list was rebuilt still acts on the row
    /// under the pointer, not on whatever now stands at its index.
    pub fn listed_entry(&self, index: usize) -> Option<BrowserEntry> {
        match &self.listed {
            Some(listed) => listed.get(index).cloned(),
            None => self.entries().get(index).cloned(),
        }
    }

    /// Whether `item` is a row the showing listing holds now (by `item_identity_key`): a row
    /// taken out of the listing is not, so a drag never carries a row the user can no longer see.
    pub fn lists_row(&self, item: &dyn Item) -> bool {
        let key = item_identity_key(item);
        self.entries()
            .iter()
            .any(|entry| entry.identity_key() == key)
    }

    /// Whether the listed row at `index` is selected.
    pub fn is_selected(&self, index: usize) -> bool {
        self.listed_entry(index)
            .is_some_and(|entry| self.selected.iter().any(|s| s.same_row(&entry)))
    }

    /// The selected rows' entries, in selection order (the tree's folder alone while the tree's
    /// menu holds a temporary selection).
    pub fn selected_entries(&self) -> Vec<BrowserEntry> {
        if let Some((link, _)) = &self.temporary_selection {
            return vec![BrowserEntry::Container(Rc::clone(link))];
        }
        self.selected.clone()
    }

    /// A click on row `index`: its press and its release without a drag (`state/gesture.rs`),
    /// a double click's activation running its hooks once `state`'s borrow has ended.
    pub fn click(state: &Rc<RefCell<Self>>, index: usize, modifiers: Modifiers) {
        {
            let mut s = state.borrow_mut();
            s.press_in(index, modifiers);
            s.release_press(false);
        }
        Self::run_hooks(state);
    }

    /// A right press on row `index`: a row not selected becomes the selection (joins it with
    /// Control or Command), a selected one leaves it as it is.
    pub fn ensure_selected(&mut self, index: usize, ctrl: bool) {
        let Some(entry) = self.listed_entry(index) else {
            return;
        };
        if self.selected.iter().any(|s| s.same_row(&entry)) {
            return;
        }
        if !ctrl {
            self.selected.clear();
        }
        self.selected.push(entry);
        self.changed();
    }

    /// The showing folder's rows are shown again (its content changed, it was sorted, the view
    /// changed). For the same search's results the list keeps its place and the selection is
    /// matched back onto the rows (`state/search.rs`); anything else is new content, so the
    /// selection is emptied and the list goes back to the top, rows waiting to be selected once
    /// listed being selected. The trace names the line that
    /// asked (`#[track_caller]`): the redisplay's trigger.
    #[track_caller]
    pub(super) fn redisplay(&mut self) {
        crate::trace(|| {
            format!(
                "browser redisplay from {} (selection {} cleared)",
                std::panic::Location::caller(),
                self.selected.len()
            )
        });
        if self.showing_search_results().is_some() {
            self.restore_search_selection();
        } else {
            self.selected.clear();
            self.parked_selection.clear();
            self.list_resets += 1;
            if self.select_pending_rows() {
                self.select_when_listed.clear();
            }
        }
        self.changed();
    }

    /// Selects the rows whose items carry the identity `keys` once the showing folder lists them,
    /// and scrolls the list to them: now, when they are listed already, and again when the folder
    /// next lists itself. Showing another folder drops the request.
    pub fn select_when_listed(&mut self, keys: Vec<String>) {
        self.select_when_listed = keys;
        if self.select_pending_rows() {
            self.changed();
        }
    }

    /// Whether the list should scroll the selection into view; answering clears it.
    pub fn take_reveal_selection(&mut self) -> bool {
        std::mem::take(&mut self.reveal_selection)
    }

    /// Selects the listed rows `select_when_listed` carries the keys of; whether any was listed.
    fn select_pending_rows(&mut self) -> bool {
        if self.select_when_listed.is_empty() {
            return false;
        }
        let found: Vec<BrowserEntry> = self
            .entries()
            .into_iter()
            .filter(|entry| self.select_when_listed.contains(&entry.identity_key()))
            .collect();
        if found.is_empty() {
            return false;
        }
        self.selected = found;
        self.reveal_selection = true;
        true
    }
}
