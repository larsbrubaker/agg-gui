//! Tests for the rich (styled) clipboard path of [`RichTextEdit`]: Ctrl+C /
//! Ctrl+X / Ctrl+V carrying inline style and block decoration across editor
//! instances.
//!
//! Copy stashes a styled fragment in the in-process
//! [`rich_clipboard`](crate::widgets::rich_text::rich_clipboard) slot alongside a
//! plain-text fingerprint written to the system clipboard. Paste reuses the
//! styled fragment only when the system text still matches that fingerprint
//! (normalised for CRLF mangling by the OS); otherwise it falls back to
//! inserting external plain text in the caret's inherited style. These tests
//! drive real key events through a laid-out widget, so they cover the event
//! handler, the fingerprint match, and the single-undo-step guarantee.
//!
//! Split out of `editor/tests.rs` (which had reached the project's 800-line
//! hard cap) because the clipboard round trip is a self-contained feature with
//! its own helpers, unused by the remaining core/geometry/paint tests.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, Key, Modifiers};
use crate::geometry::Size;
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::rich_text::model::{Block, DocPos, InlineStyle, ListKind, RichDoc, TextRun};
use crate::widgets::rich_text::view::SharedResolver;
use crate::widgets::rich_text::RichTextEdit;

const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/Arial-Regular.ttf");

fn resolver() -> SharedResolver {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("test font loads"));
    std::rc::Rc::new(move |_: &InlineStyle| Arc::clone(&font))
}

fn laid_out_editor(doc: RichDoc, w: f64, h: f64) -> RichTextEdit {
    let mut ed = RichTextEdit::new(doc, resolver()).with_font_size(16.0);
    ed.layout(Size::new(w, h));
    ed
}

fn ctrl(c: char) -> Event {
    Event::KeyDown {
        key: Key::Char(c),
        modifiers: Modifiers {
            ctrl: true,
            ..Default::default()
        },
    }
}

fn red_24_bold() -> InlineStyle {
    InlineStyle {
        bold: true,
        font_size: Some(24.0),
        text_color: Some(Color::from_rgb8(255, 0, 0)),
        ..Default::default()
    }
}

/// Copy a styled, bulleted line and paste it into a fresh editor: the bold /
/// 24pt / red run and the list decoration must all survive the round trip
/// through the in-process rich clipboard slot.
#[test]
fn styled_run_and_list_block_survive_copy_paste() {
    crate::widgets::rich_text::rich_clipboard::clear();
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![TextRun::new("Hi", red_24_bold())],
        list: ListKind::Bullet,
        ..Block::new()
    }]);
    let mut src = laid_out_editor(doc, 400.0, 200.0);
    src.on_event(&Event::FocusGained);
    src.core.borrow_mut().select_all();
    src.on_event(&ctrl('c'));

    // Cross-instance paste into a blank editor.
    let mut dst = laid_out_editor(RichDoc::new(), 400.0, 200.0);
    dst.on_event(&Event::FocusGained);
    dst.on_event(&ctrl('v'));

    let doc = dst.core.borrow().doc().clone();
    assert_eq!(doc.blocks.len(), 1);
    assert_eq!(
        doc.blocks[0].list,
        ListKind::Bullet,
        "list decoration survives"
    );
    let run = &doc.blocks[0].runs[0];
    assert_eq!(run.text, "Hi");
    assert!(run.style.bold, "bold survives");
    assert_eq!(run.style.font_size, Some(24.0), "point size survives");
    assert_eq!(
        run.style.text_color,
        Some(Color::from_rgb8(255, 0, 0)),
        "colour survives"
    );
}

/// A native clipboard (arboard on Windows) may hand back `\r\n` for text we
/// copied with `\n`. The fingerprint match normalizes line endings, so a styled
/// multi-line copy still pastes styled after that round trip.
#[test]
fn crlf_mangled_clipboard_still_matches_fingerprint() {
    crate::widgets::rich_text::rich_clipboard::clear();
    let doc = RichDoc::from_blocks(vec![
        Block::from_run(TextRun::new("one", red_24_bold())),
        Block::from_run(TextRun::new("two", red_24_bold())),
    ]);
    let mut src = laid_out_editor(doc, 400.0, 200.0);
    src.on_event(&Event::FocusGained);
    src.core.borrow_mut().select_all();
    src.on_event(&ctrl('c'));

    // Simulate the OS clipboard normalizing "one\ntwo" to CRLF line endings.
    crate::clipboard::set_text("one\r\ntwo");

    let mut dst = laid_out_editor(RichDoc::new(), 400.0, 200.0);
    dst.on_event(&Event::FocusGained);
    dst.on_event(&ctrl('v'));

    let doc = dst.core.borrow().doc().clone();
    assert_eq!(doc.blocks.len(), 2);
    assert!(doc.blocks[0].runs[0].style.bold, "styled fragment reused");
    assert!(doc.blocks[1].runs[0].style.bold);
}

