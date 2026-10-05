//! Optional "no selection" state for [`super::ComboBox`].
//!
//! By default a `ComboBox` always has a selected option.  Apps porting
//! dropdowns that start empty (e.g. MatterCAD's `MHDropDownList`, which shows
//! a `noSelectionString` while `SelectedIndex == -1`) opt in with
//! [`ComboBox::with_no_selection`]: the closed box shows the placeholder text,
//! no row is highlighted in the open list, and [`ComboBox::selected_index`]
//! reports `None` until the user (or the app) picks an option.  Combos that
//! never call these methods behave exactly as before.
//!
//! Split out of `combo_box.rs` to keep that file under the 800-line limit.

use super::*;

impl ComboBox {
    /// Start with nothing selected, showing `placeholder` in the closed box.
    /// The selection returns once an option is picked (click, arrow key or
    /// [`ComboBox::set_selected`]).
    pub fn with_no_selection(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self.clear_selection();
        self
    }

    /// Clear the selection: the closed box shows the placeholder set by
    /// [`ComboBox::with_no_selection`] (empty if none was set) and
    /// [`ComboBox::selected_index`] becomes `None`.  Does not fire `on_change`.
    pub fn clear_selection(&mut self) {
        self.has_selection = false;
        self.selected_label =
            Self::make_label(&self.placeholder, self.font_size, Arc::clone(&self.font));
    }

    /// The selected option, or `None` while nothing is selected.
    /// ([`ComboBox::selected`] keeps returning a valid fallback index for
    /// callers that never use the no-selection state.)
    pub fn selected_index(&self) -> Option<usize> {
        self.has_selection.then_some(self.selected)
    }

    /// Select `Some(index)` (as [`ComboBox::set_selected`]) or clear the
    /// selection with `None`.  Does not fire `on_change`.
    pub fn set_selected_index(&mut self, index: Option<usize>) {
        match index {
            Some(i) => self.set_selected(i),
            None => self.clear_selection(),
        }
    }

    /// The text shown in the closed box while nothing is selected.
    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    /// An arrow key with nothing selected selects the first option.
    pub(super) fn select_first_from_none(&mut self) {
        if self.options.is_empty() {
            return;
        }
        self.set_selected(0);
        self.ensure_selected_visible();
        self.fire();
        crate::animation::request_draw();
    }

    /// Track keyboard focus for [`super::ComboBoxStateStyle::focus_border`].
    pub(super) fn set_focused(&mut self, focused: bool) {
        if self.focused != focused {
            self.focused = focused;
            if self.state_style.focus_border.is_some() {
                crate::animation::request_draw();
            }
        }
    }
}
