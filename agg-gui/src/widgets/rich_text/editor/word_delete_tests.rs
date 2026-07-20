//! Tests for word-wise deletion (Ctrl/Alt+Backspace and Ctrl/Alt+Delete) in the
//! interactive rich-text editor.
//!
//! A focused [`RichTextEdit`] maps Ctrl/Alt+Backspace → delete to the previous
//! word boundary and Ctrl/Alt+Delete → delete to the next, matching egui's
//! `delete_previous_word` / `delete_next_word`. The boundary is the editor's own
//! [`word_target`](super::RichTextEdit::word_target), so a word delete removes
//! exactly the span a Ctrl+Arrow motion would traverse — including a merge into
//! the adjacent block when the caret sits at a block edge. An active selection
//! takes precedence: only the selection is removed. Each deletion is a single
//! undo step.
//!
//! Split out of `editor/tests.rs` (at the 800-line cap) to keep that file under
//! the project's limit; these drive the same shared `RichEditCore` the widget
//! renders, through real key events.

use std::sync::Arc;

use crate::event::{Event, Key, Modifiers};
use crate::geometry::Size;
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::rich_text::model::{Block, DocPos, InlineStyle, RichDoc};
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

fn plain_doc(blocks: &[&str]) -> RichDoc {
    RichDoc::from_blocks(blocks.iter().map(|s| Block::plain(*s)).collect())
}

/// A key event with Ctrl held (the word-delete chord); Alt shares the same path.
fn ctrl_key(key: Key) -> Event {
    Event::KeyDown {
        key,
        modifiers: Modifiers {
            ctrl: true,
            ..Default::default()
        },
    }
}

fn focused_editor(blocks: &[&str]) -> RichTextEdit {
    let mut ed = laid_out_editor(plain_doc(blocks), 400.0, 120.0);
    ed.on_event(&Event::FocusGained);
    ed
}

fn set_caret(ed: &RichTextEdit, block: usize, byte: usize) {
    ed.core.borrow_mut().set_caret(DocPos::new(block, byte), false);
}

fn text(ed: &RichTextEdit) -> String {
    ed.core.borrow().doc().plain_text()
}

#[test]
fn ctrl_backspace_mid_word_deletes_back_to_word_start() {
    // Caret after "wor" in "hello world" (byte 9); Ctrl+Backspace removes "wor".
    let mut ed = focused_editor(&["hello world"]);
    set_caret(&ed, 0, 9);
    ed.on_event(&ctrl_key(Key::Backspace));
    assert_eq!(text(&ed), "hello ld", "mid-word delete removes to the word start");
}

#[test]
fn ctrl_backspace_at_word_start_deletes_previous_word() {
    // Caret at the start of "world" (byte 6). Like a Ctrl+ArrowLeft motion, the
    // delete swallows the whitespace and the preceding word "hello ".
    let mut ed = focused_editor(&["hello world"]);
    set_caret(&ed, 0, 6);
    ed.on_event(&ctrl_key(Key::Backspace));
    assert_eq!(text(&ed), "world", "at word start, delete removes the previous word");
}

#[test]
fn ctrl_delete_forward_deletes_next_word() {
    // Caret at the start; Ctrl+Delete removes "hello " forward to the next word.
    let mut ed = focused_editor(&["hello world"]);
    set_caret(&ed, 0, 0);
    ed.on_event(&ctrl_key(Key::Delete));
    assert_eq!(text(&ed), "world", "forward word delete removes to the next word");
}

#[test]
fn ctrl_backspace_with_selection_deletes_only_the_selection() {
    // Selecting "llo" (bytes 2..5) and pressing Ctrl+Backspace must remove just
    // the selection, not a whole word — standard selection precedence.
    let mut ed = focused_editor(&["hello world"]);
    ed.core
        .borrow_mut()
        .set_selection(DocPos::new(0, 2), DocPos::new(0, 5));
    ed.on_event(&ctrl_key(Key::Backspace));
    assert_eq!(text(&ed), "he world", "an active selection is deleted verbatim");
}

#[test]
fn ctrl_backspace_at_block_start_merges_with_previous_block() {
    // Caret at the start of the second block. `word_target` crosses to the end
    // of the previous block, so the delete merges the two paragraphs exactly as
    // a plain Backspace at block start would.
    let mut ed = focused_editor(&["foo", "bar"]);
    set_caret(&ed, 1, 0);
    ed.on_event(&ctrl_key(Key::Backspace));
    assert_eq!(ed.core.borrow().doc().blocks.len(), 1, "the two blocks merged");
    assert_eq!(text(&ed), "foobar", "merged text is the two blocks joined");
}

#[test]
fn ctrl_backspace_is_one_undo_step() {
    // A word delete collapses into a single undo point that restores the word.
    let mut ed = focused_editor(&["hello world"]);
    set_caret(&ed, 0, 11);
    ed.core.borrow_mut().feed_undo(0.0); // baseline undo point
    ed.on_event(&ctrl_key(Key::Backspace));
    assert_eq!(text(&ed), "hello ", "the trailing word is deleted");
    // Let the change settle past the coalescing window into one undo point.
    ed.core.borrow_mut().feed_undo(0.1);
    ed.core.borrow_mut().feed_undo(1.5);
    assert!(ed.core.borrow().can_undo());
    assert!(ed.core.borrow_mut().undo(), "one undo reverts the word delete");
    assert_eq!(text(&ed), "hello world", "undo restores the deleted word");
    assert!(
        !ed.core.borrow().can_undo(),
        "the word delete was a single undo step"
    );
}
