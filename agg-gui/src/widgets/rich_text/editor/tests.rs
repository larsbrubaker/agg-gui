//! Unit tests for the interactive rich-text editor: the logical [`RichEditCore`]
//! (typing / pending style / toggle-over-selection / undo coalescing / clipboard
//! round-trip) and the widget's caret hit-testing across mixed font sizes.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, Key, Modifiers, MouseButton};
use crate::geometry::{Point, Size};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::rich_text::commands::RichCommand;
use crate::widgets::rich_text::model::{Block, DocPos, InlineStyle, ListKind, RichDoc, TextRun};
use crate::widgets::rich_text::view::SharedResolver;
use crate::widgets::text_area::TextHAlign;

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
fn pending_style_keeps_block_align_and_list_active() {
    // Caret in a left-aligned bullet block; arm bold with a collapsed caret.
    // The inline pending style must not blank out the block-level toolbar
    // state: alignment and list stay reported from the caret's block.
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![TextRun::plain("item")],
        align: TextHAlign::Left,
        list: ListKind::Bullet,
        indent: 0,
    }]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.set_caret(DocPos::new(0, 4), false);
    core.exec(&RichCommand::ToggleBold);
    assert!(core.pending_style().is_some(), "bold armed at the caret");

    let cs = core.common_style_of_selection();
    assert_eq!(cs.bold, Some(true), "pending bold is reported");
    assert_eq!(
        cs.align,
        Some(TextHAlign::Left),
        "block alignment must survive an armed pending style"
    );
    assert_eq!(
        cs.list,
        Some(ListKind::Bullet),
        "block list kind must survive an armed pending style"
    );
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

// ── Live colour preview: undo hygiene ─────────────────────────────────────
//
// A colour dialog previews by exec-ing `SetTextColor` every drag frame. The
// preview session (`begin_preview` → `commit_preview` / `cancel_preview`)
// suspends undo feeding so the drag collapses into ONE undo step on commit and
// leaves NO stray entry on cancel.

fn colored(text: &str, color: crate::color::Color) -> TextRun {
    TextRun::new(
        text,
        InlineStyle {
            text_color: Some(color),
            ..Default::default()
        },
    )
}

#[test]
fn preview_commit_collapses_drag_into_one_undo_step() {
    let red = crate::color::Color::rgb(1.0, 0.0, 0.0);
    let blue = crate::color::Color::rgb(0.0, 0.0, 1.0);
    let green = crate::color::Color::rgb(0.0, 1.0, 0.0);
    let doc = RichDoc::from_blocks(vec![Block::from_run(colored("hello", red))]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.select_all();
    core.feed_undo(0.0); // baseline undo point (red)
    assert!(!core.can_undo());

    // Dialog opens: capture the committed state + suspend feeding.
    core.begin_preview();
    assert!(core.is_previewing());

    // Drag: rapid live previews. Feeding is suspended, so none snapshot.
    core.exec(&RichCommand::SetTextColor(blue));
    core.feed_undo(0.1);
    core.exec(&RichCommand::SetTextColor(green));
    core.feed_undo(0.2);

    // Select: commit + resume, then settle into exactly one undo point.
    core.commit_preview();
    assert!(!core.is_previewing());
    core.feed_undo(3.0);
    core.feed_undo(4.5);
    assert!(core.can_undo(), "the committed colour change is undoable");

    // A single undo reverts the whole drag back to the original colour, and
    // there is nothing left to undo — the drag collapsed into one step.
    assert!(core.undo());
    assert_eq!(core.common_style_of_selection().text_color, Some(Some(red)));
    assert!(!core.can_undo(), "the drag must collapse into a single undo step");
}

#[test]
fn preview_cancel_restores_and_leaves_no_undo_residue() {
    let red = crate::color::Color::rgb(1.0, 0.0, 0.0);
    let blue = crate::color::Color::rgb(0.0, 0.0, 1.0);
    let doc = RichDoc::from_blocks(vec![Block::from_run(colored("hello", red))]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.select_all();
    core.feed_undo(0.0); // baseline undo point (red)
    assert!(!core.can_undo());

    core.begin_preview();
    // Preview to a different colour, feeding at times that WOULD stabilise into
    // an undo point (gap ≥ stable_time) if suspension were broken.
    core.exec(&RichCommand::SetTextColor(blue));
    core.feed_undo(0.1);
    core.feed_undo(2.0);
    assert_eq!(
        core.common_style_of_selection().text_color,
        Some(Some(blue)),
        "the preview updates the document live"
    );

    // Cancel: restore the captured snapshot + resume.
    core.cancel_preview();
    assert!(!core.is_previewing());
    assert_eq!(
        core.common_style_of_selection().text_color,
        Some(Some(red)),
        "cancel restores the original colour"
    );

    // Resume feeding: the restored state equals the committed baseline, so no
    // stray undo entry survives the cancelled preview.
    core.feed_undo(3.0);
    core.feed_undo(4.5);
    assert!(
        !core.can_undo(),
        "a cancelled preview must leave no undo residue"
    );
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

#[test]
fn enter_on_empty_list_item_exits_list_at_indent_zero() {
    // An empty bullet item + Enter becomes a plain paragraph (no new bullet).
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![],
        list: ListKind::Bullet,
        indent: 0,
        ..Block::new()
    }]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.set_caret(DocPos::new(0, 0), false);
    core.split();
    assert_eq!(core.doc().blocks.len(), 1, "no split occurred");
    assert_eq!(core.doc().blocks[0].list, ListKind::None);
    assert_eq!(core.doc().blocks[0].indent, 0);
}

#[test]
fn enter_on_empty_nested_list_item_outdents() {
    // An empty nested item + Enter decreases indent and keeps the list.
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![],
        list: ListKind::Ordered,
        indent: 2,
        ..Block::new()
    }]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.set_caret(DocPos::new(0, 0), false);
    core.split();
    assert_eq!(core.doc().blocks.len(), 1, "no split occurred");
    assert_eq!(core.doc().blocks[0].list, ListKind::Ordered);
    assert_eq!(core.doc().blocks[0].indent, 1);
}

