//! `MenuItemsUpdate` — new rows for a menu that is open, handed in from
//! outside it.
//!
//! A row widget inside an open menu (the search field of a
//! [`MenuItem::takes_text`] row) can't reach the [`super::MenuBar`] that owns
//! the menu, so it puts the rows it wants shown in a shared
//! `MenuItemsUpdate` ([`Self::set`]). The bar, given the same handle with
//! [`super::TopMenu::with_items_update`], swaps them into the open popup at
//! its next event or paint ([`super::PopupMenu::replace_items`]): the menu
//! stays open and its row widgets, the field among them, stay as they are.
//! Rows set while the menu is closed are dropped when it next opens, which
//! builds its rows afresh.
//!
//! [`MenuItem::takes_text`]: crate::widgets::menu::MenuItem::takes_text

use std::cell::RefCell;
use std::rc::Rc;

use super::super::model::MenuEntry;

/// The rows an open menu shows next, shared between the bar and whatever
/// sets them. Clones share one slot.
#[derive(Clone, Default)]
pub struct MenuItemsUpdate {
    pending: Rc<RefCell<Option<Vec<MenuEntry>>>>,
}

impl MenuItemsUpdate {
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks for `items` to replace the open menu's rows, and for a frame to
    /// show them. A later call before the swap replaces this one.
    pub fn set(&self, items: Vec<MenuEntry>) {
        *self.pending.borrow_mut() = Some(items);
        crate::animation::request_draw();
    }

    /// The rows asked for since the last take, if any.
    pub fn take(&self) -> Option<Vec<MenuEntry>> {
        self.pending.borrow_mut().take()
    }
}
