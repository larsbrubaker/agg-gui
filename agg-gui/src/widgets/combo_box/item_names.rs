//! Per-row names for [`super::ComboBox`], so GUI automation can click a row
//! of the open list by name — the agg-gui counterpart of the `MenuItem`s an
//! agg-sharp `DropDownList` holds (`Gui/Menu/DropDownList.cs`), which are
//! named `"{text} Menu Item"` by `AddItem` and renamed by callers
//! (`menuItem1.Name = "item1"` in `MenuTests.OpenAndCloseMenus`).
//!
//! The rows are drawn in the popup overlay pass, not widgets, so the combo
//! reports them through [`crate::Widget::find_named_targets`]: while the
//! list is open, each row in view whose name matches is a
//! [`crate::NamedTarget`] at the row's rectangle, carrying the row's
//! enabled state ([`ComboBox::item_enabled`], `item_enabled.rs`) and its
//! index as a `usize` object.
//!
//! Split out of `combo_box.rs` to keep that file under the 800-line limit.

use super::*;
use crate::NamedTarget;

impl ComboBox {
    /// Builder form of [`Self::set_item_name`].
    pub fn with_item_name(mut self, index: usize, name: impl Into<String>) -> Self {
        self.set_item_name(index, name);
        self
    }

    /// Name row `index` for automation (C# `menuItem.Name = …`). An index
    /// past the last row is remembered, so it applies if the row is added
    /// later.
    pub fn set_item_name(&mut self, index: usize, name: impl Into<String>) {
        if self.item_names.len() <= index {
            self.item_names.resize(index + 1, None);
        }
        self.item_names[index] = Some(name.into());
    }

    /// Row `index`'s automation name: the one set with
    /// [`Self::set_item_name`], else C#'s default `"{text} Menu Item"`;
    /// `None` past the last row.
    pub fn item_name(&self, index: usize) -> Option<String> {
        let text = self.options.get(index)?;
        Some(match self.item_names.get(index).cloned().flatten() {
            Some(name) => name,
            None => format!("{text} Menu Item"),
        })
    }

    /// Push a [`NamedTarget`] for every row in view of the open list named
    /// `name`; nothing while the list is closed.
    pub(super) fn named_row_targets(&self, name: &str, out: &mut Vec<NamedTarget>) {
        if !self.open {
            return;
        }
        let end = (self.scroll_offset + self.rows_in_view()).min(self.options.len());
        for index in self.scroll_offset..end {
            if self.item_name(index).as_deref() == Some(name) {
                out.push(
                    NamedTarget::new(self.item_rect(index))
                        .with_enabled(self.item_enabled(index))
                        .with_object(Rc::new(index)),
                );
            }
        }
    }
}
