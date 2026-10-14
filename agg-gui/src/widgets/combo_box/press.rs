//! Left-press handling for [`super::ComboBox`]: the closed box toggles the
//! drop-down, a press in the open list's scrollbar drags or pages it, a press
//! on a row picks it (a disabled row is ignored and the list stays open), and a press anywhere else closes the drop-down and is
//! consumed — the widget under it sees neither the press nor its release
//! (the combo is modal while open, `has_active_modal`), as a native drop-down
//! list behaves.
//!
//! Split out of `combo_box.rs` to keep that file under the 800-line limit.

use super::*;

impl ComboBox {
    pub(super) fn on_left_press(&mut self, pos: Point) -> EventResult {
        if self.in_button(pos) {
            if self.open {
                self.close_list();
            } else {
                self.open_list(false);
            }
            return EventResult::Consumed;
        }
        if self.open {
            // The list's rows and scroll bar are tested where they are drawn.
            let pos = self.to_popup(pos);
            if self.pos_in_scrollbar(pos) {
                let style = self.popup_scroll_style();
                let viewport = self.popup_scroll_viewport();
                let geom = self.scrollbar_geometry(style);
                self.sync_scrollbar_from_rows();
                if self.scrollbar.begin_drag(pos, viewport, style, geom) {
                    // No visible effect until the cursor moves.
                } else if self.scrollbar.page_at(pos, viewport, style, geom) {
                    self.sync_rows_from_scrollbar();
                }
                self.hovered_item = None;
                self.scrollbar.hovered_thumb = self.pos_on_scroll_thumb(pos);
                crate::animation::request_draw();
                return EventResult::Consumed;
            }
            if let Some(i) = self.item_for_pos(pos) {
                if !self.item_enabled(i) {
                    // A disabled row: the list stays up for another pick
                    // (`combo_box/item_enabled.rs`).
                    return EventResult::Consumed;
                }
                // Route through `set_selected` so the closed
                // combo's preview label is rebuilt with the
                // newly-selected per-item font (when item_fonts
                // is set).  Direct `self.selected = i` would
                // change the index without swapping the face,
                // leaving the closed combo showing the new
                // name in the OLD typeface — the bug visible
                // when the System window's font picker showed
                // e.g. "Bangers" in Cascadia Code.
                self.set_selected(i);
                self.close_list();
                self.fire();
                return EventResult::Consumed;
            }
            // Press outside the dropdown — close it, and consume the
            // press so the widget beneath sees nothing (native drop-down
            // lists do the same).
            self.close_list();
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    /// A middle or right press.  Middle in the open list starts a drag-scroll;
    /// any other non-left press while open closes the drop-down and is
    /// consumed, the desktop convention the menus follow
    /// (`menu/state.rs`).  A press on the closed box is left alone.
    pub(super) fn on_other_press(&mut self, button: MouseButton, pos: Point) -> EventResult {
        let pos = self.to_popup(pos);
        if button == MouseButton::Middle && self.pos_in_popup(pos) {
            self.middle_dragging = true;
            self.middle_last_pos = pos;
            self.hovered_item = None;
            crate::animation::request_draw();
            return EventResult::Consumed;
        }
        if !self.open {
            return EventResult::Ignored;
        }
        self.close_list();
        EventResult::Consumed
    }
}
