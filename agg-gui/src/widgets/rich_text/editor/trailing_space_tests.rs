//! Trailing-whitespace caret tests for the interactive rich-text editor.
//!
//! Regression guards for the bug where typing spaces at the end of a line left
//! the caret parked on the last glyph: the word-wrap layout drops a trailing
//! space piece at a wrap point, and that same drop used to swallow an
//! *end-of-line* trailing space too, so `caret_geometry` clamped the caret back
//! onto the last word. The fix keeps a dangling end-of-line space in the line
//! (it has no glyph but still advances the pen), matching egui.
//!
//! Split out of `editor/tests.rs` to keep that file under the project's
//! 800-line cap; these drive the same production `RichTextEdit`.

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

fn type_space(ed: &mut RichTextEdit) {
    ed.on_event(&Event::KeyDown {
        key: Key::Char(' '),
        modifiers: Modifiers::default(),
    });
}

/// Typing a space at the end of a line must move the caret right by one
/// space-advance.
#[test]
fn trailing_space_advances_caret() {
    let mut ed = laid_out_editor(plain_doc(&["word"]), 400.0, 120.0);
    let x0 = ed.caret_geometry(DocPos::new(0, 4)).unwrap().x;

    ed.core.borrow_mut().set_caret(DocPos::new(0, 4), false);
    type_space(&mut ed);
    ed.layout(Size::new(400.0, 120.0));
    let x1 = ed.caret_geometry(DocPos::new(0, 5)).unwrap().x;
    assert!(
        x1 > x0 + 1.0,
        "caret must advance past the trailing space: x0={x0} x1={x1}"
    );

    // A second trailing space advances the caret again by the same step.
    ed.core.borrow_mut().set_caret(DocPos::new(0, 5), false);
    type_space(&mut ed);
    ed.layout(Size::new(400.0, 120.0));
    let x2 = ed.caret_geometry(DocPos::new(0, 6)).unwrap().x;
    assert!(
        (x2 - x1) > 1.0 && ((x2 - x1) - (x1 - x0)).abs() < 0.5,
        "each trailing space adds one uniform advance: x0={x0} x1={x1} x2={x2}"
    );
}

/// A trailing space typed right at a soft-wrap boundary must not panic and
/// keeps the caret on a valid, finite position (the space that lands at the
/// wrap point is still dropped from the layout — only end-of-line trailing
/// space is preserved).
#[test]
fn trailing_space_at_wrap_boundary_no_panic() {
    // Narrow width forces "aaaa bbbb" to wrap into two visual lines.
    let mut ed = laid_out_editor(plain_doc(&["aaaa bbbb"]), 70.0, 120.0);
    ed.core.borrow_mut().set_caret(DocPos::new(0, 4), false);
    type_space(&mut ed);
    ed.layout(Size::new(70.0, 120.0));
    let geom = ed.caret_geometry(DocPos::new(0, 5)).unwrap();
    assert!(geom.x.is_finite() && geom.y_bottom.is_finite());
}
