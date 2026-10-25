//! [`RichMarkdownEditWidget`]'s selection as flat offsets
//! ([`Widget::text_selection`](crate::widget::Widget::text_selection)): the
//! editor's text positions numbered in document order (each block, or each
//! table cell row by row), counted in characters as [`DocPosition::offset`]
//! counts them, with one position between each. The on-screen keyboard's
//! caret gestures read it and put it back (`on_screen_keyboard/cursor_slide.rs`).
//! A whole-block selection (an object, not text) has no flat form.

use super::block::RichBlockKind;
use super::document::{DocPosition, RichDocument};
use super::edit_widget::RichMarkdownEditWidget;
use super::selection::RichSelection;
use crate::widget::TextSelection;

/// Every text position holder in document order: `(block, row, column,
/// length)`, a table block giving one per cell.
fn segments(doc: &RichDocument) -> Vec<(usize, usize, usize, usize)> {
    let mut out = Vec::new();
    for (b, block) in doc.blocks.iter().enumerate() {
        if block.kind == RichBlockKind::Table {
            for (r, row) in block.table_rows.iter().enumerate() {
                for c in 0..row.len() {
                    out.push((b, r, c, block.text_length(r, c)));
                }
            }
        } else {
            out.push((b, 0, 0, block.text_length(0, 0)));
        }
    }
    out
}

/// The flat offset of `pos`.
fn flat_offset(doc: &RichDocument, pos: DocPosition) -> usize {
    let mut before = 0;
    for (b, r, c, len) in segments(doc) {
        if (b, r, c) == (pos.block_index, pos.row, pos.column) {
            return before + pos.offset.min(len);
        }
        before += len + 1;
    }
    before
}

/// The position at flat `offset`, clamped to the document.
fn position_at(doc: &RichDocument, offset: usize) -> DocPosition {
    let segments = segments(doc);
    let mut left = offset;
    for (i, &(b, r, c, len)) in segments.iter().enumerate() {
        if left <= len || i + 1 == segments.len() {
            return DocPosition::in_cell(b, left.min(len), r, c);
        }
        left -= len + 1;
    }
    DocPosition::default()
}

impl RichMarkdownEditWidget {
    /// The selection as flat offsets; `None` for a whole-block selection.
    pub(super) fn selection_offsets(&self) -> Option<TextSelection> {
        let sel = self.selection();
        if sel.whole_block {
            return None;
        }
        let doc = self.document();
        Some(TextSelection {
            anchor: flat_offset(doc, sel.anchor),
            caret: flat_offset(doc, sel.caret),
        })
    }

    /// Put the selection at flat offsets (clamped).
    pub(super) fn set_selection_offsets(&mut self, selection: TextSelection) {
        let doc = self.document();
        let sel = RichSelection::new(
            position_at(doc, selection.anchor),
            position_at(doc, selection.caret),
        );
        self.select(sel);
        crate::animation::request_draw();
    }
}
