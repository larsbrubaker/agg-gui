//! A disabled [`TextField`] draws its hint faded, as agg-sharp's disabled
//! `ThemedTextEditWidget` does: the hint is a `TextWidget`, whose disabled
//! colour is `new Color(textColor, 50)` (the hint's own colour at alpha 50 of
//! 255). The field's fill, border and typed text stay as they were, and a
//! disabled field takes neither focus nor input.
//!
//! Pixel tests through the production `paint_subtree`, as
//! `edit_field_state_color_tests` does, on the same helpers.

use std::rc::Rc;

use super::edit_field_state_color_tests::{
    close, edit_field_colors, field, hint_theme, render, rgb_of, strongest_text_pixel, State,
};
use super::*;
use crate::framebuffer::Framebuffer;

/// The hint's blue over the field's fill at C#'s disabled alpha, 50 of 255.
fn faded(hint: Color, background: Color) -> [u8; 3] {
    let a = 50.0 / 255.0;
    let mix = |h: f32, b: f32| ((h * a + b * (1.0 - a)) * 255.0).round() as u8;
    [
        mix(hint.r, background.r),
        mix(hint.g, background.g),
        mix(hint.b, background.b),
    ]
}

#[test]
fn a_disabled_field_draws_its_hint_at_alpha_50() {
    let t = hint_theme();
    let (bg, hint) = (t.background.unwrap(), t.placeholder_color.unwrap());
    let mut f = field("", t).with_placeholder("MMMM").with_enabled(false);
    let fb = render(&mut f, State::Normal);
    let p = strongest_text_pixel(&fb, rgb_of(bg));
    assert!(
        close(p, faded(hint, bg)),
        "disabled hint painted {p:?}, expected {:?}",
        faded(hint, bg)
    );
}

#[test]
fn an_enabled_field_still_draws_its_hint_at_full_strength() {
    let t = hint_theme();
    let (bg, hint) = (t.background.unwrap(), t.placeholder_color.unwrap());
    let mut f = field("", t).with_placeholder("MMMM").with_enabled(true);
    let fb = render(&mut f, State::Normal);
    let p = strongest_text_pixel(&fb, rgb_of(bg));
    assert!(close(p, rgb_of(hint)), "hint painted {p:?}");
}

#[test]
fn a_disabled_field_keeps_its_fill_and_typed_text() {
    let t = hint_theme();
    let (bg, text) = (t.background.unwrap(), t.text_color.unwrap());
    let mut f = field("MMMM", t).with_enabled(false);
    let fb = render(&mut f, State::Normal);
    let p = strongest_text_pixel(&fb, rgb_of(bg));
    assert!(close(p, rgb_of(text)), "typed text painted {p:?}");
    let centre = super::edit_field_state_color_tests::rgb(&fb, 2, 20);
    assert!(close(centre, rgb_of(bg)), "fill painted {centre:?}");
}

#[test]
fn a_disabled_field_takes_neither_focus_nor_input() {
    let mut f = field("abc", hint_theme()).with_enabled(false);
    assert!(!f.is_enabled());
    assert!(!f.is_focusable());
    f.set_bounds(Rect::new(0.0, 0.0, 160.0, 40.0));
    let pos = crate::Point::new(80.0, 20.0);
    f.on_event(&Event::MouseDown {
        pos,
        button: crate::MouseButton::Left,
        modifiers: crate::Modifiers::default(),
    });
    f.on_event(&Event::FocusGained);
    f.on_event(&Event::TextInput { text: "x".into() });
    assert_eq!(f.text(), "abc");
    assert!(!f.is_focusable());
}

#[test]
fn the_enabled_cell_follows_its_owner_and_try_set_enabled_pins_it() {
    let cell = Rc::new(Cell::new(false));
    let mut f = field("", hint_theme()).with_enabled_cell(Rc::clone(&cell));
    assert!(!f.is_enabled());
    cell.set(true);
    assert!(f.is_enabled());
    assert!(f.try_set_enabled(false));
    assert!(!f.is_enabled());
    assert!(f.try_set_enabled(true));
    assert!(f.is_enabled());
}

