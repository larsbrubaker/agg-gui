//! The names the open caret suggestion list answers to in a widget search,
//! so GUI automation can find it and click a row by name — agg-sharp
//! `TextSuggestionPopup.Name` ("Text Suggestions"), `RowName` and its
//! `FindDescendants` override.
//!
//! The list is drawn by its `TextField`, not a widget of its own, so the
//! field reports the list and each row in view through
//! [`Widget::find_named_targets`](crate::Widget::find_named_targets)
//! (`text_field/widget_impl.rs`) as [`NamedTarget`]s in the field's local
//! space. A row scrolled out of view is not reported, as in C#: the
//! highlight or the wheel brings it into view first. The state the
//! rectangles come from is `super::State`.

use std::rc::Rc;

use super::TextSuggestionController;
use crate::NamedTarget;

impl TextSuggestionController {
    /// The name the open list answers to in a widget search (C#
    /// `TextSuggestionPopup`'s `Name`).
    pub const POPUP_NAME: &'static str = "Text Suggestions";

    /// The name a row showing `label` answers to in a widget search:
    /// `"[red] Suggestion"` (C# `TextSuggestionPopup.RowName`).
    pub fn row_name(label: &str) -> String {
        format!("{label} Suggestion")
    }

    /// Push the open list (named [`POPUP_NAME`](Self::POPUP_NAME)) and every
    /// row in view whose [`row_name`](Self::row_name) is `name` onto `out`,
    /// in the owning field's local space. A row carries its
    /// [`TextSuggestion`](super::TextSuggestion) as the target's object (C#'s
    /// `NamedObject`). Nothing while the list is closed.
    pub(crate) fn named_targets(&self, name: &str, out: &mut Vec<NamedTarget>) {
        let st = self.state.borrow();
        if !st.is_open {
            return;
        }
        if name == Self::POPUP_NAME {
            out.push(NamedTarget::new(st.rect));
        }
        let first = st.first_visible_row;
        for index in first..first + st.visible_row_count() {
            let Some(suggestion) = st.suggestions.suggestions.get(index) else {
                continue;
            };
            if Self::row_name(&suggestion.label) != name {
                continue;
            }
            if let Some(row) = st.row_bounds_local(index) {
                out.push(NamedTarget::new(row).with_object(Rc::new(suggestion.clone())));
            }
        }
    }
}
