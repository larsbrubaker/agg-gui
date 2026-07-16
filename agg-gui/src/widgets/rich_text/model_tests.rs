//! Unit tests for the rich-text document model ([`super`]).

use super::*;
use crate::color::Color;

fn bold() -> InlineStyle {
    InlineStyle {
        bold: true,
        ..Default::default()
    }
}

#[test]
fn text_len_and_flatten() {
    let block = Block {
        runs: vec![TextRun::plain("Hello, "), TextRun::new("world", bold())],
        ..Block::new()
    };
    assert_eq!(block.text_len(), "Hello, world".len());
    assert_eq!(block.text(), "Hello, world");
}

#[test]
fn ensure_boundary_splits_straddling_run() {
    let mut block = Block::plain("HelloWorld");
    let idx = block.ensure_boundary(5);
    assert_eq!(idx, 1);
    assert_eq!(block.runs.len(), 2);
    assert_eq!(block.runs[0].text, "Hello");
    assert_eq!(block.runs[1].text, "World");
}

#[test]
fn ensure_boundary_at_existing_edges() {
    let mut block = Block::plain("Hello");
    assert_eq!(block.ensure_boundary(0), 0);
    assert_eq!(block.ensure_boundary(5), 1); // == runs.len()
    assert_eq!(block.runs.len(), 1);
}

#[test]
fn normalize_merges_and_drops() {
    let mut block = Block {
        runs: vec![
            TextRun::plain("a"),
            TextRun::plain(""), // empty -> dropped
            TextRun::plain("b"),
            TextRun::new("c", bold()),
        ],
        ..Block::new()
    };
    block.normalize();
    assert_eq!(block.runs.len(), 2);
    assert_eq!(block.runs[0].text, "ab");
    assert_eq!(block.runs[1].text, "c");
}

#[test]
fn insert_text_seamless_when_same_style() {
    let mut doc = RichDoc::from_blocks(vec![Block::plain("Held")]);
    insert_text(&mut doc, DocPos::new(0, 2), "l", InlineStyle::default());
    assert_eq!(doc.blocks[0].text(), "Helld");
    // Same style splices into one run.
    assert_eq!(doc.blocks[0].runs.len(), 1);
}

#[test]
fn insert_text_new_run_when_style_differs() {
    let mut doc = RichDoc::from_blocks(vec![Block::plain("Hi")]);
    insert_text(&mut doc, DocPos::new(0, 1), "X", bold());
    assert_eq!(doc.blocks[0].text(), "HXi");
    assert_eq!(doc.blocks[0].runs.len(), 3);
    assert!(doc.blocks[0].runs[1].style.bold);
}

#[test]
fn remove_range_within_block_preserves_styles() {
    let mut doc = RichDoc::from_blocks(vec![Block {
        runs: vec![TextRun::plain("abc"), TextRun::new("DEF", bold())],
        ..Block::new()
    }]);
    // Remove "cD" straddling the run boundary.
    let pos = remove_range(&mut doc, DocRange::new(DocPos::new(0, 2), DocPos::new(0, 4)));
    assert_eq!(pos, DocPos::new(0, 2));
    assert_eq!(doc.blocks[0].text(), "abEF");
    assert_eq!(doc.blocks[0].runs.len(), 2);
    assert_eq!(doc.blocks[0].runs[0].text, "ab");
    assert!(doc.blocks[0].runs[1].style.bold);
    assert_eq!(doc.blocks[0].runs[1].text, "EF");
}

#[test]
fn remove_range_across_blocks_merges() {
    let mut doc = RichDoc::from_blocks(vec![
        Block::plain("Hello"),
        Block::plain("cruel"),
        Block::new_ordered("world"),
    ]);
    // From block 0 byte 2 to block 2 byte 2 -> "He" + "rld".
    let pos = remove_range(&mut doc, DocRange::new(DocPos::new(0, 2), DocPos::new(2, 2)));
    assert_eq!(pos, DocPos::new(0, 2));
    assert_eq!(doc.blocks.len(), 1);
    assert_eq!(doc.blocks[0].text(), "Herld");
    // First block's attributes win.
    assert_eq!(doc.blocks[0].list, ListKind::None);
}

