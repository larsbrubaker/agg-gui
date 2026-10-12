//! Input-filtering helpers + callback-dispatch helpers split out
//! of `text_field.rs` to keep that file under the 800-line cap.
//!
//! Two filters: the per-character allow-list ([`TextField::with_char_filter`])
//! and the whole-edit judge ([`TextField::with_text_filter`]), which sees the
//! text an insertion would leave behind — what agg-sharp's
//! `InternalNumberEdit.CanType` judges (see `widgets/number_edit`).

use super::*;

/// The whole-edit judge: `(inserted, resulting_text) -> accept`.
pub(super) type TextFilter = dyn Fn(&str, &str) -> bool;

impl TextField {
    /// Install a per-character allow-list. Any character for which
    /// `filter` returns `false` is silently dropped from
    /// insertions — both single-character typing and multi-character
    /// paste. The text already in the field at install time is left
    /// alone.
    ///
    /// Common patterns:
    /// - decimal digits: `|c| c.is_ascii_digit()`
    /// - hex seed with `0x` prefix: `|c| c.is_ascii_hexdigit() || c == 'x' || c == 'X'`
    /// - single-line: `|c| c != '\n' && c != '\r'`
    pub fn with_char_filter(mut self, filter: impl Fn(char) -> bool + 'static) -> Self {
        self.char_filter = Some(Rc::new(filter));
        self
    }

    /// Install a whole-edit judge. Before any insertion lands — a typed
    /// character, a paste, a [`replace_range`](Self::replace_range) — the
    /// filter is called with the inserted text (after the
    /// [`char_filter`](Self::with_char_filter)) and the text the field would
    /// hold afterwards (the selection replaced, or the insertion at the
    /// caret). Returning `false` refuses the edit whole: the text, caret and
    /// undo history stay exactly as they were, and a typed key is still
    /// consumed (C#'s refused `KeyPress` is `Handled`). Deletions and
    /// programmatic [`set_text`](Self::set_text) are not judged.
    pub fn with_text_filter(mut self, filter: impl Fn(&str, &str) -> bool + 'static) -> Self {
        self.text_filter = Some(Rc::new(filter));
        self
    }

    /// Treat the current text as committed: a later focus loss fires
    /// `on_edit_complete` only if the text changes again (C#
    /// `OnEditComplete`'s `textWhenGotFocus = Text`). A wrapper that rewrites
    /// the text while committing it (a number box writing back its parsed
    /// value) calls this afterwards.
    pub fn mark_text_committed(&mut self) {
        self.text_on_focus = self.text();
    }

    /// Whether the text filter accepts inserting `s` at the caret (over the
    /// selection). No filter installed → accepted.
    pub(crate) fn text_filter_accepts(&self, s: &str) -> bool {
        let Some(filter) = &self.text_filter else {
            return true;
        };
        let st = self.edit.borrow();
        let lo = st.cursor.min(st.anchor);
        let hi = st.cursor.max(st.anchor);
        let mut result = String::with_capacity(st.text.len() + s.len());
        result.push_str(&st.text[..lo]);
        result.push_str(s);
        result.push_str(&st.text[hi..]);
        drop(st);
        filter(s, &result)
    }

    /// [`text_filter_accepts`](Self::text_filter_accepts) for
    /// `replace_range(start, len, insert)`, with its clamping.
    pub(crate) fn text_filter_accepts_range(&self, start: usize, len: usize, insert: &str) -> bool {
        let Some(filter) = &self.text_filter else {
            return true;
        };
        let st = self.edit.borrow();
        let mut lo = start.min(st.text.len());
        while !st.text.is_char_boundary(lo) {
            lo -= 1;
        }
        let mut hi = start.saturating_add(len).min(st.text.len()).max(lo);
        while !st.text.is_char_boundary(hi) {
            hi += 1;
        }
        let mut result = st.text.clone();
        result.replace_range(lo..hi, insert);
        drop(st);
        filter(insert, &result)
    }

    /// Apply the configured `char_filter` (if any) to `s`, returning
    /// the resulting string. No filter installed → returns `s` as-is.
    pub(crate) fn apply_char_filter(&self, s: &str) -> String {
        match &self.char_filter {
            Some(f) => s.chars().filter(|c| f(*c)).collect(),
            None => s.to_string(),
        }
    }

    pub(crate) fn notify_change(&mut self) {
        let t = self.text();
        if let Some(cell) = &self.text_cell {
            *cell.borrow_mut() = t.clone();
        }
        if let Some(mut cb) = self.on_change.take() {
            cb(&t);
            self.on_change = Some(cb);
        }
    }

    pub(crate) fn notify_enter(&mut self) {
        if let Some(mut cb) = self.on_enter.take() {
            let t = self.text();
            cb(&t);
            self.on_enter = Some(cb);
        }
    }

    pub(crate) fn notify_edit_complete(&mut self) {
        if let Some(mut cb) = self.on_edit_complete.take() {
            let t = self.text();
            cb(&t);
            self.on_edit_complete = Some(cb);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use super::*;
    use crate::event::Event;

    const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

    fn typed(field: &mut TextField, c: char) -> EventResult {
        field.on_event(&Event::KeyDown {
            key: Key::Char(c),
            modifiers: Modifiers::default(),
        })
    }

    #[test]
    fn rust_only_text_filter_sees_the_resulting_text_and_refuses_whole() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&seen);
        let mut field = TextField::new(Arc::new(Font::from_slice(FONT_BYTES).expect("font")))
            .with_text("ab")
            .with_text_filter(move |inserted, result| {
                log.borrow_mut()
                    .push((inserted.to_string(), result.to_string()));
                result.len() <= 3
            });
        field.on_event(&Event::FocusGained);
        field.set_cursor_position(1);
        assert!(typed(&mut field, 'x').is_consumed());
        assert_eq!(field.text(), "axb");
        // refused: still consumed, text and caret untouched
        assert!(typed(&mut field, 'y').is_consumed());
        assert_eq!(field.text(), "axb");
        assert_eq!(field.cursor_pos(), 2);
        assert_eq!(
            *seen.borrow(),
            vec![
                ("x".to_string(), "axb".to_string()),
                ("y".to_string(), "axyb".to_string())
            ]
        );
        // a selection is replaced in the judged text
        field.replace_range(0, 3, "zz");
        assert_eq!(field.text(), "zz");
        assert_eq!(
            seen.borrow().last().map(|(_, r)| r.clone()),
            Some("zz".to_string())
        );
    }

    #[test]
    fn rust_only_mark_text_committed_stops_a_second_edit_complete() {
        let commits = Rc::new(RefCell::new(0));
        let count = Rc::clone(&commits);
        let mut field = TextField::new(Arc::new(Font::from_slice(FONT_BYTES).expect("font")))
            .on_edit_complete(move |_| *count.borrow_mut() += 1);
        field.on_event(&Event::FocusGained);
        typed(&mut field, '1');
        field.mark_text_committed();
        field.on_event(&Event::FocusLost);
        assert_eq!(*commits.borrow(), 0);
    }
}
