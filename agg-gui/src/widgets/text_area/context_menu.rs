//! Right-click Cut/Copy/Paste/Select-All for [`TextArea`].
//!
//! The clipboard verbs are the SINGLE implementation shared by both the
//! keyboard shortcuts (`KeyDown` handling in `widget_impl`) and the
//! [`TextContextMenu`] items, so the two entry points can never drift.
//! Presentation and event plumbing live in the shared
//! [`crate::widgets::text_context_menu`] module. A `TextArea` is always
//! editable, so Cut/Paste are only gated on the selection state.

use super::*;
use crate::widgets::text_context_menu::TextMenuAction;

impl TextArea {
    /// Enable or disable the default right-click Cut/Copy/Paste/Select-All
    /// context menu. On by default.
    pub fn with_context_menu(mut self, v: bool) -> Self {
        self.context_menu_enabled = v;
        self
    }

    /// Copy the selection to the clipboard (no-op with an empty selection).
    pub(super) fn clipboard_copy(&mut self) {
        let sel = self.selected_text();
        if !sel.is_empty() {
            clipboard_set(&sel);
        }
    }

    /// Cut = copy the selection then delete it.
    pub(super) fn clipboard_cut(&mut self) {
        let sel = self.selected_text();
        if !sel.is_empty() {
            clipboard_set(&sel);
            self.delete(0);
        }
    }

    /// Paste the clipboard text at the caret / over the selection.
    pub(super) fn clipboard_paste(&mut self) {
        if let Some(t) = clipboard_get() {
            self.insert_str(&t);
        }
    }

    /// Select the whole buffer (Ctrl+A).
    pub(super) fn select_all_text(&mut self) {
        let len = self.edit.borrow().text.len();
        self.move_cursor_to(0, false);
        self.move_cursor_to(len, true);
    }

    /// `true` when `offset` falls inside the current (non-empty) selection.
    fn selection_contains(&self, offset: usize) -> bool {
        match self.selection() {
            Some((lo, hi)) => lo != hi && offset >= lo && offset <= hi,
            None => false,
        }
    }

    /// Open the right-click menu at `pos` (widget-local). A right-click outside
    /// the current selection first moves the caret there; a right-click on an
    /// existing selection keeps it.
    pub(super) fn open_context_menu(&mut self, pos: Point) {
        let off = self.byte_offset_at(pos);
        if !self.selection_contains(off) {
            self.move_cursor_to(off, false);
        }
        let has_sel = self.selection().map(|(l, h)| l != h).unwrap_or(false);
        // A TextArea has no read-only mode: it is always editable.
        self.context_menu.open(pos, has_sel, true);
        self.focus_time = Some(Instant::now());
        crate::animation::request_draw();
    }

    /// Dispatch a chosen menu item through the same verbs as the shortcuts.
    pub(super) fn apply_text_menu_action(&mut self, action: TextMenuAction) {
        match action {
            TextMenuAction::Cut => self.clipboard_cut(),
            TextMenuAction::Copy => self.clipboard_copy(),
            TextMenuAction::Paste => self.clipboard_paste(),
            TextMenuAction::SelectAll => self.select_all_text(),
        }
        self.ensure_cursor_visible();
        crate::animation::request_draw();
    }

    /// Route an event into the open menu. Returns `Some(result)` when the menu
    /// consumed it; `None` otherwise so the caller proceeds with normal
    /// handling.
    pub(super) fn route_context_menu(&mut self, event: &Event) -> Option<EventResult> {
        if !self.context_menu.is_open() {
            return None;
        }
        let (result, action) = self.context_menu.handle_event(event);
        if let Some(action) = action {
            self.apply_text_menu_action(action);
        }
        if result.is_consumed() {
            crate::animation::request_draw();
            Some(result)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, EventResult, MouseButton};
    use crate::widget::Widget;
    use crate::widgets::text_context_menu::TextMenuAction;
    use std::sync::Arc;

    const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

    fn font() -> Arc<Font> {
        Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
    }

    fn laid_out(text: &str) -> TextArea {
        crate::widget::set_current_viewport(Size::new(800.0, 600.0));
        let mut ta = TextArea::new(font()).with_text(text);
        ta.layout(Size::new(200.0, 100.0));
        ta.on_event(&Event::FocusGained);
        ta
    }

    fn right_click(ta: &mut TextArea, x: f64, y: f64) -> EventResult {
        ta.on_event(&Event::MouseDown {
            pos: Point::new(x, y),
            button: MouseButton::Right,
            modifiers: Modifiers::default(),
        })
    }

    #[test]
    fn right_click_opens_menu_through_event_routing() {
        let mut ta = laid_out("hello world");
        assert_eq!(right_click(&mut ta, 20.0, 50.0), EventResult::Consumed);
        assert!(ta.has_active_modal());
        assert!(ta.context_menu.is_open());
    }

    #[test]
    fn context_menu_opt_out() {
        crate::widget::set_current_viewport(Size::new(800.0, 600.0));
        let mut ta = TextArea::new(font())
            .with_text("hello")
            .with_context_menu(false);
        ta.layout(Size::new(200.0, 100.0));
        ta.on_event(&Event::FocusGained);
        assert_eq!(right_click(&mut ta, 20.0, 50.0), EventResult::Ignored);
        assert!(!ta.has_active_modal());
    }

    #[test]
    fn copy_item_copies_selection_via_clipboard() {
        let mut ta = laid_out("hello world");
        ta.select_all_text();
        right_click(&mut ta, 20.0, 50.0);
        assert!(ta.context_menu.is_open());
        // Seed a sentinel so we prove the menu's Copy actually wrote.
        crate::clipboard::set_text("SENTINEL");
        // Click the Copy row for real, routed through the widget's on_event so
        // the whole modal-capture → menu → verb path runs.
        let center = ta
            .context_menu
            .action_row_center(TextMenuAction::Copy)
            .expect("Copy row is laid out");
        let r = ta.on_event(&Event::MouseDown {
            pos: center,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        assert_eq!(r, EventResult::Consumed);
        assert_eq!(crate::clipboard::get_text().as_deref(), Some("hello world"));
        assert!(!ta.context_menu.is_open(), "activating Copy closes the menu");
    }
}