#[test]
fn enter_on_non_empty_list_item_splits_normally() {
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![TextRun::plain("item")],
        list: ListKind::Bullet,
        ..Block::new()
    }]);
    let mut core = RichEditCore::new(doc, 16.0);
    core.set_caret(core.doc().end_pos(), false);
    core.split();
    assert_eq!(core.doc().blocks.len(), 2, "non-empty item splits");
    // The new block inherits the list kind (an empty continuation bullet).
    assert_eq!(core.doc().blocks[1].list, ListKind::Bullet);
    assert_eq!(core.doc().blocks[1].text(), "");
}

// Programmatic handle-API tests live in `handle_api_tests.rs` (declared from
// `editor.rs`) to keep this file under the 800-line cap.

// ── Styled clipboard (rich Copy / Cut / Paste) ─────────────────────────────

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
    assert_eq!(doc.blocks[0].list, ListKind::Bullet, "list decoration survives");
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
        runs: vec![TextRun::new("keep ", InlineStyle::default()), TextRun::new("cutme", red_24_bold())],
        ..Block::new()
    }]);
    let mut src = laid_out_editor(doc, 400.0, 200.0);
    src.on_event(&Event::FocusGained);
    // Select just "cutme".
    src.core.borrow_mut().set_selection(DocPos::new(0, 5), DocPos::new(0, 10));
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
    assert!(!dst.core.borrow().can_undo(), "paste was a single undo step");
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

// ── Double-click word / triple-click block selection ──────────────────────

fn plain_doc(blocks: &[&str]) -> RichDoc {
    RichDoc::from_blocks(blocks.iter().map(|s| Block::plain(*s)).collect())
}

#[test]
fn double_click_selects_word() {
    let mut ed = laid_out_editor(plain_doc(&["hello world"]), 400.0, 120.0);
    // DocPos byte 8 is inside "world".
    ed.begin_pointer_selection(DocPos::new(0, 8), 2, false);
    assert_eq!(ed.core.borrow().selected_plain_text(), "world");
}

#[test]
fn double_click_stops_at_punctuation() {
    let mut ed = laid_out_editor(plain_doc(&["foo.bar"]), 400.0, 120.0);
    ed.begin_pointer_selection(DocPos::new(0, 1), 2, false);
    assert_eq!(ed.core.borrow().selected_plain_text(), "foo");
    ed.begin_pointer_selection(DocPos::new(0, 5), 2, false);
    assert_eq!(ed.core.borrow().selected_plain_text(), "bar");
}

#[test]
fn triple_click_selects_block() {
    let mut ed = laid_out_editor(plain_doc(&["first block", "second block"]), 400.0, 200.0);
    // Triple-click in the second block selects that whole block.
    ed.begin_pointer_selection(DocPos::new(1, 4), 3, false);
    assert_eq!(ed.core.borrow().selected_plain_text(), "second block");
}

#[test]
fn word_drag_extends_by_whole_words() {
    let mut ed = laid_out_editor(plain_doc(&["alpha beta gamma"]), 400.0, 120.0);
    ed.begin_pointer_selection(DocPos::new(0, 7), 2, false); // "beta"
    assert_eq!(ed.core.borrow().selected_plain_text(), "beta");
    ed.extend_selection_drag(DocPos::new(0, 13)); // drag into "gamma"
    assert_eq!(ed.core.borrow().selected_plain_text(), "beta gamma");
    ed.extend_selection_drag(DocPos::new(0, 2)); // drag back into "alpha"
    assert_eq!(ed.core.borrow().selected_plain_text(), "alpha beta");
}

