//! Menu rows as named automation targets: [`PopupMenu::named_row_targets`]
//! reports each row on show of an open popup (every open level) whose name
//! matches, so a host widget can hand them out through
//! [`Widget::find_named_targets`](crate::Widget::find_named_targets) and GUI
//! automation can click a row by name.
//!
//! A row's name is agg-sharp's: the item's own automation name when it has one
//! (`MenuItemModel.AutomationName`, set as the row's `Name` by
//! `MenuModelPopupBuilder`), else `PopupMenu.CreateMenuItem` and
//! `CreateSubMenu`'s `"{Text} Menu Item"` (`Gui/Menu/PopupMenu.cs`).
//! Separators and widget rows (whose hosted widget is the clickable thing)
//! are not reported. A scrolled panel's rows are cut to the panel, and rows
//! scrolled wholly out of it are left out.
//!
//! [`MenuBar`](super::MenuBar) reports its open menu's rows this way; a widget
//! that hosts its own [`PopupMenu`] (`popup_local.rs`) does the same from its
//! `find_named_targets`.

use crate::geometry::{Rect, Size};
use crate::NamedTarget;

use super::super::geometry::item_at_path;
use super::super::model::MenuItem;
use super::PopupMenu;

/// The automation name of a menu row: its
/// [`automation_name`](MenuItem::automation_name) when set (C#
/// `MenuItemModel.AutomationName`), else C#'s `"{Text} Menu Item"`.
pub fn menu_row_name(item: &MenuItem) -> String {
    match &item.automation_name {
        Some(name) => name.clone(),
        None => format!("{} Menu Item", item.label),
    }
}

impl PopupMenu {
    /// Push a [`NamedTarget`] for every row on show named `name` onto
    /// `out`, in the host widget's local space (the popup's space less its
    /// [`root_origin`](Self::root_origin)). `viewport` is the one the popup
    /// is laid out against (`current_viewport()` for an overlay popup). The
    /// target carries the row's enabled flag and its item path
    /// (`Vec<usize>`) as its object. Nothing while the popup is closed.
    pub fn named_row_targets(&self, name: &str, viewport: Size, out: &mut Vec<NamedTarget>) {
        let origin = self.root_origin();
        for layout in self.state.layouts(&self.items, viewport) {
            for row in &layout.rows {
                let Some(index) = row.item_index else {
                    continue;
                };
                let mut path = layout.path_prefix.clone();
                path.push(index);
                let Some(item) = item_at_path(&self.items, &path) else {
                    continue;
                };
                if item.widget_row.is_some() || menu_row_name(item) != name {
                    continue;
                }
                let bottom = row.rect.y.max(layout.rect.y);
                let top = (row.rect.y + row.rect.height).min(layout.rect.y + layout.rect.height);
                if top <= bottom {
                    continue;
                }
                let rect = Rect::new(
                    row.rect.x - origin.x,
                    bottom - origin.y,
                    row.rect.width,
                    top - bottom,
                );
                out.push(
                    NamedTarget::new(rect)
                        .with_enabled(item.enabled)
                        .with_object(std::rc::Rc::new(path)),
                );
            }
        }
    }
}
