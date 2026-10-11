//! Keyboard handling for [`super::ComboBox`], ported from agg-sharp's
//! `DropDownList` (`Gui/Menu/DropDownList.cs`: `OnKeyUp`, and the
//! `DropDownContainer.KeyDown` / `KeyPressed` handlers `ShowMenu` installs).
//!
//! Closed list:
//! - **Down** opens the list on its *release* (C# `OnKeyUp`), so the press's
//!   auto-repeat does not run through the rows; the press itself is taken so a
//!   scrolling parent does not move.
//! - **Enter** / **Space** open the list too.  C# does nothing for them on a
//!   closed list; agg-gui keeps them so the list can be opened from the
//!   keyboard the way native drop-downs are.
//! - **Up** and every other key do nothing.
//!
//! Open list (the keys go to the combo: it is modal while open):
//! - **Up** / **Down** move the highlight one row, stopping at the ends, and
//!   scroll it into view.  The selection does not change.
//! - **Enter** picks the highlighted row and closes; `on_change` fires only
//!   when the selection changed (C#'s `SelectedIndex` setter).
//! - **Escape** closes without changing the selection.
//! - Typed characters build a type-ahead filter: the closed box shows what
//!   was typed and the highlight jumps to the first row whose text starts
//!   with it, ignoring case.  **Backspace** drops the last character.
//!
//! The highlight is C#'s `highLightedIndex`; it is painted as the hovered row,
//! and a row the mouse moves over takes it, so Enter picks the row the user
//! sees highlighted.
//!
//! C# bugs fixed here (each pinned by a `rust_only_*` test in
//! `tests/widgets/combo_keyboard.rs`):
//! - C# never resets `highLightedIndex` when the list opens, so a list
//!   reopened on its selected row steps from wherever the last visit left
//!   off.  Here the highlight starts on the selected row (or the first row).
//! - C# adds its key handlers again on every `ShowMenu`, so on the second
//!   opening one Down moves the highlight two rows.  Here one key is one step.

use super::*;

impl ComboBox {
    /// `Event::KeyDown` while enabled.
    pub(super) fn on_key_down(&mut self, key: &Key) -> EventResult {
        if self.open {
            return self.on_open_key_down(key);
        }
        match key {
            // The list opens on the release; the press is ours.
            Key::ArrowDown => EventResult::Consumed,
            Key::Enter | Key::Char(' ') => {
                self.open_list(true);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// `Event::KeyUp` while enabled: Down released on a closed list opens it.
    pub(super) fn on_key_up(&mut self, key: &Key) -> EventResult {
        if !self.open && *key == Key::ArrowDown {
            self.open_list(true);
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    fn on_open_key_down(&mut self, key: &Key) -> EventResult {
        let n = self.options.len();
        match key {
            Key::ArrowUp => {
                if self.key_highlight > 0 {
                    self.highlight(self.key_highlight - 1);
                }
                EventResult::Consumed
            }
            Key::ArrowDown => {
                if self.key_highlight + 1 < n {
                    self.highlight(self.key_highlight + 1);
                }
                EventResult::Consumed
            }
            Key::Enter => {
                self.pick_highlighted();
                EventResult::Consumed
            }
            Key::Escape => {
                self.close_list();
                EventResult::Consumed
            }
            Key::Backspace => {
                let mut text = self.type_ahead.take().unwrap_or_default();
                text.pop();
                self.apply_type_ahead(text);
                EventResult::Consumed
            }
            Key::Char(c) => {
                let mut text = self.type_ahead.take().unwrap_or_default();
                text.push(*c);
                self.apply_type_ahead(text);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Open the list with the highlight on the selected row (or the first
    /// row when nothing is selected).  `by_keyboard` paints that highlight at
    /// once; a click leaves it to the hover, as before.
    pub(super) fn open_list(&mut self, by_keyboard: bool) {
        self.open = true;
        self.scrollbar.hovered_bar = false;
        self.scrollbar.hovered_thumb = false;
        self.scrollbar.dragging = false;
        self.middle_dragging = false;
        self.type_ahead = None;
        self.key_highlight = if self.has_selection { self.selected } else { 0 };
        self.hovered_item = (by_keyboard && !self.options.is_empty()).then_some(self.key_highlight);
        self.ensure_selected_visible();
        crate::animation::request_draw();
    }

    /// Move the highlight to row `idx` and scroll it into view.
    fn highlight(&mut self, idx: usize) {
        self.key_highlight = idx;
        self.hovered_item = Some(idx);
        self.ensure_index_visible(idx);
        crate::animation::request_draw();
    }

    /// Enter: select the highlighted row and close.
    fn pick_highlighted(&mut self) {
        let idx = self.key_highlight;
        if idx < self.options.len() {
            let changed = !self.has_selection || self.selected != idx;
            if changed {
                self.set_selected(idx);
            }
            self.close_list();
            if changed {
                self.fire();
            }
        } else {
            self.close_list();
        }
    }

    /// C# `ApplyFilter`: show `text` in the closed box and highlight the
    /// first row starting with it, ignoring case (an empty filter matches
    /// the first row, as C#'s `IndexOf("") == 0` does).
    fn apply_type_ahead(&mut self, text: String) {
        self.selected_label.set_text(&text);
        let needle = text.to_lowercase();
        let hit = self
            .options
            .iter()
            .position(|o| o.to_lowercase().starts_with(&needle));
        self.type_ahead = Some(text);
        if let Some(idx) = hit {
            self.highlight(idx);
        } else {
            crate::animation::request_draw();
        }
    }

    /// The list is closing: the closed box goes back from the typed filter
    /// to the selection (or the placeholder).
    pub(super) fn end_type_ahead(&mut self) {
        if self.type_ahead.take().is_none() {
            return;
        }
        if self.has_selection {
            self.set_selected(self.selected);
        } else {
            self.clear_selection();
        }
    }
}
