//! Double-click word / triple-click line selection tests for [`TextField`].
//!
//! Exercises the real `MouseDown` event path (so the shared multi-click counter
//! and the word/line selection wiring are covered end to end) against the
//! production widget — no logic copies.

use std::sync::Arc;

use super::*;
use crate::event::{Event, Key, Modifiers, MouseButton};
use crate::geometry::Point;
use crate::widget::Widget;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn laid_out(text: &str) -> TextField {
    let mut f = TextField::new(font()).with_text(text);
    f.layout(Size::new(400.0, 32.0));
    f.on_event(&Event::FocusGained);
    f
}

/// Widget-local x for the left edge of byte `off` (scroll_x is 0 fresh).
fn x_for_offset(f: &TextField, text: &str, off: usize) -> f64 {
    f.padding + measure_advance(&f.active_font(), &text[..off], f.font_size)
}

fn press(f: &mut TextField, x: f64) {
    f.on_event(&Event::MouseDown {
        pos: Point::new(x, 16.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    f.on_event(&Event::MouseUp {
        pos: Point::new(x, 16.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

#[test]
fn double_click_selects_word() {
    let text = "hello world";
    let mut f = laid_out(text);
    let x = x_for_offset(&f, text, 8); // inside "world"
    press(&mut f, x);
    press(&mut f, x);
    assert_eq!(f.selection(), "world");
}

#[test]
fn double_click_stops_at_punctuation() {
    let text = "foo.bar";
    let mut f = laid_out(text);
    let x_foo = x_for_offset(&f, text, 1);
    press(&mut f, x_foo);
    press(&mut f, x_foo);
    assert_eq!(f.selection(), "foo");

    let mut f = laid_out(text);
    let x_bar = x_for_offset(&f, text, 5);
    press(&mut f, x_bar);
    press(&mut f, x_bar);
    assert_eq!(f.selection(), "bar");
}

#[test]
fn triple_click_selects_whole_field() {
    let text = "select all of this";
    let mut f = laid_out(text);
    let x = x_for_offset(&f, text, 4);
    press(&mut f, x);
    press(&mut f, x);
    press(&mut f, x);
    assert_eq!(f.selection(), text);
}

/// Typing a space at the end of the field must move the caret right by one
/// space-advance. TextField is single-line and never trims, so this is a
/// regression guard against a future measurement change re-introducing the
/// trailing-whitespace caret bug that affected the wrapped editors.
#[test]
fn trailing_space_advances_caret() {
    let mut f = laid_out("word");
    // Caret at end of "word".
    f.on_event(&Event::KeyDown {
        key: Key::End,
        modifiers: Modifiers::default(),
    });
    let x0 = f.caret_x();

    let type_space = |f: &mut TextField| {
        f.on_event(&Event::KeyDown {
            key: Key::Char(' '),
            modifiers: Modifiers::default(),
        });
    };

    type_space(&mut f);
    let x1 = f.caret_x();
    assert!(
        x1 > x0 + 1.0,
        "caret must advance past the trailing space: x0={x0} x1={x1}"
    );

    type_space(&mut f);
    let x2 = f.caret_x();
    assert!(
        (x2 - x1) > 1.0 && ((x2 - x1) - (x1 - x0)).abs() < 0.5,
        "each trailing space adds one uniform advance: x0={x0} x1={x1} x2={x2}"
    );
}

/// A double-click landing on the last glyph of a word (near its right edge)
/// still selects the whole word, not the following whitespace.
#[test]
fn double_click_near_word_edge() {
    let text = "cat dog";
    let mut f = laid_out(text);
    // A couple of pixels into the last glyph of "cat" ('t' starts at byte 2).
    let x = x_for_offset(&f, text, 2) + 2.0;
    press(&mut f, x);
    press(&mut f, x);
    assert_eq!(f.selection(), "cat");
}
