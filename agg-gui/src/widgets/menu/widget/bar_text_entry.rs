//! The parts of [`MenuBar`] that serve a menu with a text entry (a search
//! field, [`MenuItem::takes_text`]): installing the row widgets a
//! [`super::TopMenu`] supplies when it opens, putting the highlight on the
//! field, and swapping in the rows its [`super::super::MenuItemsUpdate`]
//! asks for while the menu stays open. `bar.rs` calls these from its open,
//! event and paint paths.
//!
//! [`MenuItem::takes_text`]: crate::widgets::menu::MenuItem::takes_text

use super::MenuBar;

impl MenuBar {
    /// After menu `idx` opened: registers the widgets its rows host, drops
    /// rows asked for while it was closed, and puts the highlight on its
    /// text entry.
    pub(super) fn prepare_opened_menu(&mut self, idx: usize) {
        let menu = &mut self.menus[idx];
        for (id, widget) in menu.make_row_widgets() {
            self.popup.set_row_widget(id, widget);
        }
        if let Some(update) = &menu.update {
            update.take();
        }
        self.popup.focus_text_row();
    }

    /// Swaps rows the open menu's update asked for into the popup.
    pub(super) fn apply_items_update(&mut self) {
        let Some(idx) = self.open_index else {
            return;
        };
        if !self.popup.is_open() {
            return;
        }
        let Some(items) = self.menus[idx].update.as_ref().and_then(|u| u.take()) else {
            return;
        };
        self.menus[idx].items = items.clone();
        self.popup.replace_items(items);
        self.cache.invalidate();
    }
}
