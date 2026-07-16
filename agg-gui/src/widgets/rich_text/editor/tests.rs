//! Unit tests for the interactive rich-text editor: the logical [`RichEditCore`]
//! (typing / pending style / toggle-over-selection / undo coalescing / clipboard
//! round-trip) and the widget's caret hit-testing across mixed font sizes.

use std::sync::Arc;

use crate::geometry::{Point, Size};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::rich_text::commands::RichCommand;
use crate::widgets::rich_text::model::{Block, DocPos, InlineStyle, RichDoc, TextRun};
use crate::widgets::rich_text::view::SharedResolver;

use super::core::RichEditCore;
use super::RichTextEdit;

const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/Arial-Regular.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("test font loads"))
}

fn resolver() -> SharedResolver {
    let f = font();
    std::rc::Rc::new(move |_: &InlineStyle| Arc::clone(&f))
}

fn bold_style() -> InlineStyle {
    InlineStyle {
        bold: true,
        ..Default::default()
    }
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

// ── Logical core tests ────────────────────────────────────────────────────

#[test]
fn typing_inherits_style_at_caret() {
    // Caret at the end of a bold run: the next char must inherit bold.
    let doc = RichDoc::from_blocks(vec![Block::from_run(TextRun::new("X", bold_style()))]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.set_caret(DocPos::new(0, 1), false);
    core.insert("y");
    let block = &core.doc().blocks[0];
    assert_eq!(block.text(), "Xy");
    // Normalised into a single bold run.
    assert_eq!(block.runs.len(), 1);
    assert!(block.runs[0].style.bold);
}

#[test]
fn pending_style_applies_to_next_insert() {
    let doc = RichDoc::from_blocks(vec![Block::plain("hi")]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.set_caret(DocPos::new(0, 2), false);
    // Toggle bold with a collapsed selection: arms a pending style, no mutation.
    core.exec(&RichCommand::ToggleBold);
    assert_eq!(core.doc().blocks[0].text(), "hi");
    assert!(core.pending_style().is_some());
    // The next typed char is bold; the plain prefix is untouched.
    core.insert("Z");
    let block = &core.doc().blocks[0];
    assert_eq!(block.text(), "hiZ");
    let last = block.runs.last().unwrap();
    assert_eq!(last.text, "Z");
    assert!(last.style.bold);
    // Pending style consumed after typing.
    assert!(core.pending_style().is_none());
}

#[test]
fn moving_caret_clears_pending_style() {
    let doc = RichDoc::from_blocks(vec![Block::plain("hi")]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.set_caret(DocPos::new(0, 2), false);
    core.exec(&RichCommand::ToggleBold);
    assert!(core.pending_style().is_some());
    core.set_caret(DocPos::new(0, 0), false);
    assert!(core.pending_style().is_none());
}

#[test]
fn toggle_bold_over_selection_through_exec() {
    let doc = RichDoc::from_blocks(vec![Block::plain("hello")]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.select_all();
    core.exec(&RichCommand::ToggleBold);
    assert_eq!(core.common_style_of_selection().bold, Some(true));
    // A second toggle clears it.
    core.exec(&RichCommand::ToggleBold);
    assert_eq!(core.common_style_of_selection().bold, Some(false));
}

#[test]
fn undo_coalesces_rapid_typing_into_one_step() {
    let doc = RichDoc::from_blocks(vec![Block::plain("Hi")]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.set_caret(core.doc().end_pos(), false);
    // Baseline snapshot.
    core.feed_undo(0.0);

    // Rapid typing within stable_time — must not each become an undo point.
    core.insert("a");
    core.feed_undo(0.1);
    core.insert("b");
    core.feed_undo(0.2);
    core.insert("c");
    core.feed_undo(0.3);
    assert_eq!(core.doc().blocks[0].text(), "Hiabc");

    // Hold steady past stable_time (default 1.0s) → exactly one undo point.
    core.feed_undo(0.4);
    core.feed_undo(1.6);
    assert!(core.can_undo());

    // A single undo reverts the whole coalesced run back to the baseline.
    assert!(core.undo());
    assert_eq!(core.doc().blocks[0].text(), "Hi");
    assert!(!core.can_undo(), "typing coalesced into one undo step");
}

#[test]
fn clipboard_round_trip_across_blocks() {
    // Multi-block selection flattens to text with `\n`; re-inserting it rebuilds
    // the paragraph split. This exercises the plain-text bridge Copy/Paste use.
    let doc = RichDoc::from_blocks(vec![Block::plain("alpha"), Block::plain("beta")]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.select_all();
    let copied = core.selected_plain_text();
    assert_eq!(copied, "alpha\nbeta");

    let mut fresh = RichEditCore::new(RichDoc::new(), 16.0);
    fresh.insert(&copied);
    assert_eq!(fresh.doc().blocks.len(), 2);
    assert_eq!(fresh.doc().blocks[0].text(), "alpha");
    assert_eq!(fresh.doc().blocks[1].text(), "beta");
}

// ── Widget geometry (caret hit-testing) ───────────────────────────────────

fn laid_out_editor(doc: RichDoc, w: f64, h: f64) -> RichTextEdit {
    let mut ed = RichTextEdit::new(doc, resolver()).with_font_size(16.0);
    ed.layout(Size::new(w, h));
    ed
}

#[test]
fn caret_hit_test_round_trips_at_mixed_sizes() {
    // One line mixing a 12px run and a 32px run. Hit-testing at a byte's own
    // caret x must return that byte, proving per-fragment metrics are honoured.
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![sized("small ", 12.0), sized("BIG", 32.0)],
        ..Block::new()
    }]);
    let ed = laid_out_editor(doc, 400.0, 120.0);

    for byte in [0usize, 3, 6, 7, 9] {
        let pos = DocPos::new(0, byte);
        let geom = ed
            .caret_geometry(pos)
            .expect("caret geometry after layout");
        // Sample just inside the caret column, on the caret's line.
        let probe = Point::new(geom.x + 0.5, geom.y_bottom + geom.height * 0.5);
        let hit = ed.hit_test_pos(probe);
        assert_eq!(hit.block, 0);
        assert_eq!(
            hit.byte, byte,
            "hit-test at caret x for byte {byte} returned {}",
            hit.byte
        );
    }
}

#[test]
fn click_before_first_char_is_byte_zero() {
    let doc = RichDoc::from_blocks(vec![Block::from_run(sized("hello", 16.0))]);
    let ed = laid_out_editor(doc, 400.0, 120.0);
    let geom = ed.caret_geometry(DocPos::new(0, 0)).unwrap();
    // Far to the left of the text still clamps to the line start.
    let hit = ed.hit_test_pos(Point::new(geom.x - 50.0, geom.y_bottom + 2.0));
    assert_eq!(hit, DocPos::new(0, 0));
}
