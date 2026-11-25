//! The parts of [`MenuBar`] that serve a menu with a text entry (a search
//! field, [`MenuItem::takes_text`]): installing the row widgets a
//! [`super::TopMenu`] supplies when it opens, putting the highlight on the
//! field, and swapping in the rows its [`super::super::MenuItemsUpdate`]
//! asks for while the menu stays open, and passing the row widgets' draw
//! needs on (a field's caret blink), as they live outside the widget tree.
//! `bar.rs` calls these from its open, event, paint and draw-need paths.
//!
//! [`MenuItem::takes_text`]: crate::widgets::menu::MenuItem::takes_text

use super::MenuBar;
use crate::widget::Widget;

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

    /// The bar's draw need: its children's, or a widget in the open menu's
    /// rows (a search field's caret blink).
    pub(super) fn subtree_needs_draw(&self) -> bool {
        self.is_visible()
            && (self.children().iter().any(|c| c.needs_draw())
                || self.popup.row_widgets_need_draw())
    }

    /// The earliest next draw the bar's children or its open menu's row
    /// widgets ask for.
    pub(super) fn subtree_draw_deadline(&self) -> Option<web_time::Instant> {
        if !self.is_visible() {
            return None;
        }
        self.children()
            .iter()
            .filter_map(|c| c.next_draw_deadline())
            .chain(self.popup.row_widgets_draw_deadline())
            .min()
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
