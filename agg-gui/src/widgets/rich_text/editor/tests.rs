//! Unit tests for the interactive rich-text editor: the logical [`RichEditCore`]
//! (typing / pending style / toggle-over-selection / undo coalescing / clipboard
//! round-trip) and the widget's caret hit-testing across mixed font sizes.

use std::sync::Arc;

use crate::event::{Event, Modifiers, MouseButton};
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
