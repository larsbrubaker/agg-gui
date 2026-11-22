//! The whole-edit judge for [`TextArea`] ([`TextArea::with_text_filter`]),
//! the multi-line counterpart of `TextField::with_text_filter`
//! (`widgets/text_field/filter.rs`): same signature, called with the inserted
//! text and the text the area would hold afterwards.
//!
//! Every insertion reaches the buffer through `insert_str` (typing, Enter,
//! Tab, paste, the context menu's Paste, an IME commit) or `indent_selection`
//! (Tab over several lines), and both ask the judge first, before the undo
//! step opens, so a refused edit leaves the text, selection and undo history
//! exactly as they were. Deletions, undo/redo, programmatic text and an IME
//! composition still being typed (it leaves the buffer before its commit is
//! judged, `ime.rs`) are not judged.
//!
//! An insertion of more than one character that the judge refuses is cut to
//! its longest leading part the judge accepts, at a character boundary, so a
//! paste into a length-limited area fills it up to the limit instead of being
//! lost. The search halves the candidate length each try, so it finds the
//! longest part for a judge that accepts every shorter part of what it
//! accepts (a length limit does); any other judge still only ever sees and
//! gets an accepted part. Split out of `text_area.rs` (800-line cap).

use super::*;

/// The whole-edit judge: `(inserted, resulting_text) -> accept`.
pub(super) type TextFilter = dyn Fn(&str, &str) -> bool;

impl TextArea {
    /// Install a whole-edit judge. Before any insertion lands — a typed
    /// character, Enter, Tab, a paste, an IME commit — the filter is called
    /// with the inserted text (line ends already `'\n'`) and the text the
    /// area would hold afterwards (the selection replaced, or the insertion
    /// at the caret). Returning `false` refuses the edit: the text, caret and
    /// undo history stay exactly as they were, and a typed key is still
    /// consumed. A refused insertion of several characters is first cut to
    /// its longest leading part the filter accepts (never splitting a
    /// character), which is inserted as one undo step instead. Deletions,
    /// undo/redo and programmatic [`set_text`](Self::set_text) are not
    /// judged.
    pub fn with_text_filter(mut self, filter: impl Fn(&str, &str) -> bool + 'static) -> Self {
        self.text_filter = Some(Rc::new(filter));
        self
    }

    /// What of `s` may be inserted over the selection: all of it, its
    /// longest accepted leading part, or `None` when the filter refuses even
    /// its first character. No filter installed, or an empty `s` (a plain
    /// deletion of the selection) → all of it.
    pub(super) fn text_filter_cut<'a>(&self, s: &'a str) -> Option<&'a str> {
        let Some(filter) = &self.text_filter else {
            return Some(s);
        };
        if s.is_empty() {
            return Some(s);
        }
        let st = self.edit.borrow();
        let lo = st.cursor.min(st.anchor).min(st.text.len());
        let hi = st.cursor.max(st.anchor).min(st.text.len());
        let (head, tail) = (&st.text[..lo], &st.text[hi..]);
        let accepts = |part: &str| {
            let mut result = String::with_capacity(head.len() + part.len() + tail.len());
            result.push_str(head);
            result.push_str(part);
            result.push_str(tail);
            filter(part, &result)
        };
        if accepts(s) {
            return Some(s);
        }
        // Byte ends of every leading part of one or more characters, short
        // to long; the last (all of `s`) was refused above.
        let ends: Vec<usize> = s
            .char_indices()
            .skip(1)
            .map(|(i, _)| i)
            .chain(std::iter::once(s.len()))
            .collect();
        let (mut best, mut lo_i, mut hi_i) = (None, 0usize, ends.len() - 1);
        while lo_i < hi_i {
            let mid = lo_i + (hi_i - lo_i) / 2;
            if accepts(&s[..ends[mid]]) {
                best = Some(ends[mid]);
                lo_i = mid + 1;
            } else {
                hi_i = mid;
            }
        }
        best.map(|end| &s[..end])
    }

    /// Whether the filter accepts an indent edit inserting `inserted` in all
    /// that leaves `result`. Refused whole: an indent cut short would indent
    /// only some of the selected lines.
    pub(super) fn text_filter_accepts(&self, inserted: &str, result: &str) -> bool {
        self.text_filter
            .as_ref()
            .is_none_or(|filter| filter(inserted, result))
    }
}
