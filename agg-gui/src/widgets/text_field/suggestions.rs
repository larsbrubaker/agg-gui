//! The field side of caret suggestions: attaching a
//! [`TextSuggestionController`], the key preview that runs before the
//! field's own key handling, requerying after edits and caret moves, the
//! accept (one undo step of its own) and the list's overlay paint, hit test,
//! click and wheel. Ports the event wiring of agg-sharp
//! `TextSuggestionController.cs` and the input half of
//! `TextSuggestionPopup.cs`; the controller's state is
//! [`crate::widgets::text_suggestion`].

use super::*;
use crate::geometry::Point;
use crate::widgets::text_suggestion::TextSuggestionController;

/// A controller attached to a field: closing its list when the field is
/// dropped, so `is_open()` / `popup_bounds()` never report a list nobody shows.
pub(super) struct AttachedSuggestions(pub(super) TextSuggestionController);

impl Drop for AttachedSuggestions {
    fn drop(&mut self) {
        self.0.close();
    }
}

impl TextField {
    /// Offer `controller`'s suggestions below the caret while this field is
    /// edited. Keep a clone of the controller to read what it shows.
    /// The list closes when the field is dropped.
    pub fn with_text_suggestions(mut self, controller: TextSuggestionController) -> Self {
        self.suggestions = Some(AttachedSuggestions(controller));
        self
    }

    /// The attached suggestion controller, if any.
    pub fn text_suggestions(&self) -> Option<&TextSuggestionController> {
        self.suggestions.as_ref().map(|a| &a.0)
    }

    pub(super) fn suggest_ctrl(&self) -> Option<TextSuggestionController> {
        self.text_suggestions().cloned()
    }

    /// Replace the bytes `[start, start + len)` with `insert` as one undo step
    /// of its own, the way an edit by the user is (C# `ReplaceRange`): the
    /// selection ends and the caret goes after the inserted text. Offsets are
    /// **UTF-8 byte offsets** (as [`cursor_pos`](Self::cursor_pos) and the
    /// suggestion lists), clamped to the text and outward to character
    /// boundaries.
    ///
    /// Like C#, a read-only field is left alone, line endings in `insert`
    /// are normalized to `\n` (`TextLineEndings.Normalize`), and a
    /// replacement that changes nothing only moves the caret: no undo step,
    /// no change notification. The field's [`char_filter`](Self::with_char_filter)
    /// also applies (C# has no per-field filter on this path; agg-gui's filter
    /// promises that every insertion, typed or pasted, is filtered).
    pub fn replace_range(&mut self, start: usize, len: usize, insert: &str) {
        if self.read_only {
            return;
        }
        let insert = self.apply_char_filter(&insert.replace("\r\n", "\n").replace('\r', "\n"));
        self.flush_pending();
        let before = self.snap();
        let changed = {
            let mut st = self.edit.borrow_mut();
            let mut lo = start.min(st.text.len());
            while !st.text.is_char_boundary(lo) {
                lo -= 1;
            }
            let mut hi = start.saturating_add(len).min(st.text.len()).max(lo);
            while !st.text.is_char_boundary(hi) {
                hi += 1;
            }
            let changed = st.text[lo..hi] != *insert;
            if changed {
                st.text.replace_range(lo..hi, &insert);
            }
            st.cursor = lo + insert.len();
            st.anchor = st.cursor;
            changed
        };
        if changed {
            let after = self.snap();
            self.undo.add(Box::new(TextEditCommand {
                name: "replace text",
                before,
                after,
                target: Rc::clone(&self.edit),
            }));
        }
        self.ensure_cursor_visible();
        crate::animation::request_draw();
        if changed {
            self.notify_change();
        }
    }

    /// Where the text at UTF-8 byte offset `index` starts, in this field's
    /// local space (clamped back to a character boundary):
    /// one pixel wide, the height of the text line (ascender to descender).
    /// The suggestion list hangs from here.
    pub fn line_bounds_at(&self, index: usize) -> Rect {
        let font = self.active_font();
        let st = self.edit.borrow();
        let mut i = index.min(st.text.len());
        while !st.text.is_char_boundary(i) {
            i -= 1;
        }
        let x = self.text_insets().left - self.scroll_x
            + measure_advance(&font, &st.text[..i], self.font_size);
        let ascent = font.ascender_px(self.font_size);
        let descent = font.descender_px(self.font_size);
        let baseline = self.text_center_y(self.bounds.height) - (ascent - descent) * 0.5;
        Rect::new(x, baseline - descent, 1.0, ascent + descent)
    }

    /// C# `Requery`: ask the provider at the caret, showing the answer or
    /// closing when there is none (or the field is not focused).
    pub(super) fn suggest_requery(&mut self) {
        let Some(ctrl) = self.suggest_ctrl() else {
            return;
        };
        if !self.focused {
            ctrl.close();
            return;
        }
        let (text, caret) = {
            let st = self.edit.borrow();
            (st.text.clone(), st.cursor)
        };
        if ctrl.requery(&text, caret, (self.active_font(), self.font_size)) {
            self.suggest_place();
        }
        crate::animation::request_draw();
    }

