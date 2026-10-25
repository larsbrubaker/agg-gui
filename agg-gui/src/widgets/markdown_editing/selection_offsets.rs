//! [`RichMarkdownEditWidget`]'s selection as flat offsets
//! ([`Widget::text_selection`](crate::widget::Widget::text_selection)): the
//! editor's text positions numbered in document order (each block, or each
//! table cell row by row), counted in characters as [`DocPosition::offset`]
//! counts them, with one position between each. The on-screen keyboard's
//! caret gestures read it and put it back (`on_screen_keyboard/cursor_slide.rs`).
//! A whole-block selection (an object, not text) has no flat form. The
//! caret's side of a soft wrap travels with it
//! ([`TextSelection::caret_at_line_end`]).

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
            caret_at_line_end: self.state.caret_at_line_end(),
        })
    }

    /// Put the selection at flat offsets (clamped), with the caret on the
    /// wrap side it names. Returns whether anything changed: the selection
    /// it already has changes nothing (a pending style stays armed, the
    /// caret keeps its side of a soft wrap).
    pub(super) fn set_selection_offsets(&mut self, selection: TextSelection) -> bool {
        let doc = self.document();
        let sel = RichSelection::new(
            position_at(doc, selection.anchor),
            position_at(doc, selection.caret),
        );
        let side = selection.caret_at_line_end;
        if sel == self.selection() && side == self.state.caret_at_line_end() {
            return false;
        }
        self.state.set_selection(sel, side);
        crate::animation::request_draw();
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::geometry::Size;
    use crate::text::Font;
    use crate::widget::Widget;
    use crate::widgets::markdown_editing::layout_style::RichLayoutFonts;

    const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

    /// An editor over a paragraph, a table with an empty and a multi-byte
    /// cell, a thematic break (a Raw block), a code block, a list item and a
    /// last paragraph ending in an emoji.
    fn editor() -> RichMarkdownEditWidget {
        let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
        let fonts = RichLayoutFonts {
            regular: font,
            bold: None,
            mono: None,
        };
        let mut w = RichMarkdownEditWidget::new(fonts, None);
        let md = "héllo wörld\n\n| a | bé |\n|---|---|\n| | x |\n\n---\n\n```\ncodé\n```\n\n- item\n\nend 🎉";
        w.set_markdown(md).expect("markdown loads");
        w.layout(Size::new(400.0, 800.0));
        w
    }

    /// Every caret position in document order.
    fn every_position(doc: &RichDocument) -> Vec<DocPosition> {
        let mut out = Vec::new();
        for (b, r, c, len) in segments(doc) {
            for o in 0..=len {
                out.push(DocPosition::in_cell(b, o, r, c));
            }
        }
        out
    }

    #[test]
    fn every_position_has_its_own_flat_offset_and_comes_back_from_it() {
        let mut w = editor();
        let kinds: Vec<RichBlockKind> = w.document().blocks.iter().map(|b| b.kind).collect();
        assert!(kinds.contains(&RichBlockKind::Table), "{kinds:?}");
        assert!(kinds.contains(&RichBlockKind::Raw), "{kinds:?}");
        assert!(kinds.contains(&RichBlockKind::CodeBlock), "{kinds:?}");
        assert!(kinds.contains(&RichBlockKind::ListItem), "{kinds:?}");
        let positions = every_position(w.document());
        let mut flats = Vec::new();
        for &p in &positions {
            w.select(RichSelection::at(p));
            let s = w.text_selection().expect("a text selection");
            assert_eq!(s.anchor, s.caret);
            flats.push(s.caret);
            assert!(w.set_text_selection(s));
            assert_eq!(w.selection(), RichSelection::at(p), "{p:?} -> {s:?}");
            assert_eq!(w.text_selection(), Some(s), "{p:?}: the same again");
        }
        assert!(
            flats.windows(2).all(|f| f[0] + 1 == f[1]),
            "one flat offset per position, in order: {flats:?}"
        );
        assert_eq!(flats.first(), Some(&0));
    }

    #[test]
    fn a_reversed_range_across_the_table_comes_back_reversed() {
        let mut w = editor();
        let positions = every_position(w.document());
        let (early, late) = (positions[3], positions[positions.len() - 2]);
        let reversed = RichSelection::new(late, early);
        w.select(reversed);
        let s = w.text_selection().expect("a text selection");
        assert!(s.anchor > s.caret);
        w.select(RichSelection::at(early));
        assert!(w.set_text_selection(s));
        assert_eq!(w.selection(), reversed);
    }

    #[test]
    fn an_offset_past_the_end_clamps_to_the_last_position() {
        let mut w = editor();
        let last = *every_position(w.document()).last().expect("positions");
        assert!(w.set_text_selection(TextSelection::new(9999, 9999)));
        assert_eq!(w.selection(), RichSelection::at(last));
    }

    #[test]
    fn a_whole_block_selection_has_no_flat_form() {
        let mut w = editor();
        let mut whole = RichSelection::at(DocPosition::new(0, 0));
        whole.whole_block = true;
        w.select(whole);
        assert_eq!(w.text_selection(), None);
    }
}
