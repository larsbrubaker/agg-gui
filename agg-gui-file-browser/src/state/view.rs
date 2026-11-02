//! How the browser's list shows the showing folder: its [`ViewMode`] and picture size, restored
//! from the folder's remembered view whenever a folder shows and remembered when the user picks
//! one, through the config's `BrowserMemory`; the sort, the folder's `default_sort` whenever it
//! shows, then its remembered sort; and "Show Folders", on by default and not remembered, so a
//! folder never seems empty because its sub-folders were hidden last time. The memory's
//! `view`/`set_view` are handed the path's names alone, so they may run while the state is
//! borrowed. Part of `state/mod.rs`, whose fields it reads; navigation (`state/navigation.rs`)
//! restores the view each time a folder shows.

use agg_gui_files::{SortBehavior, SortKey};

use crate::memory::{ViewMode, ViewState};

use super::BrowserState;

impl BrowserState {
    /// The showing folder's `default_sort`, then the folder's remembered view and sort when it
    /// has one. The remembered view is compared with what this list shows, so a list opened on a
    /// size of its own keeps it unless the folder remembers a different mode.
    pub(super) fn restore_user_view(&mut self) {
        let default_sort = self.active_container().and_then(|c| c.default_sort());
        if let Some(sort) = default_sort {
            self.sort = sort;
        }
        if let Some(view) = self.config.memory.view(&self.path_names()) {
            if view.view_mode != self.view_mode {
                self.view_mode = view.view_mode;
                self.thumb_size = view.view_mode.thumb_size();
            }
            self.sort = view.sort;
        }
    }

    /// The picture side the list shows.
    pub fn thumb_size(&self) -> i32 {
        self.thumb_size
    }

    /// The key and direction the list is sorted on.
    pub fn sort(&self) -> SortBehavior {
        self.sort
    }

    /// Sorts on `key` and remembers it for the folder; the list is shown again only when the
    /// sort changed.
    pub fn set_user_sort_key(&mut self, key: SortKey) {
        let changed = self.sort.sort_key != key as i32;
        self.sort.sort_key = key as i32;
        self.persist_user_view();
        if changed {
            self.redisplay();
        }
    }

    /// Sorts in that direction and remembers it for the folder; the list is shown again only
    /// when the direction changed.
    pub fn set_user_sort_ascending(&mut self, ascending: bool) {
        let changed = self.sort.ascending != ascending;
        self.sort.ascending = ascending;
        self.persist_user_view();
        if changed {
            self.redisplay();
        }
    }

    /// Remembers the view and sort this list shows for the folder.
    fn persist_user_view(&self) {
        let view = ViewState {
            view_mode: self.view_mode,
            sort: self.sort,
        };
        self.config.memory.set_view(&self.path_names(), &view);
    }

    /// How the list shows its entries.
    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    /// The view menu's pick: shows the folder in `mode`, remembers it for the folder, then shows
    /// the list again.
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
        self.thumb_size = mode.thumb_size();
        self.persist_user_view();
        self.redisplay();
    }

    /// Whether the list shows folders ("Show Folders").
    pub fn show_containers(&self) -> bool {
        self.show_containers
    }

    /// The view menu's "Show Folders": hides or shows the folders, then lists again.
    pub fn set_show_containers(&mut self, show: bool) {
        if self.show_containers != show {
            self.show_containers = show;
            self.selected.clear();
            self.redisplay();
        }
    }
}