// ── A cell or flag that disables a focused field ────────────────────────────
//
// As for `TextArea`: the owner's cell can turn the enabled state off with no
// call into the field, so the field notices at its next layout, lets go of
// focus and repaints unfocused (faded hint, unfocused selection colour).

fn lay_out(f: &mut TextField) {
    f.set_bounds(Rect::new(0.0, 0.0, 160.0, 40.0));
    f.layout(crate::Size::new(160.0, 40.0));
}

#[test]
fn rust_only_a_cell_that_disables_a_focused_field_repaints_it_with_the_faded_hint() {
    let cell = Rc::new(Cell::new(true));
    let t = hint_theme();
    let (bg, hint) = (t.background.unwrap(), t.placeholder_color.unwrap());
    let mut f = field("", t)
        .with_placeholder("MMMM")
        .with_enabled_cell(Rc::clone(&cell));
    lay_out(&mut f);
    f.on_event(&Event::FocusGained);
    assert!(f.focused);
    cell.set(false);
    // The next layout notices the change: focus is released and the cached
    // bitmap re-rendered with the faded hint.
    let fb = render(&mut f, State::Normal);
    assert!(!f.focused, "a disabled field keeps no focus");
    let p = strongest_text_pixel(&fb, rgb_of(bg));
    assert!(
        close(p, faded(hint, bg)),
        "hint painted {p:?}, expected {:?}",
        faded(hint, bg)
    );
}

#[test]
fn rust_only_a_cell_that_disables_a_focused_field_drops_its_caret_and_focused_selection() {
    let cell = Rc::new(Cell::new(true));
    let focused_sel = Color::rgb(1.0, 0.0, 0.0);
    let unfocused_sel = Color::rgb(0.0, 1.0, 0.0);
    let t = TextFieldTheme {
        selection_bg: Some(focused_sel),
        selection_bg_unfocused: Some(unfocused_sel),
        ..edit_field_colors()
    };
    let mut f = field("MMMM", t).with_enabled_cell(Rc::clone(&cell));
    lay_out(&mut f);
    f.on_event(&Event::FocusGained);
    {
        let mut st = f.edit.borrow_mut();
        st.anchor = 0;
        st.cursor = st.text.len();
    }
    let count = |fb: &Framebuffer, want: Color| {
        fb.pixels()
            .chunks_exact(4)
            .filter(|p| close([p[0], p[1], p[2]], rgb_of(want)))
            .count()
    };
    let focused = render(&mut f, State::Normal);
    assert!(
        count(&focused, focused_sel) > 50,
        "focused selection colour"
    );
    assert_eq!(count(&focused, unfocused_sel), 0);
    cell.set(false);
    let disabled = render(&mut f, State::Normal);
    assert!(!f.focused);
    assert!(f.focus_time.is_none(), "the caret blink is stopped");
    assert!(
        count(&disabled, unfocused_sel) > 50,
        "unfocused selection colour"
    );
    assert_eq!(count(&disabled, focused_sel), 0);
}

#[test]
fn rust_only_set_enabled_toggles_editing_and_drops_focus() {
    let mut f = field("", hint_theme());
    lay_out(&mut f);
    f.on_event(&Event::FocusGained);
    f.set_enabled(false);
    assert!(!f.focused);
    f.on_event(&Event::FocusGained);
    f.on_event(&Event::TextInput { text: "a".into() });
    assert_eq!(f.text(), "");
    f.set_enabled(true);
    f.on_event(&Event::FocusGained);
    f.on_event(&Event::TextInput { text: "a".into() });
    assert_eq!(f.text(), "a");
}

#[test]
fn rust_only_a_disabled_field_does_not_accept_text_input() {
    // The web shell keeps its hidden IME input focused, and the on-screen
    // keyboard up, for a focused widget that accepts text input.
    let f = field("", hint_theme()).with_enabled(false);
    assert!(!f.accepts_text_input());
    assert!(field("", hint_theme()).accepts_text_input());
}