/// A real double `MouseDown` at the same point selects the word, proving the
/// multi-click counter is wired through the event handler.
#[test]
fn double_mouse_down_event_selects_word() {
    let mut ed = laid_out_editor(plain_doc(&["hello world"]), 400.0, 120.0);
    ed.on_event(&Event::FocusGained);
    let geom = ed.caret_geometry(DocPos::new(0, 8)).unwrap();
    let click = Point::new(geom.x + 0.5, geom.y_bottom + geom.height * 0.5);
    let down = Event::MouseDown {
        pos: click,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    };
    ed.on_event(&down);
    ed.on_event(&Event::MouseUp {
        pos: click,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    ed.on_event(&down);
    assert_eq!(ed.core.borrow().selected_plain_text(), "world");
}

// ── Text-rendering pipeline: LCD routing ───────────────────────────────────

/// The editor must follow the System LCD setting exactly like `Label` /
/// `TextField` / `TextArea`: an `LcdCoverage` backbuffer when LCD is on, a
/// grayscale `Rgba` backbuffer when it is off.
#[test]
fn backbuffer_mode_follows_lcd_setting() {
    use crate::widget::BackbufferMode;
    // Standard density so the high-scale hard cap in `lcd_enabled` doesn't mask
    // the explicit override under test.
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);

    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![sized("hello", 16.0)],
        ..Block::new()
    }]);
    let ed = laid_out_editor(doc, 400.0, 120.0);

    crate::font_settings::set_lcd_enabled(true);
    assert_eq!(ed.backbuffer_mode(), BackbufferMode::LcdCoverage);

    crate::font_settings::set_lcd_enabled(false);
    assert_eq!(ed.backbuffer_mode(), BackbufferMode::Rgba);

    crate::font_settings::clear_lcd_enabled_override();
}

/// An async font arrival reshapes the layout via `invalidate_layout` WITHOUT
/// advancing `core.rev()`, so the paint signature must fold in the layout
/// generation or the backbuffer would keep blitting the stale fallback-font
/// bitmap. Bumping the generation must change the sig.
#[test]
fn invalidate_layout_changes_cache_sig() {
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![sized("hello", 16.0)],
        ..Block::new()
    }]);
    let mut ed = laid_out_editor(doc, 400.0, 120.0);

    let before = ed.cache_sig();
    // No document/caret change — `core.rev()` is untouched.
    let rev_before = ed.core.borrow().rev();
    ed.invalidate_layout();
    assert_eq!(ed.core.borrow().rev(), rev_before, "rev must not change");
    let after = ed.cache_sig();
    assert_ne!(
        before, after,
        "invalidate_layout must change the cache sig so an async font arrival re-rasters"
    );
}

/// A `RichTextEdit` added **directly** as a tree child — with no host wrapper
/// forwarding `backbuffer_cache_mut` — must still engage the cached LCD/RGBA
/// backbuffer path when painted through the framework. The widget forwards the
/// backbuffer hooks itself, so `paint_subtree` takes the backbuffered branch and
/// populates the cache pixels. This mirrors the demo host test
/// (`host_engages_editor_backbuffer_cache`) but proves an unwrapped editor needs
/// no framework precondition the host was compensating for.
#[test]
fn direct_embed_engages_backbuffer_cache() {
    // Standard density so LCD is available; either mode still populates pixels
    // (the assertion only cares that the cached path ran, not which mode).
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);

    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![sized("hello world", 16.0)],
        ..Block::new()
    }]);
    let mut ed = RichTextEdit::new(doc, resolver()).with_font_size(16.0);

    // The widget exposes its own cache — no host forwarding involved.
    assert!(
        ed.backbuffer_cache_mut().is_some(),
        "RichTextEdit must expose its backbuffer cache directly"
    );

    ed.layout(Size::new(400.0, 300.0));

    let mut fb = crate::Framebuffer::new(400, 300);
    {
        let mut ctx = crate::GfxCtx::new(&mut fb);
        crate::widget::paint_subtree(&mut ed, &mut ctx);
    }

    let engaged = ed
        .backbuffer_cache_mut()
        .map(|c| c.pixels.is_some())
        .unwrap_or(false);
    assert!(
        engaged,
        "a directly-embedded RichTextEdit's backbuffer cache must populate after \
         a framework paint — the cached LCD/RGBA path did not engage"
    );
}