    fn suggest_place(&self) {
        if let Some(ctrl) = self.text_suggestions() {
            let start = ctrl.state.borrow().suggestions.replace_start;
            ctrl.place(self.line_bounds_at(start));
        }
    }

    /// C# `Accept`: replace the list's span with row `index`'s insert text,
    /// then ask again at once, so an insert ending in '.' shows what follows.
    pub(super) fn suggest_accept(&mut self, index: usize) {
        let Some(ctrl) = self.suggest_ctrl() else {
            return;
        };
        let (start, len, insert) = {
            let st = ctrl.state.borrow();
            match st.suggestions.suggestions.get(index) {
                Some(s) if st.is_open => (
                    st.suggestions.replace_start,
                    st.suggestions.replace_length,
                    s.insert_text.clone(),
                ),
                _ => return,
            }
        };
        ctrl.state.borrow_mut().accepting = true;
        self.replace_range(start, len, &insert);
        ctrl.state.borrow_mut().accepting = false;
        ctrl.close();
        self.suggest_requery();
    }

    /// C# `Field_EditedOrCaretMoved`: an open list refilters after any edit
    /// or caret move that is not an accept's own.
    pub(super) fn suggest_after_edit(&mut self) {
        let refilter = self
            .text_suggestions()
            .is_some_and(|c| c.is_open() && !c.state.borrow().accepting);
        if refilter {
            self.suggest_requery();
        }
    }

    /// C# `Field_PreviewKeyDown`. `true` = the key was taken (handled and its
    /// character suppressed); the field must not act on it.
    pub(super) fn suggest_preview_key(&mut self, key: &Key, mods: Modifiers) -> bool {
        let Some(ctrl) = self.suggest_ctrl() else {
            return false;
        };
        if *key == Key::Char(' ') && mods.ctrl && !mods.alt && !mods.shift {
            if !self.read_only {
                // Forget the last query so an open list is asked again too.
                ctrl.forget_last_query();
                self.suggest_requery();
            }
            // Never a typed space, whatever the provider answered.
            return true;
        }
        if !ctrl.is_open() || mods.ctrl || mods.alt || mods.meta {
            return false;
        }
        let page = ctrl.visible_row_count().saturating_sub(1).max(1) as i64;
        match key {
            Key::ArrowDown if !mods.shift => ctrl.move_highlight(1, true),
            Key::ArrowUp if !mods.shift => ctrl.move_highlight(-1, true),
            Key::PageDown if !mods.shift => ctrl.move_highlight(page, false),
            Key::PageUp if !mods.shift => ctrl.move_highlight(-page, false),
            Key::Enter | Key::Tab if !mods.shift => {
                let index = ctrl.highlight_index();
                self.suggest_accept(index);
                return true;
            }
            Key::Tab => {
                // Shift+Tab keeps doing what it does in the field (focus the
                // previous control); the list must not be left hanging.
                ctrl.close();
                return false;
            }
            Key::Escape => {
                ctrl.close();
                return true;
            }
            _ => return false,
        }
        // The footer comes and goes with the highlighted row's description.
        self.suggest_place();
        true
    }

    /// Paint the open list (from `paint_global_overlay`), recording where the
    /// field sits in root space so the list clamps into the window.
    pub(super) fn suggest_paint(&mut self, ctx: &mut dyn DrawCtx) {
        let Some(ctrl) = self.suggest_ctrl() else {
            return;
        };
        if !ctrl.is_open() {
            return;
        }
        let (mut x, mut y) = (0.0, 0.0);
        crate::widget::logical_root_transform(ctx).transform(&mut x, &mut y);
        ctrl.set_root_frame(
            Point::new(x, y),
            crate::widgets::combo_box::current_combo_viewport(),
        );
        // Placed every frame from the field's own transform, so the list
        // follows the field when the window resizes and the word as it scrolls.
        self.suggest_place();
        ctrl.paint(ctx);
    }

    /// Pointer events on the open list: a press on a row accepts it, the
    /// wheel scrolls the rows. `Some` when the event was the list's.
    pub(super) fn suggest_pointer(&mut self, event: &Event) -> Option<EventResult> {
        let ctrl = self.suggest_ctrl()?;
        match event {
            Event::MouseDown { pos, .. } if ctrl.contains(*pos) => {
                if let Some(index) = ctrl.row_at(*pos) {
                    self.suggest_accept(index);
                }
                Some(EventResult::Consumed)
            }
            // The arrow, not the I-beam, over the list: it is not text.
            Event::MouseMove { pos } if !self.mouse_down && ctrl.contains(*pos) => {
                self.hovered = false;
                crate::cursor::set_cursor_icon(crate::cursor::CursorIcon::Default);
                Some(EventResult::Ignored)
            }
            Event::MouseWheel { pos, delta_y, .. } if ctrl.contains(*pos) => {
                ctrl.wheel(*delta_y);
                self.suggest_place();
                Some(EventResult::Consumed)
            }
            _ => None,
        }
    }
}
