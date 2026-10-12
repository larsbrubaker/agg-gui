//! Rust-only tests of the unwrapped mode (`no_wrap.rs`) beyond what
//! `TextEditWordWrapTests` (`word_wrap_tests.rs`) pins: the sideways scroll
//! follows the caret both ways, hit-testing reads the scrolled text, and
//! turning wrapping back on shows the start of the line again.

use std::sync::Arc;

use super::*;
use crate::event::{Event, Key, Modifiers};
use crate::widget::Widget;

const LONG_LINE: &str = "abcdefghijk lmnopqrstuvwxyz123456789";

/// An unwrapped 100 x 100 area holding `LONG_LINE` (far wider than 100) with
/// the caret at its end, scrolled there.
fn scrolled_to_end() -> TextArea {
    let mut ta = TextArea::new(Arc::new(crate::fonts::standard_ui_font()))
        .with_font_size(16.0)
        .with_padding(0.0)
        .with_line_spacing(1.0)
        .with_word_wrap(false)
        .with_text(LONG_LINE);
    ta.set_bounds(Rect::new(0.0, 0.0, 100.0, 100.0));
    ta.layout(Size::new(100.0, 100.0));
    ta.ensure_cursor_visible();
    ta
}

fn key(ta: &mut TextArea, key: Key) {
    ta.on_event(&Event::KeyDown {
        key,
        modifiers: Modifiers::default(),
    });
    ta.layout(Size::new(100.0, 100.0));
}

#[test]
fn rust_only_unwrapped_caret_at_the_end_sits_inside_the_right_edge() {
    let ta = scrolled_to_end();
    let caret = ta.pos_for_cursor(LONG_LINE.len());
    assert!(ta.top_left_offset_x() < 0.0);
    // the caret is drawn the caret room inside the right edge, not past it
    assert!(
        (caret.x - (100.0 - 2.0)).abs() < 1e-9,
        "caret x {}",
        caret.x
    );
}

#[test]
fn rust_only_unwrapped_home_scrolls_back_to_the_line_start() {
    let mut ta = scrolled_to_end();
    key(&mut ta, Key::Home);
    assert_eq!(ta.cursor(), 0);
    assert_eq!(ta.top_left_offset_x(), 0.0);
}

#[test]
fn rust_only_unwrapped_click_hits_the_scrolled_text() {
    let ta = scrolled_to_end();
    // the right edge of the field shows the end of the line, so a point there
    // is the end of the text, not the 100-pixel column of the unscrolled line
    let hit = ta.byte_offset_at(Point::new(98.0, 90.0));
    assert_eq!(hit, LONG_LINE.len());
}

#[test]
fn rust_only_turning_wrap_back_on_shows_the_line_start() {
    let mut ta = scrolled_to_end();
    ta.set_word_wrap(true);
    ta.layout(Size::new(100.0, 100.0));
    assert!(ta.word_wrap());
    assert_eq!(ta.top_left_offset_x(), 0.0);
    assert!(ta.visual_line_count() > 1);
}
