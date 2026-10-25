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
        .map(|b| b.text_len() + 1)
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
        TextSelection::new(
            flat_offset(core.doc(), sel.start),
            flat_offset(core.doc(), sel.end),
        )
    }

    /// Put the selection at flat offsets (clamped). The selection it already
    /// has changes nothing: a pending caret style stays armed.
    pub(super) fn set_selection_offsets(&mut self, selection: TextSelection) {
        let mut core = self.core.borrow_mut();
        let anchor = doc_pos_at(core.doc(), selection.anchor);
        let caret = doc_pos_at(core.doc(), selection.caret);
        let now = core.selection();
        if (now.start, now.end) == (anchor, caret) {
            return;
        }
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

    #[test]
    fn every_position_round_trips_with_empty_blocks() {
        let doc = RichDoc::from_blocks(vec![
            Block::plain("héllo"),
            Block::new(),
            Block::plain("🎉x"),
            Block::new(),
        ]);
        let plain = doc.plain_text();
        assert_eq!(plain, "héllo\n\n🎉x\n");
        let mut last = None;
        for offset in 0..=plain.len() + 2 {
            let pos = doc_pos_at(&doc, offset);
            let back = flat_offset(&doc, pos);
            if offset <= plain.len() && plain.is_char_boundary(offset) {
                assert_eq!(back, offset, "{offset} -> {pos:?}");
                if let Some(prev) = last {
                    assert!(pos > prev, "positions in order: {prev:?} then {pos:?}");
                }
                last = Some(pos);
            } else if offset <= plain.len() {
                // Inside a character: back to its start.
                assert!(back < offset && plain.is_char_boundary(back), "{offset}");
            } else {
                assert_eq!(pos, DocPos::new(3, 0), "past the end: the empty last block");
            }
        }
        assert_eq!(doc_pos_at(&doc, 7), DocPos::new(1, 0), "the empty block");
        assert_eq!(flat_offset(&doc, DocPos::new(3, 0)), plain.len());
    }
}
