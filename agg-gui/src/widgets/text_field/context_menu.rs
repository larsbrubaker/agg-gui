//! Right-click Cut/Copy/Paste/Select-All for [`TextField`].
//!
//! The clipboard verbs here are the SINGLE implementation shared by both the
//! keyboard shortcuts (`handle_key`) and the [`TextContextMenu`] items, so the
//! two entry points can never drift. Presentation and event plumbing live in
//! the shared [`crate::widgets::text_context_menu`] module; this file only maps
//! the menu's actions onto the field's existing edit primitives.

use super::*;
use crate::geometry::Point;
use crate::widgets::text_context_menu::TextMenuAction;

impl TextField {
    /// Copy the current selection to the clipboard (no-op without a selection).
    /// Available even when read-only.
    pub(super) fn clipboard_copy(&mut self) {
        if self.has_selection() {
            clipboard_set(&self.selection());
        }
    }

    /// Cut = copy the selection then delete it. Only mutates when editable and a
    /// selection exists.
    pub(super) fn clipboard_cut(&mut self) {
        if !self.read_only && self.has_selection() {
            clipboard_set(&self.selection());
            self.do_delete(false, false);
        }
    }

    /// Paste the clipboard text at the caret / over the selection. No-op when
    /// read-only or the clipboard is empty.
    pub(super) fn clipboard_paste(&mut self) {
        if self.read_only {
            return;
        }
        if let Some(clip) = clipboard_get() {
            self.do_insert(&clip, false);
        }
    }

    /// Select the whole field (Ctrl+A).
    pub(super) fn select_all_text(&mut self) {
        let len = self.edit.borrow().text.len();
        let mut st = self.edit.borrow_mut();
        st.anchor = 0;
        st.cursor = len;
    }

    /// `true` when `offset` falls inside the current (non-empty) selection.
    fn selection_contains(&self, offset: usize) -> bool {
        let st = self.edit.borrow();
        let (lo, hi) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
        lo != hi && offset >= lo && offset <= hi
    }

    /// Open the right-click menu at `pos` (widget-local). A right-click outside
    /// the current selection first moves the caret there (standard); a
    /// right-click on an existing selection keeps it.
    pub(super) fn open_context_menu(&mut self, pos: Point) {
        let tx = pos.x - self.padding + self.scroll_x;
        let text = self.edit.borrow().text.clone();
        let click = self.click_to_cursor(&text, tx);
        if !self.selection_contains(click) {
            let mut st = self.edit.borrow_mut();
            st.cursor = click;
            st.anchor = click;
        }
        let has_sel = self.has_selection();
        let editable = !self.read_only;
        self.context_menu.open(pos, has_sel, editable);
        // Keep the caret solid while the menu is up.
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
    /// was open (whether or not it consumed the event); `None` when closed so
    /// the caller proceeds with normal handling.
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
            // Menu ignored the event (e.g. a key it doesn't handle) — let the
            // field's own handling run.
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::Widget;

    const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

    fn font() -> Arc<Font> {
        Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
    }

    fn laid_out(mut tf: TextField) -> TextField {
        crate::widget::set_current_viewport(Size::new(800.0, 600.0));
        tf.layout(Size::new(200.0, 28.0));
        tf.set_bounds(Rect::new(0.0, 0.0, 200.0, 28.0));
        tf.on_event(&Event::FocusGained);
        tf
    }

    fn right_click(tf: &mut TextField, x: f64, y: f64) -> EventResult {
        tf.on_event(&Event::MouseDown {
            pos: Point::new(x, y),
            button: MouseButton::Right,
            modifiers: Modifiers::default(),
        })
    }

    #[test]
    fn right_click_opens_menu_through_event_routing() {
        let mut tf = laid_out(TextField::new(font()).with_text("hello world"));
        assert_eq!(right_click(&mut tf, 20.0, 14.0), EventResult::Consumed);
        assert!(tf.has_active_modal());
        assert!(tf.context_menu.is_open());
    }

    #[test]
    fn context_menu_opt_out() {
        let mut tf = laid_out(
            TextField::new(font())
                .with_text("hello")
                .with_context_menu(false),
        );
        assert_eq!(right_click(&mut tf, 20.0, 14.0), EventResult::Ignored);
        assert!(!tf.has_active_modal());
    }

    #[test]
    fn read_only_field_disables_cut_and_paste() {
        let mut tf = laid_out(
            TextField::new(font())
                .with_text("hello world")
                .with_read_only(true),
        );
        tf.select_all_text();
        right_click(&mut tf, 20.0, 14.0);
        assert!(tf.context_menu.is_open());
        assert_eq!(
            tf.context_menu.action_enabled(TextMenuAction::Copy),
            Some(true)
        );
        assert_eq!(
            tf.context_menu.action_enabled(TextMenuAction::Cut),
            Some(false)
        );
        assert_eq!(
            tf.context_menu.action_enabled(TextMenuAction::Paste),
            Some(false)
        );
    }

    #[test]
    fn copy_item_copies_selection_via_clipboard() {
        let mut tf = laid_out(TextField::new(font()).with_text("hello world"));
        tf.select_all_text();
        right_click(&mut tf, 20.0, 14.0);
        assert!(tf.context_menu.is_open());
        crate::clipboard::set_text("SENTINEL");
        let center = tf
            .context_menu
            .action_row_center(TextMenuAction::Copy)
            .expect("Copy row is laid out");
        let r = tf.on_event(&Event::MouseDown {
            pos: center,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        assert_eq!(r, EventResult::Consumed);
        assert_eq!(crate::clipboard::get_text().as_deref(), Some("hello world"));
        assert!(!tf.context_menu.is_open());
    }
}
