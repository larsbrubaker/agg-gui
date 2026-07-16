//! Unit tests for the rich-text layout engine ([`super`]), including the
//! phase-1 "proof" test that lays out a document mirroring the owner's
//! reference image and asserts the visible invariants.

use std::sync::Arc;

use crate::text::Font;
use crate::widgets::rich_text::model::{Block, InlineStyle, ListKind, RichDoc, TextRun};

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/Arial-Regular.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("test font loads"))
}

/// A resolver that returns the same face for every style (size varies per run
/// via `InlineStyle::font_size`, which the engine honours independently).
fn resolver(f: Arc<Font>) -> impl Fn(&InlineStyle) -> Arc<Font> {
    move |_: &InlineStyle| Arc::clone(&f)
}

fn sized(text: &str, size: f64) -> TextRun {
    TextRun::new(
        text,
        InlineStyle {
            font_size: Some(size),
            ..Default::default()
        },
    )
}

#[test]
fn mixed_sizes_grow_line_height() {
    let f = font();
    let r = resolver(Arc::clone(&f));
    // One block, one line: a small run followed by a big run.
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![sized("small ", 12.0), sized("BIG", 32.0)],
        ..Block::new()
    }]);
    let layout = super::layout_doc(&doc, 1000.0, 16.0, &r);
    assert_eq!(layout.blocks.len(), 1);
    let block = &layout.blocks[0];
    assert_eq!(block.lines.len(), 1, "should fit on one wide line");
    let line = &block.lines[0];
    // The 32px run dominates the line metrics.
    let big_only = f.ascender_px(32.0) + f.descender_px(32.0);
    assert!(
        line.height >= big_only * super::LINE_SPACING - 0.5,
        "line height {} should reflect the 32px run",
        line.height
    );
}

#[test]
fn heading_line_taller_than_body() {
    let f = font();
    let r = resolver(Arc::clone(&f));
    let doc = RichDoc::from_blocks(vec![
        Block::from_run(sized("Heading", 28.0)),
        Block::from_run(sized("body text", 14.0)),
    ]);
    let layout = super::layout_doc(&doc, 1000.0, 16.0, &r);
    let heading_h = layout.blocks[0].lines[0].height;
    let body_h = layout.blocks[1].lines[0].height;
    assert!(
        heading_h > body_h,
        "heading line {heading_h} should be taller than body line {body_h}"
    );
}

#[test]
fn wrap_occurs_at_narrow_width() {
    let f = font();
    let r = resolver(Arc::clone(&f));
    let doc = RichDoc::from_blocks(vec![Block::from_run(sized(
        "the quick brown fox jumps over the lazy dog",
        16.0,
    ))]);
    let wide = super::layout_doc(&doc, 1000.0, 16.0, &r);
    assert_eq!(wide.blocks[0].lines.len(), 1, "fits on one wide line");
    let narrow = super::layout_doc(&doc, 80.0, 16.0, &r);
    assert!(
        narrow.blocks[0].lines.len() > 1,
        "should wrap into multiple lines at 80px"
    );
}

#[test]
fn wrap_keeps_words_across_style_boundaries() {
    let f = font();
    let r = resolver(Arc::clone(&f));
    // "Rich" + "Text" with no whitespace between the two runs: one word.
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![
            sized("Rich", 16.0),
            TextRun::new(
                "Text",
                InlineStyle {
                    bold: true,
                    font_size: Some(16.0),
                    ..Default::default()
                },
            ),
        ],
        ..Block::new()
    }]);
    // Even at a width narrower than the whole word, the two style pieces must
    // stay on the same line (a word is never broken).
    let layout = super::layout_doc(&doc, 20.0, 16.0, &r);
    assert_eq!(layout.blocks[0].lines.len(), 1);
    assert_eq!(layout.blocks[0].lines[0].fragments.len(), 2);
}

#[test]
fn ordered_numbering_and_restart() {
    let f = font();
    let r = resolver(Arc::clone(&f));
    let doc = RichDoc::from_blocks(vec![
        ordered("first"),
        ordered("second"),
        Block::from_run(sized("a break", 16.0)),
        ordered("restarted"),
    ]);
    let layout = super::layout_doc(&doc, 1000.0, 16.0, &r);
    assert_eq!(layout.blocks[0].marker.as_deref(), Some("1."));
    assert_eq!(layout.blocks[1].marker.as_deref(), Some("2."));
    assert_eq!(layout.blocks[2].marker, None);
    assert_eq!(
        layout.blocks[3].marker.as_deref(),
        Some("1."),
        "ordered list restarts after a break"
    );
}

#[test]
fn bullet_marker_present_and_hung_in_gutter() {
    let f = font();
    let r = resolver(Arc::clone(&f));
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![sized("item", 16.0)],
        list: ListKind::Bullet,
        ..Block::new()
    }]);
    let layout = super::layout_doc(&doc, 1000.0, 16.0, &r);
    let block = &layout.blocks[0];
    assert_eq!(block.marker.as_deref(), Some("\u{2022}"));
    // Text column is pushed right to make room for the marker gutter.
    assert!(block.text_left >= super::LIST_GUTTER_PX);
    // Marker hangs left of the text column.
    assert!(block.marker_x < block.text_left);
}

