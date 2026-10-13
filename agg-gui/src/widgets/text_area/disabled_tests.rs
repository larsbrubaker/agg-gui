//! A disabled [`TextArea`] behaves as a disabled `TextField` does (see
//! `text_field/disabled_tests.rs`): agg-sharp's `Enabled = false` on a
//! `ThemedTextEditWidget`. The area draws its hint at C#'s disabled alpha (50 of
//! 255), keeps its fill and typed text, and takes neither focus nor input.
//!
//! Pixel tests go through the production `paint_subtree`, on the helpers of
//! `theme_tests`.

use std::cell::Cell;
use std::rc::Rc;

use super::theme_tests::{area, close, colors, render, rgb, rgb_of, strongest_pixel, State};
use super::*;
use crate::color::Color;

/// The hint's colour over the area's fill at alpha 50 of 255.
fn faded(hint: Color, background: Color) -> [u8; 3] {
    let a = 50.0 / 255.0;
    let mix = |h: f32, b: f32| ((h * a + b * (1.0 - a)) * 255.0).round() as u8;
    [
        mix(hint.r, background.r),
        mix(hint.g, background.g),
        mix(hint.b, background.b),
    ]
}

/// Theme whose caret cannot be mistaken for ink (it paints in the fill).
fn theme() -> crate::widgets::TextFieldTheme {
    let t = colors();
    crate::widgets::TextFieldTheme {
        cursor_color: t.background,
        ..t
    }
}

fn key(ta: &mut TextArea, key: Key) {
    ta.on_event(&Event::KeyDown {
        key,
        modifiers: Modifiers::default(),
    });
}

fn lay_out(ta: &mut TextArea) {
    ta.set_bounds(Rect::new(0.0, 0.0, 200.0, 100.0));
    ta.layout(Size::new(200.0, 100.0));
}

#[test]
fn rust_only_a_disabled_area_draws_its_hint_at_alpha_50() {
    let t = theme();
    let (bg, hint) = (t.background.unwrap(), t.placeholder_color.unwrap());
    let mut ta = area("", "MMMM", t).with_enabled(false);
    let fb = render(&mut ta, State::Normal);
    let p = strongest_pixel(&fb, rgb_of(bg));
    assert!(
        close(p, faded(hint, bg)),
        "disabled hint painted {p:?}, expected {:?}",
        faded(hint, bg)
    );
}

#[test]
fn rust_only_an_enabled_area_still_draws_its_hint_at_full_strength() {
    let t = theme();
    let (bg, hint) = (t.background.unwrap(), t.placeholder_color.unwrap());
    let mut ta = area("", "MMMM", t).with_enabled(true);
    let fb = render(&mut ta, State::Normal);
    let p = strongest_pixel(&fb, rgb_of(bg));
    assert!(close(p, rgb_of(hint)), "hint painted {p:?}");
}

#[test]
fn rust_only_a_disabled_area_keeps_its_fill_and_typed_text() {
    let t = theme();
    let (bg, text) = (t.background.unwrap(), t.text_color.unwrap());
    let mut ta = area("MMMM", "", t).with_enabled(false);
    let fb = render(&mut ta, State::Normal);
    let p = strongest_pixel(&fb, rgb_of(bg));
    assert!(close(p, rgb_of(text)), "typed text painted {p:?}");
    let low = rgb(&fb, 100, 20);
    assert!(close(low, rgb_of(bg)), "fill painted {low:?}");
}

#[test]
fn rust_only_a_disabled_area_takes_neither_focus_nor_input() {
    let mut ta = area("abc", "", theme()).with_enabled(false);
    assert!(!ta.is_enabled());
    assert!(!ta.is_focusable());
    lay_out(&mut ta);
    let down = ta.on_event(&Event::MouseDown {
        pos: Point::new(80.0, 50.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(down, EventResult::Ignored);
    ta.on_event(&Event::FocusGained);
    ta.on_event(&Event::TextInput { text: "x".into() });
    key(&mut ta, Key::Char('y'));
    key(&mut ta, Key::Backspace);
    assert_eq!(ta.text(), "abc");
    assert!(!ta.is_focusable());
    assert!(!ta.focused, "a disabled area must not take focus");
}

#[test]
fn rust_only_a_disabled_area_shows_no_hover_state() {
    let mut ta = area("abc", "", theme()).with_enabled(false);
    lay_out(&mut ta);
    ta.on_event(&Event::MouseMove {
        pos: Point::new(80.0, 50.0),
    });
    assert!(!ta.hovered);
}

#[test]
fn rust_only_the_area_enabled_cell_follows_its_owner_and_try_set_enabled_pins_it() {
    let cell = Rc::new(Cell::new(false));
    let mut ta = area("", "", theme()).with_enabled_cell(Rc::clone(&cell));
    assert!(!ta.is_enabled());
    cell.set(true);
    assert!(ta.is_enabled());
    assert!(ta.is_focusable());
    assert!(ta.try_set_enabled(false));
    cell.set(true);
    assert!(!ta.is_enabled(), "the pinned state outranks the cell");
    assert!(ta.try_set_enabled(true));
    assert!(ta.is_enabled());
}

#[test]
fn rust_only_a_cell_that_disables_a_focused_area_repaints_it_with_the_faded_hint() {
    let cell = Rc::new(Cell::new(true));
    let mut ta = area("", "MMMM", theme()).with_enabled_cell(Rc::clone(&cell));
    lay_out(&mut ta);
    ta.on_event(&Event::FocusGained);
    assert!(ta.focused);
    cell.set(false);
    // The next layout notices the change: focus is released and the cached
    // bitmap re-rendered with the faded hint.
    let t = theme();
    let (bg, hint) = (t.background.unwrap(), t.placeholder_color.unwrap());
    let fb = render(&mut ta, State::Normal);
    assert!(!ta.focused);
    let p = strongest_pixel(&fb, rgb_of(bg));
    assert!(close(p, faded(hint, bg)), "hint painted {p:?}");
}

#[test]
fn rust_only_set_enabled_toggles_editing_and_drops_focus() {
    let mut ta = area("", "", theme());
    lay_out(&mut ta);
    ta.on_event(&Event::FocusGained);
    ta.set_enabled(false);
    assert!(!ta.focused);
    ta.on_event(&Event::FocusGained);
    key(&mut ta, Key::Char('a'));
    assert_eq!(ta.text(), "");
    ta.set_enabled(true);
    ta.on_event(&Event::FocusGained);
    key(&mut ta, Key::Char('a'));
    assert_eq!(ta.text(), "a");
}
