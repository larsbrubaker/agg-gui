//! Unit tests for the programmatic [`RichEditHandle`] / [`RichTextEdit`] APIs —
//! `select_all` / `set_caret` / `set_selection` (clamped) / `selection` /
//! `plain_text` / `load` — plus `load`'s interaction with the undo history and
//! an in-flight live-preview session.
//!
//! Split out of `editor/tests.rs` to keep that file under the project's
//! 800-line cap; these exercise the same shared `RichEditCore` the widget drives.

use std::sync::Arc;

use crate::color::Color;
use crate::text::Font;
use crate::widgets::rich_text::commands::RichCommand;
use crate::widgets::rich_text::model::{Block, DocPos, DocRange, InlineStyle, RichDoc};
use crate::widgets::rich_text::view::SharedResolver;
use crate::widgets::rich_text::RichTextEdit;

use super::core::RichEditCore;

const FONT_BYTES: &[u8] = include_bytes!("../../../../../demo/assets/Arial-Regular.ttf");

fn resolver() -> SharedResolver {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("test font loads"));
    std::rc::Rc::new(move |_: &InlineStyle| Arc::clone(&font))
}

/// A toolbar-style flow: `select_all` through the handle, then `ToggleBold`,
/// must bold the whole document — proving the handle's selection API drives the
/// same core the widget renders.
#[test]
fn handle_select_all_then_bold() {
    let editor = RichTextEdit::new(RichDoc::from_blocks(vec![Block::plain("hello")]), resolver());
    let handle = editor.handle();
    handle.select_all();
    assert_eq!(
        handle.selection(),
        DocRange::new(DocPos::new(0, 0), DocPos::new(0, 5)),
    );
    handle.exec(&RichCommand::ToggleBold);
    assert_eq!(handle.common_style_of_selection().bold, Some(true));
    assert_eq!(handle.plain_text(), "hello");
}

/// `set_caret` / `set_selection` clamp out-of-range positions onto valid ones.
#[test]
fn handle_set_selection_clamps() {
    let editor = RichTextEdit::new(RichDoc::from_blocks(vec![Block::plain("hi")]), resolver());
    let handle = editor.handle();
    // A wildly out-of-range selection clamps to the single block's extent.
    handle.set_selection(DocRange::new(DocPos::new(9, 9), DocPos::new(0, 99)));
    let sel = handle.selection();
    assert_eq!(sel.start, DocPos::new(0, 2), "anchor clamped to end of block");
    assert_eq!(sel.end, DocPos::new(0, 2), "caret clamped to end of block");
    // A collapsed caret set past the end clamps too.
    handle.set_caret(DocPos::new(0, 50));
    assert!(handle.selection().is_empty());
    assert_eq!(handle.selection().start, DocPos::new(0, 2));
}

/// The handle's `load` replaces the document and resets the caret to the start.
#[test]
fn handle_load_replaces_content() {
    let editor = RichTextEdit::new(RichDoc::from_blocks(vec![Block::plain("old")]), resolver());
    let handle = editor.handle();
    handle.select_all();
    handle.load(RichDoc::from_blocks(vec![Block::plain("brand new")]));
    assert_eq!(handle.plain_text(), "brand new");
    assert_eq!(handle.selection().start, DocPos::new(0, 0));
    assert!(handle.selection().is_empty(), "caret collapsed after load");
}

/// `load` **discards the undo history**: after loading, there is nothing to undo
/// back to even though the prior document had a coalesced edit.  Driven at the
/// core level, which owns the per-frame undo feed.
#[test]
fn load_resets_undo_history() {
    let mut core = RichEditCore::new(RichDoc::from_blocks(vec![Block::plain("old")]), 16.0);
    // Baseline, then a coalesced edit → one undo point.
    core.feed_undo(0.0);
    core.select_all();
    core.exec(&RichCommand::ToggleBold);
    core.feed_undo(0.1);
    core.feed_undo(2.0);
    assert!(core.can_undo(), "edit created an undo point");

    core.load(RichDoc::from_blocks(vec![Block::plain("brand new")]));
    assert_eq!(core.doc().plain_text(), "brand new");
    assert!(!core.can_undo(), "load must discard the prior undo history");
    assert_eq!(core.caret(), DocPos::new(0, 0), "caret reset to document start");
}

/// `load` mid-preview must abandon the session: no dangling snapshot (which
/// would clobber the loaded doc on a later `cancel_preview`) and no dead undo
/// (feeding stays suspended forever otherwise).
#[test]
fn load_during_preview_abandons_session() {
    let mut core = RichEditCore::new(RichDoc::from_blocks(vec![Block::plain("old")]), 16.0);
    core.select_all();
    // Begin a live preview and mutate mid-drag, exactly like a colour dialog.
    core.begin_preview();
    core.exec(&RichCommand::SetTextColor(Color::from_rgb8(255, 0, 0)));
    assert!(core.is_previewing(), "preview active before load");

    core.load(RichDoc::from_blocks(vec![Block::plain("brand new")]));
    assert!(!core.is_previewing(), "load must end the preview session");

    // Undo feeding works again (not stuck suspended): a fresh edit banks a step.
    core.feed_undo(0.0);
    core.select_all();
    core.exec(&RichCommand::ToggleBold);
    core.feed_undo(0.1);
    core.feed_undo(2.0);
    assert!(core.can_undo(), "undo must be functional after load-during-preview");

    // A stale cancel_preview is now a no-op — it must NOT restore the old doc.
    core.cancel_preview();
    assert_eq!(
        core.doc().plain_text(),
        "brand new",
        "cancel_preview after load must not clobber the loaded document"
    );
}
