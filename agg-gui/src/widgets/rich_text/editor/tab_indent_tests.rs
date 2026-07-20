//! Tests for Tab / Shift+Tab indenting in the interactive rich-text editor.
//!
//! A focused [`RichTextEdit`] maps Tab → increase indent and Shift+Tab →
//! decrease indent for every touched block, reusing the toolbar's
//! [`RichCommand::Indent`] / [`Outdent`](crate::widgets::rich_text::commands::RichCommand::Outdent)
//! (a list item's "level" *is* its `block.indent`, so one command covers list
//! items and plain blocks). Plain Tab / Shift+Tab are `Consumed` so the App's
//! focus traversal does not steal focus mid-edit; Ctrl+Tab stays `Ignored` (the
//! App routes it straight to traversal as the keyboard escape hatch).
//!
//! Split out of `editor/tests.rs` to keep that file under the project's
//! 800-line cap; these drive the same shared `RichEditCore` the widget renders.

use std::sync::Arc;

use crate::event::{Event, EventResult, Key, Modifiers};
use crate::geometry::Size;
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::rich_text::model::{Block, InlineStyle, ListKind, RichDoc, TextRun};
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

fn tab_event(shift: bool) -> Event {
    Event::KeyDown {
        key: Key::Tab,
        modifiers: Modifiers {
            shift,
            ..Default::default()
        },
    }
}

fn ctrl_tab_event() -> Event {
    Event::KeyDown {
        key: Key::Tab,
        modifiers: Modifiers {
            ctrl: true,
            ..Default::default()
        },
    }
}

fn bullet_block(indent: u8) -> Block {
    Block {
        runs: vec![TextRun::plain("item")],
        list: ListKind::Bullet,
        indent,
        ..Block::new()
    }
}

#[test]
fn tab_indents_list_item_one_level() {
    let mut ed = laid_out_editor(RichDoc::from_blocks(vec![bullet_block(0)]), 400.0, 120.0);
    ed.on_event(&Event::FocusGained);
    ed.on_event(&tab_event(false));
    assert_eq!(ed.core.borrow().doc().blocks[0].indent, 1, "Tab indents the list item");
    assert_eq!(
        ed.core.borrow().doc().blocks[0].list,
        ListKind::Bullet,
        "the list decoration survives the indent"
    );
}

#[test]
fn shift_tab_outdents_list_item_one_level() {
    let mut ed = laid_out_editor(RichDoc::from_blocks(vec![bullet_block(2)]), 400.0, 120.0);
    ed.on_event(&Event::FocusGained);
    ed.on_event(&tab_event(true));
    assert_eq!(ed.core.borrow().doc().blocks[0].indent, 1, "Shift+Tab outdents the list item");
    assert_eq!(ed.core.borrow().doc().blocks[0].list, ListKind::Bullet);
}

#[test]
fn tab_increases_block_indent_outside_a_list() {
    let mut ed = laid_out_editor(plain_doc(&["paragraph"]), 400.0, 120.0);
    ed.on_event(&Event::FocusGained);
    assert_eq!(ed.core.borrow().doc().blocks[0].indent, 0);
    ed.on_event(&tab_event(false));
    assert_eq!(ed.core.borrow().doc().blocks[0].indent, 1, "Tab increases block indent");
    assert_eq!(
        ed.core.borrow().doc().blocks[0].list,
        ListKind::None,
        "still a plain paragraph"
    );
}

#[test]
fn shift_tab_decreases_block_indent() {
    let doc = RichDoc::from_blocks(vec![Block {
        runs: vec![TextRun::plain("paragraph")],
        indent: 2,
        ..Block::new()
    }]);
    let mut ed = laid_out_editor(doc, 400.0, 120.0);
    ed.on_event(&Event::FocusGained);
    ed.on_event(&tab_event(true));
    assert_eq!(ed.core.borrow().doc().blocks[0].indent, 1, "Shift+Tab decreases block indent");
}

#[test]
fn tab_and_shift_tab_are_consumed() {
    let mut ed = laid_out_editor(plain_doc(&["paragraph"]), 400.0, 120.0);
    ed.on_event(&Event::FocusGained);
    assert_eq!(
        ed.on_event(&tab_event(false)),
        EventResult::Consumed,
        "Tab is consumed so App focus traversal does not steal focus"
    );
    assert_eq!(
        ed.on_event(&tab_event(true)),
        EventResult::Consumed,
        "Shift+Tab is consumed too"
    );
}

#[test]
fn ctrl_tab_is_ignored_and_does_not_indent() {
    let mut ed = laid_out_editor(plain_doc(&["paragraph"]), 400.0, 120.0);
    ed.on_event(&Event::FocusGained);
    assert_eq!(
        ed.on_event(&ctrl_tab_event()),
        EventResult::Ignored,
        "Ctrl+Tab falls through so the App can traverse focus"
    );
    assert_eq!(
        ed.core.borrow().doc().blocks[0].indent, 0,
        "Ctrl+Tab must not change the indent"
    );
}