#[test]
fn indent_shifts_text_left() {
    let f = font();
    let r = resolver(Arc::clone(&f));
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![sized("x", 16.0)],
        indent: 2,
        ..Block::new()
    }]);
    let layout = super::layout_doc(&doc, 1000.0, 16.0, &r);
    assert!((layout.blocks[0].text_left - 2.0 * super::INDENT_PX).abs() < 0.001);
}

#[test]
fn empty_block_reserves_a_line() {
    let f = font();
    let r = resolver(Arc::clone(&f));
    let doc = RichDoc::from_blocks(vec![Block::new()]);
    let layout = super::layout_doc(&doc, 1000.0, 16.0, &r);
    assert_eq!(layout.blocks[0].lines.len(), 1);
    assert!(layout.blocks[0].height > 0.0);
}

#[test]
fn fragment_byte_offsets_track_flattened_block() {
    let f = font();
    let r = resolver(Arc::clone(&f));
    // Two differently-styled runs, no whitespace between => one word, two
    // fragments on one line (distinct styles prevent fragment merging).
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![
            sized("abc", 16.0),
            TextRun::new(
                "DEF",
                InlineStyle {
                    bold: true,
                    font_size: Some(16.0),
                    ..Default::default()
                },
            ),
        ],
        ..Block::new()
    }]);
    let layout = super::layout_doc(&doc, 1000.0, 16.0, &r);
    let line = &layout.blocks[0].lines[0];
    assert_eq!(line.start_byte, 0);
    assert_eq!(line.end_byte, 6);
    assert_eq!(line.fragments[0].start_byte, 0);
    // Second fragment begins right after "abc".
    assert_eq!(line.fragments[1].start_byte, 3);
}

#[test]
fn wrapped_line_byte_ranges_skip_dropped_space() {
    let f = font();
    let r = resolver(Arc::clone(&f));
    let doc = RichDoc::from_blocks(vec![Block::from_run(sized("alpha beta", 16.0))]);
    // Narrow enough to wrap between the two words.
    let layout = super::layout_doc(&doc, 60.0, 16.0, &r);
    let lines = &layout.blocks[0].lines;
    assert!(lines.len() >= 2, "expected a wrap");
    assert_eq!(lines[0].start_byte, 0);
    assert_eq!(lines[0].end_byte, "alpha".len());
    // Second line starts after the dropped space (byte 6, past "alpha ").
    assert_eq!(lines[1].start_byte, "alpha ".len());
}

// ---------------------------------------------------------------------------
// Phase-1 proof: lay out a document mirroring the owner's reference image and
// assert the visible invariants.  Phase 2 registers an actual demo window; this
// only exercises layout.
// ---------------------------------------------------------------------------

fn ordered(text: &str) -> Block {
    Block {
        runs: vec![sized(text, 16.0)],
        list: ListKind::Ordered,
        ..Block::new()
    }
}

fn heading(text: &str) -> Block {
    Block::from_run(TextRun::new(
        text,
        InlineStyle {
            bold: true,
            font_size: Some(24.0),
            ..Default::default()
        },
    ))
}

#[test]
fn proof_reference_document_layout() {
    let f = font();
    let r = resolver(Arc::clone(&f));
    let doc = RichDoc::from_blocks(vec![
        heading("Toolbar"),
        ordered("Toggle bold and italic"),
        ordered("Pick a text color"),
        heading("Links"),
        Block {
            runs: vec![sized("Visit the project page", 16.0)],
            list: ListKind::Bullet,
            ..Block::new()
        },
    ]);
    // Narrow enough that the longer items wrap.
    let layout = super::layout_doc(&doc, 160.0, 16.0, &r);
    assert_eq!(layout.blocks.len(), 5);

    // Headings are bold+large => taller lines than the body list items.
    let heading_h = layout.blocks[0].lines[0].height;
    let body_h = layout.blocks[1].lines[0].height;
    assert!(
        heading_h > body_h,
        "heading line ({heading_h}) must be taller than list line ({body_h})"
    );

    // Ordered items numbered 1. and 2.
    assert_eq!(layout.blocks[1].marker.as_deref(), Some("1."));
    assert_eq!(layout.blocks[2].marker.as_deref(), Some("2."));

    // Second heading has no marker; the ordered sequence stopped.
    assert_eq!(layout.blocks[3].marker, None);

    // Bullet item present.
    assert_eq!(layout.blocks[4].marker.as_deref(), Some("\u{2022}"));

    // At least one list item wrapped at this narrow width.
    let wrapped = layout.blocks.iter().any(|b| b.lines.len() > 1);
    assert!(wrapped, "expected some block to wrap at 160px");

    // Total document height is the sum of all block heights (non-zero).
    let sum: f64 = layout.blocks.iter().map(|b| b.height).sum();
    assert!((layout.height - sum).abs() < 0.001);
    assert!(layout.height > 0.0);
}