#[test]
fn remove_range_clamps_end_block_past_document() {
    // A range whose high endpoint names a block past the end (e.g. a stale
    // select-all target after the doc shrank) must not panic and should behave
    // as if it reached the true last block's tail.
    let mut doc = RichDoc::from_blocks(vec![Block::plain("Hello"), Block::plain("World")]);
    let pos = remove_range(
        &mut doc,
        DocRange::new(DocPos::new(0, 2), DocPos::new(9, 999)),
    );
    assert_eq!(pos, DocPos::new(0, 2));
    // Everything from (0,2) to the clamped end of block 1 is gone.
    assert_eq!(doc.blocks.len(), 1);
    assert_eq!(doc.blocks[0].text(), "He");
}

#[test]
fn remove_range_single_block_out_of_range_end() {
    // Same clamp when the document has only one block.
    let mut doc = RichDoc::from_blocks(vec![Block::plain("Hello")]);
    let pos = remove_range(
        &mut doc,
        DocRange::new(DocPos::new(0, 1), DocPos::new(5, 100)),
    );
    assert_eq!(pos, DocPos::new(0, 1));
    assert_eq!(doc.blocks.len(), 1);
    assert_eq!(doc.blocks[0].text(), "H");
}

#[test]
fn split_block_inherits_attributes() {
    let mut doc = RichDoc::from_blocks(vec![Block {
        runs: vec![TextRun::plain("HelloWorld")],
        align: crate::widgets::text_area::TextHAlign::Center,
        list: ListKind::Bullet,
        indent: 2,
    }]);
    let pos = split_block(&mut doc, DocPos::new(0, 5));
    assert_eq!(pos, DocPos::new(1, 0));
    assert_eq!(doc.blocks.len(), 2);
    assert_eq!(doc.blocks[0].text(), "Hello");
    assert_eq!(doc.blocks[1].text(), "World");
    assert_eq!(doc.blocks[1].indent, 2);
    assert_eq!(doc.blocks[1].list, ListKind::Bullet);
    assert_eq!(
        doc.blocks[1].align,
        crate::widgets::text_area::TextHAlign::Center
    );
}

#[test]
fn merge_block_with_prev_joins() {
    let mut doc = RichDoc::from_blocks(vec![Block::plain("Hello"), Block::plain("World")]);
    let pos = merge_block_with_prev(&mut doc, 1);
    assert_eq!(pos, DocPos::new(0, 5));
    assert_eq!(doc.blocks.len(), 1);
    assert_eq!(doc.blocks[0].text(), "HelloWorld");
    // Single merged run (same style).
    assert_eq!(doc.blocks[0].runs.len(), 1);
}

#[test]
fn merge_block_with_prev_noop_at_zero() {
    let mut doc = RichDoc::from_blocks(vec![Block::plain("Hello")]);
    let before = doc.clone();
    let _ = merge_block_with_prev(&mut doc, 0);
    assert_eq!(doc, before);
}

#[test]
fn docrange_ordering() {
    let a = DocPos::new(0, 3);
    let b = DocPos::new(1, 1);
    assert!(a < b);
    let r = DocRange::new(b, a);
    assert_eq!(r.ordered(), (a, b));
    assert_eq!(r.min(), a);
    assert_eq!(r.max(), b);
    assert!(!r.is_empty());
    assert!(DocRange::collapsed(a).is_empty());
}

#[test]
fn colors_round_trip_through_style() {
    let mut s = InlineStyle::default();
    s.text_color = Some(Color::from_rgb8(10, 20, 30));
    s.highlight = Some(Color::from_rgb8(200, 200, 0));
    let run = TextRun::new("x", s.clone());
    assert_eq!(run.style, s);
}

