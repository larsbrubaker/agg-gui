//! A field's hint from a shared cell ([`TextField::with_placeholder_cell`]):
//! the owner writes the cell and the next paint shows the new hint, with the
//! field never rebuilt (its cached bitmap is dropped when the hint changes);
//! a cell takes the place of the fixed hint.
//!
//! Pixel tests through the production paint, on the
//! `edit_field_state_color_tests` helpers.

use std::cell::RefCell;
use std::rc::Rc;

use super::edit_field_state_color_tests::{
    close, field, hint_theme, render, rgb_of, strongest_text_pixel, State,
};
use crate::geometry::Size;
use crate::widget::Widget;

#[test]
fn the_hint_follows_its_cell_without_rebuilding_the_field() {
    let t = hint_theme();
    let (bg, hint) = (t.background.unwrap(), t.placeholder_color.unwrap());
    let cell = Rc::new(RefCell::new(String::new()));
    let mut f = field("", t).with_placeholder_cell(Rc::clone(&cell));
    let empty = strongest_text_pixel(&render(&mut f, State::Normal), rgb_of(bg));
    assert!(!close(empty, rgb_of(hint)), "an empty cell drew {empty:?}");

    *cell.borrow_mut() = "MMMM".to_string();
    assert_eq!(f.placeholder_text(), "MMMM");
    let shown = strongest_text_pixel(&render(&mut f, State::Normal), rgb_of(bg));
    assert!(
        close(shown, rgb_of(hint)),
        "the cell's hint painted {shown:?}"
    );
}

#[test]
fn a_cell_takes_the_place_of_the_fixed_hint() {
    let cell = Rc::new(RefCell::new("Suggested".to_string()));
    let f = field("", hint_theme())
        .with_placeholder("Fixed")
        .with_placeholder_cell(cell);
    assert_eq!(f.placeholder_text(), "Suggested");
    assert_eq!(
        field("", hint_theme())
            .with_placeholder("Fixed")
            .placeholder_text(),
        "Fixed"
    );
}

#[test]
fn a_new_hint_from_the_cell_drops_the_cached_bitmap() {
    let cell = Rc::new(RefCell::new("Width".to_string()));
    let mut f = field("", hint_theme()).with_placeholder_cell(Rc::clone(&cell));
    f.layout(Size::new(200.0, 40.0));
    let before = f.last_sig.clone();
    f.layout(Size::new(200.0, 40.0));
    assert!(
        f.last_sig == before,
        "nothing changed, so the cache is kept"
    );
    *cell.borrow_mut() = "Arm Width".to_string();
    f.layout(Size::new(200.0, 40.0));
    assert!(
        f.last_sig != before,
        "the cached bitmap still holds the old hint"
    );
}