/// When the system clipboard text no longer matches our stored fingerprint
/// (something was copied elsewhere in between), paste falls back to inserting
/// the external plain text in the caret's inherited style.
#[test]
fn fingerprint_mismatch_falls_back_to_plain_text() {
    crate::widgets::rich_text::rich_clipboard::clear();
    let doc = RichDoc::from_blocks(vec![Block::from_run(TextRun::new("Hi", red_24_bold()))]);
    let mut src = laid_out_editor(doc, 400.0, 200.0);
    src.on_event(&Event::FocusGained);
    src.core.borrow_mut().select_all();
    src.on_event(&ctrl('c'));

    // Simulate an external app overwriting the system clipboard: the rich slot
    // still holds the styled fragment, but its fingerprint no longer matches.
    crate::clipboard::set_text("external");

    let mut dst = laid_out_editor(RichDoc::new(), 400.0, 200.0);
    dst.on_event(&Event::FocusGained);
    dst.on_event(&ctrl('v'));

    let doc = dst.core.borrow().doc().clone();
    assert_eq!(doc.blocks[0].text(), "external");
    // Inserted as plain text — no bold carried over from the stale fragment.
    assert!(!doc.blocks[0].runs[0].style.bold);
}

/// External plain text (never copied from a RichTextEdit) pastes as plain text.
#[test]
fn external_plain_text_pastes_unstyled() {
    crate::widgets::rich_text::rich_clipboard::clear();
    crate::clipboard::set_text("hello world");
    let mut dst = laid_out_editor(RichDoc::new(), 400.0, 200.0);
    dst.on_event(&Event::FocusGained);
    dst.on_event(&ctrl('v'));
    let doc = dst.core.borrow().doc().clone();
    assert_eq!(doc.blocks[0].text(), "hello world");
    assert!(!doc.blocks[0].runs[0].style.bold);
}

/// Cut removes the styled selection and still makes it available for a styled
/// paste elsewhere.
#[test]
fn cut_removes_styled_content_and_keeps_it_for_paste() {
    crate::widgets::rich_text::rich_clipboard::clear();
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![
            TextRun::new("keep ", InlineStyle::default()),
            TextRun::new("cutme", red_24_bold()),
        ],
        ..Block::new()
    }]);
    let mut src = laid_out_editor(doc, 400.0, 200.0);
    src.on_event(&Event::FocusGained);
    // Select just "cutme".
    src.core
        .borrow_mut()
        .set_selection(DocPos::new(0, 5), DocPos::new(0, 10));
    src.on_event(&ctrl('x'));
    assert_eq!(src.core.borrow().doc().blocks[0].text(), "keep ");

    let mut dst = laid_out_editor(RichDoc::new(), 400.0, 200.0);
    dst.on_event(&Event::FocusGained);
    dst.on_event(&ctrl('v'));
    let doc = dst.core.borrow().doc().clone();
    assert_eq!(doc.blocks[0].text(), "cutme");
    assert!(doc.blocks[0].runs[0].style.bold);
}

/// A styled paste is a single undo step, matching plain paste.
#[test]
fn styled_paste_is_one_undo_step() {
    crate::widgets::rich_text::rich_clipboard::clear();
    let doc = RichDoc::from_blocks(vec![Block::from_run(TextRun::new("AB", red_24_bold()))]);
    let mut src = laid_out_editor(doc, 400.0, 200.0);
    src.on_event(&Event::FocusGained);
    src.core.borrow_mut().select_all();
    src.on_event(&ctrl('c'));

    let mut dst = laid_out_editor(RichDoc::new(), 400.0, 200.0);
    dst.on_event(&Event::FocusGained);
    // Baseline undo snapshot for the empty doc, then paste and let it settle.
    dst.core.borrow_mut().feed_undo(0.0);
    dst.on_event(&ctrl('v'));
    dst.core.borrow_mut().feed_undo(0.1);
    dst.core.borrow_mut().feed_undo(1.5);
    assert_eq!(dst.core.borrow().doc().blocks[0].text(), "AB");

    assert!(dst.core.borrow_mut().undo(), "one undo reverts the paste");
    assert_eq!(dst.core.borrow().doc().blocks[0].text(), "");
    assert!(
        !dst.core.borrow().can_undo(),
        "paste was a single undo step"
    );
}