// Small test-only helper to build an ordered-list block.
impl Block {
    fn new_ordered(text: &str) -> Self {
        Self {
            runs: vec![TextRun::plain(text)],
            list: ListKind::Ordered,
            ..Self::new()
        }
    }
}

// ── extract_range / splice_fragment (styled clipboard) ─────────────────────

#[test]
fn extract_range_keeps_run_styles_within_one_block() {
    // "plainBOLD" — select "ainBO" straddling the run boundary; the fragment
    // must keep both the plain and bold portions with their styles.
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![TextRun::plain("plain"), TextRun::new("BOLD", bold())],
        ..Block::new()
    }]);
    let frag = extract_range(&doc, DocRange::new(DocPos::new(0, 2), DocPos::new(0, 7)));
    assert_eq!(frag.len(), 1);
    assert_eq!(frag[0].text(), "ainBO");
    assert_eq!(frag[0].runs.len(), 2);
    assert_eq!(frag[0].runs[0].text, "ain");
    assert!(!frag[0].runs[0].style.bold);
    assert_eq!(frag[0].runs[1].text, "BO");
    assert!(frag[0].runs[1].style.bold);
}

#[test]
fn extract_range_keeps_block_attributes_for_whole_blocks() {
    let doc = RichDoc::from_blocks(vec![Block::plain("head"), Block::new_ordered("item")]);
    let frag = extract_range(&doc, DocRange::new(DocPos::new(0, 0), DocPos::new(1, 4)));
    assert_eq!(frag.len(), 2);
    assert_eq!(frag[1].text(), "item");
    assert_eq!(frag[1].list, ListKind::Ordered);
}

#[test]
fn splice_single_block_fragment_is_inline() {
    // Paste "XY" into the middle of "abcd" — no new paragraph, runs spliced.
    let mut doc = RichDoc::from_blocks(vec![Block::plain("abcd")]);
    let frag = vec![Block::from_run(TextRun::new("XY", bold()))];
    let end = splice_fragment(&mut doc, DocPos::new(0, 2), &frag);
    assert_eq!(doc.blocks.len(), 1);
    assert_eq!(doc.blocks[0].text(), "abXYcd");
    assert_eq!(end, DocPos::new(0, 4));
    // The bold run is preserved between the plain halves.
    let bold_run = doc.blocks[0].runs.iter().find(|r| r.text == "XY").unwrap();
    assert!(bold_run.style.bold);
}

#[test]
fn splice_single_block_into_empty_adopts_list_attribute() {
    // Copying a whole bullet/ordered item and pasting into a blank paragraph
    // should reproduce the list decoration, not a plain line.
    let mut doc = RichDoc::new();
    let frag = vec![Block::new_ordered("item")];
    splice_fragment(&mut doc, DocPos::new(0, 0), &frag);
    assert_eq!(doc.blocks.len(), 1);
    assert_eq!(doc.blocks[0].text(), "item");
    assert_eq!(doc.blocks[0].list, ListKind::Ordered);
}

#[test]
fn splice_multi_block_fragment_splits_paragraph() {
    // Paste a 2-block fragment into the middle of "abcd": head keeps "ab" plus
    // the fragment's first line, a new paragraph carries the second line plus
    // the original tail "cd".
    let mut doc = RichDoc::from_blocks(vec![Block::plain("abcd")]);
    let frag = vec![Block::plain("ONE"), Block::plain("TWO")];
    let end = splice_fragment(&mut doc, DocPos::new(0, 2), &frag);
    assert_eq!(doc.blocks.len(), 2);
    assert_eq!(doc.blocks[0].text(), "abONE");
    assert_eq!(doc.blocks[1].text(), "TWOcd");
    // Caret sits between the pasted "TWO" and the original tail "cd".
    assert_eq!(end, DocPos::new(1, 3));
}
