//! Input-filtering helpers + callback-dispatch helpers split out
//! of `text_field.rs` to keep that file under the 800-line cap.

use super::*;

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
