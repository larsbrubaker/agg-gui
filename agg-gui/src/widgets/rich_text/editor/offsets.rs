//! [`RichTextEdit`]'s selection as flat offsets
//! ([`Widget::text_selection`](crate::widget::Widget::text_selection)): byte
//! offsets into its plain text, the blocks joined with one `\n` each (what
//! `text_input_value` reports), mapped to and from the document's
//! `(block, byte)` positions. The on-screen keyboard's caret gestures read it
//! and put it back (`on_screen_keyboard/cursor_slide.rs`).

use super::super::model::{DocPos, RichDoc};
use super::RichTextEdit;
use crate::widget::TextSelection;

/// The flat offset of `pos` in `doc`'s plain text.
fn flat_offset(doc: &RichDoc, pos: DocPos) -> usize {
    let before: usize = doc
        .blocks
        .iter()
        .take(pos.block)
        .map(|b| b.text().len() + 1)
        .sum();
    before + pos.byte
}

/// The document position at flat `offset`, clamped to the document and to a
/// character boundary in its block.
fn doc_pos_at(doc: &RichDoc, offset: usize) -> DocPos {
    let mut left = offset;
    let last = doc.blocks.len().saturating_sub(1);
    for (i, block) in doc.blocks.iter().enumerate() {
        let text = block.text();
        if left <= text.len() || i == last {
            let byte = crate::widgets::text_field_core::floor_char_boundary(&text, left);
            return DocPos::new(i, byte);
        }
        left -= text.len() + 1;
    }
    DocPos::new(0, 0)
}

impl RichTextEdit {
    /// The selection as flat offsets.
    pub(super) fn selection_offsets(&self) -> TextSelection {
        let core = self.core.borrow();
        let sel = core.selection();
        TextSelection {
            anchor: flat_offset(core.doc(), sel.start),
            caret: flat_offset(core.doc(), sel.end),
        }
    }

    /// Put the selection at flat offsets (clamped).
    pub(super) fn set_selection_offsets(&mut self, selection: TextSelection) {
        let mut core = self.core.borrow_mut();
        let anchor = doc_pos_at(core.doc(), selection.anchor);
        let caret = doc_pos_at(core.doc(), selection.caret);
        core.set_selection(anchor, caret);
        drop(core);
        crate::animation::request_draw();
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::model::Block;
    use super::*;

    #[test]
    fn flat_offsets_join_the_blocks_with_one_newline() {
        let doc = RichDoc::from_blocks(vec![Block::plain("héllo"), Block::plain("ab")]);
        assert_eq!(flat_offset(&doc, DocPos::new(1, 1)), 8);
        assert_eq!(doc_pos_at(&doc, 8), DocPos::new(1, 1));
        assert_eq!(doc_pos_at(&doc, 6), DocPos::new(0, 6), "end of the first");
        assert_eq!(
            doc_pos_at(&doc, 2),
            DocPos::new(0, 1),
            "inside é: back to it"
        );
        assert_eq!(
            doc_pos_at(&doc, 99),
            DocPos::new(1, 2),
            "past the end: its end"
        );
    }
}
