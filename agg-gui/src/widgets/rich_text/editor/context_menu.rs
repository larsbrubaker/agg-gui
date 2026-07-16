//! Right-click Cut/Copy/Paste/Select-All for [`RichTextEdit`].
//!
//! Presentation and event plumbing reuse the shared
//! [`crate::widgets::text_context_menu`] module, exactly as `TextField` and
//! `TextArea` do. Cut/Copy/Paste delegate to the SAME styled-clipboard helpers
//! the keyboard chords use ([`copy_selection`](RichTextEdit::copy_selection) /
//! [`paste_clipboard`](RichTextEdit::paste_clipboard) in `input.rs`), so the
//! menu gets identical rich-fragment behaviour with no duplicated logic. Select
//! All uses the core's programmatic path.

use web_time::Instant;

use crate::event::{Event, EventResult};
use crate::geometry::Point;
use crate::widgets::text_context_menu::TextMenuAction;

use super::super::model::{DocPos, InlineStyle};
use super::RichTextEdit;

impl RichTextEdit {
    /// Copy the selection (styled fragment + plain text) — same as Ctrl+C.
    pub(super) fn menu_copy(&mut self) {
        self.copy_selection();
    }

    /// Cut = styled copy then delete the selection — same as Ctrl+X.
    pub(super) fn menu_cut(&mut self) {
        if self.copy_selection() {
            self.core.borrow_mut().backspace();
        }
    }

    /// Paste the clipboard (styled fragment when it matches our last copy,
    /// else external plain text) — same as Ctrl+V.
    pub(super) fn menu_paste(&mut self) {
        self.paste_clipboard();
    }

    /// Select the whole document (Ctrl+A).
    pub(super) fn menu_select_all(&mut self) {
        self.core.borrow_mut().select_all();
    }

    fn has_selection(&self) -> bool {
        !self.core.borrow().selection().is_empty()
    }

    /// `true` when `pos` falls inside the current (non-empty) selection.
    fn selection_contains(&self, pos: DocPos) -> bool {
        let sel = self.core.borrow().selection();
        let (lo, hi) = sel.ordered();
        !sel.is_empty() && pos >= lo && pos <= hi
    }

    /// Open the right-click menu at `pos` (widget-local). A right-click outside
    /// the current selection first moves the caret there; a right-click on an
    /// existing selection keeps it.
    pub(super) fn open_context_menu(&mut self, pos: Point) {
        let target = self.hit_test_pos(pos);
        if !self.selection_contains(target) {
            self.core.borrow_mut().set_caret(target, false);
        }
        let has_sel = self.has_selection();
        // The editor is always editable.
        self.context_menu.open(pos, has_sel, true);
        self.focus_time = Some(Instant::now());
        crate::animation::request_draw();
    }

    /// Dispatch a chosen menu item through the same core operations as the
    /// keyboard chords.
    pub(super) fn apply_text_menu_action(&mut self, action: TextMenuAction) {
        match action {
            TextMenuAction::Cut => self.menu_cut(),
            TextMenuAction::Copy => self.menu_copy(),
            TextMenuAction::Paste => self.menu_paste(),
            TextMenuAction::SelectAll => self.menu_select_all(),
        }
        let caret = self.core.borrow().caret();
        self.ensure_pos_visible(caret);
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

    /// The UI font for painting the menu — the system font, falling back to the
    /// resolver's default-style face.
    pub(super) fn context_menu_font(&self) -> std::sync::Arc<crate::text::Font> {
        crate::font_settings::current_system_font()
            .unwrap_or_else(|| (self.resolver)(&InlineStyle::default()))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::event::{Event, EventResult, Modifiers, MouseButton};
    use crate::geometry::{Point, Size};
    use crate::text::Font;
    use crate::widget::Widget;
    use crate::widgets::rich_text::model::{Block, InlineStyle, RichDoc};
    use crate::widgets::rich_text::view::SharedResolver;
    use crate::widgets::text_context_menu::TextMenuAction;

    use super::RichTextEdit;

    const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/Arial-Regular.ttf");

    fn resolver() -> SharedResolver {
        let f = Arc::new(Font::from_slice(FONT_BYTES).expect("test font"));
        std::rc::Rc::new(move |_: &InlineStyle| Arc::clone(&f))
    }

    fn laid_out(text: &str) -> RichTextEdit {
        crate::widget::set_current_viewport(Size::new(800.0, 600.0));
        let doc = RichDoc::from_blocks(vec![Block::plain(text)]);
        let mut ed = RichTextEdit::new(doc, resolver()).with_font_size(16.0);
        ed.layout(Size::new(400.0, 120.0));
        ed.on_event(&Event::FocusGained);
        ed
    }

    fn right_click(ed: &mut RichTextEdit, x: f64, y: f64) -> EventResult {
        ed.on_event(&Event::MouseDown {
            pos: Point::new(x, y),
            button: MouseButton::Right,
            modifiers: Modifiers::default(),
        })
    }

    #[test]
    fn right_click_opens_menu_through_event_routing() {
        let mut ed = laid_out("hello world");
        assert_eq!(right_click(&mut ed, 20.0, 60.0), EventResult::Consumed);
        assert!(ed.has_active_modal());
        assert!(ed.context_menu.is_open());
    }

    #[test]
    fn context_menu_opt_out() {
        crate::widget::set_current_viewport(Size::new(800.0, 600.0));
        let doc = RichDoc::from_blocks(vec![Block::plain("hello")]);
        let mut ed = RichTextEdit::new(doc, resolver())
            .with_font_size(16.0)
            .with_context_menu(false);
        ed.layout(Size::new(400.0, 120.0));
        ed.on_event(&Event::FocusGained);
        assert_eq!(right_click(&mut ed, 20.0, 60.0), EventResult::Ignored);
        assert!(!ed.has_active_modal());
    }

    #[test]
    fn copy_item_copies_selection_via_clipboard() {
        let mut ed = laid_out("hello world");
        ed.menu_select_all();
        right_click(&mut ed, 20.0, 60.0);
        assert!(ed.context_menu.is_open());
        crate::clipboard::set_text("SENTINEL");
        let center = ed
            .context_menu
            .action_row_center(TextMenuAction::Copy)
            .expect("Copy row is laid out");
        let r = ed.on_event(&Event::MouseDown {
            pos: center,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        assert_eq!(r, EventResult::Consumed);
        assert_eq!(crate::clipboard::get_text().as_deref(), Some("hello world"));
        assert!(!ed.context_menu.is_open());
    }
}
