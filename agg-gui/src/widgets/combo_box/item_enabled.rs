//! Per-row enabled state for [`super::ComboBox`] — the agg-gui counterpart of
//! agg-sharp's `MenuItem.Enabled = false` on an item a `DropDownList` holds
//! (`Tests/Agg.Tests/Agg Automation Tests/MenuTests.cs`, item3).
//!
//! A disabled row:
//! - is painted with its text at the disabled label alpha
//!   ([`super::ComboBoxDisabledStyle::label_alpha`], 50 by default, the alpha
//!   agg-sharp's `TextWidget` draws a disabled label with) and is never
//!   highlighted;
//! - takes no hover: the mouse over it highlights nothing;
//! - ignores a press: the press is consumed, the list stays open and the
//!   selection does not change, as a click on a disabled C# `MenuItem` (which
//!   receives no mouse events) leaves the `DropDownList` open;
//! - is skipped by Up / Down and by type-ahead, and Enter never picks it.
//!
//! C# bug fixed here (pinned by `rust_only_keys_never_pick_a_disabled_row` in
//! `tests/widgets/combo_item_enabled.rs`): C#'s `DropDownContainer.KeyDown`
//! moves `highLightedIndex` onto a disabled item and Enter then selects it,
//! so the keyboard picks what the mouse cannot.
//!
//! Split out of `combo_box.rs` to keep that file under the 800-line limit;
//! `press.rs`, `keyboard.rs` and `popup_paint.rs` consult [`ComboBox::item_enabled`].

use super::*;

impl ComboBox {
    /// Builder form of [`Self::set_item_enabled`].
    pub fn with_item_enabled(mut self, index: usize, enabled: bool) -> Self {
        self.set_item_enabled(index, enabled);
        self
    }

    /// Enable or disable row `index` (rows start enabled).  See the module
    /// docs for what a disabled row does.  An index past the last row is
    /// remembered, so it applies if the row is added later.
    pub fn set_item_enabled(&mut self, index: usize, enabled: bool) {
        if self.disabled_items.len() <= index {
            if enabled {
                return;
            }
            self.disabled_items.resize(index + 1, false);
        }
        self.disabled_items[index] = !enabled;
        if !enabled && self.hovered_item == Some(index) {
            self.hovered_item = None;
        }
        crate::animation::request_draw();
    }

    /// Whether row `index` can be picked.
    pub fn item_enabled(&self, index: usize) -> bool {
        !self.disabled_items.get(index).copied().unwrap_or(false)
    }

    /// The nearest enabled row after (`forward`) or before `from`, if any.
    pub(super) fn next_enabled_item(&self, from: usize, forward: bool) -> Option<usize> {
        if forward {
            (from + 1..self.options.len()).find(|&i| self.item_enabled(i))
        } else {
            (0..from.min(self.options.len()))
                .rev()
                .find(|&i| self.item_enabled(i))
        }
    }
}

/// Whether the popup request's row `index` is disabled.
pub(super) fn request_row_disabled(disabled_items: &[bool], index: usize) -> bool {
    disabled_items.get(index).copied().unwrap_or(false)
}
