//! Input-method (IME) composition state shared by the text widgets.
//!
//! The platform shells deliver [`Event::ImePreedit`](crate::event::Event) while
//! a composition is in progress and [`Event::TextInput`](crate::event::Event)
//! when it is committed. A text widget keeps the in-flight composition as a
//! [`Preedit`] beside (never inside) its edit buffer, shows it inline with
//! [`Preedit::splice`], and drops it when the commit, a focus change or an
//! empty preedit arrives. `TextField`, `TextArea` and `RichTextEdit` use it.

/// A composition in progress: the preedit string and the caret inside it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preedit {
    /// The composition text. Never empty (an empty preedit is "no preedit").
    pub text: String,
    /// The input method's caret / highlighted range inside `text`, as byte
    /// offsets on char boundaries, or `None` when no caret should show.
    pub cursor: Option<(usize, usize)>,
}

impl Preedit {
    /// A preedit from an `Event::ImePreedit` payload, or `None` for the empty
    /// text that ends a composition. A range that is out of bounds, reversed
    /// or off a char boundary is clamped so later slicing cannot panic.
    pub fn from_event(text: &str, cursor: Option<(usize, usize)>) -> Option<Preedit> {
        if text.is_empty() {
            return None;
        }
        let fix = |i: usize| {
            let mut i = i.min(text.len());
            while !text.is_char_boundary(i) {
                i -= 1;
            }
            i
        };
        let cursor = cursor.map(|(a, b)| {
            let (a, b) = (fix(a), fix(b));
            (a.min(b), a.max(b))
        });
        Some(Preedit {
            text: text.to_string(),
            cursor,
        })
    }

    /// `base` with the composition shown in place of the byte range
    /// `lo..hi` (the selection, or the caret when `lo == hi`).
    pub fn splice(&self, base: &str, lo: usize, hi: usize) -> String {
        let mut out = String::with_capacity(base.len() + self.text.len());
        out.push_str(&base[..lo]);
        out.push_str(&self.text);
        out.push_str(&base[hi..]);
        out
    }

    /// Byte offset of the composition's caret inside [`splice`](Self::splice)'s
    /// result, given the splice started at `lo`; a composition with no caret
    /// puts it at the composition's end.
    pub fn caret_in_spliced(&self, lo: usize) -> usize {
        lo + self.cursor.map_or(self.text.len(), |(start, _)| start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_no_preedit() {
        assert_eq!(Preedit::from_event("", Some((0, 0))), None);
    }

    #[test]
    fn cursor_range_is_clamped_to_char_boundaries() {
        // "日本" is two 3-byte chars; byte 4 is inside the second one.
        let p = Preedit::from_event("日本", Some((4, 99))).unwrap();
        assert_eq!(p.cursor, Some((3, 6)));
        let p = Preedit::from_event("日本", Some((6, 0))).unwrap();
        assert_eq!(p.cursor, Some((0, 6)));
    }

    #[test]
    fn splice_replaces_the_range_and_places_the_caret() {
        let p = Preedit::from_event("にほん", Some((3, 6))).unwrap();
        assert_eq!(p.splice("abcd", 1, 3), "aにほんd");
        assert_eq!(p.caret_in_spliced(1), 4);
        let none = Preedit::from_event("ab", None).unwrap();
        assert_eq!(none.caret_in_spliced(2), 4);
    }
}
