//! A window wired to a shared `visible_cell` (`with_visible_cell`) reads its
//! visibility from that cell, so every programmatic route that shows or hides
//! it (`show`, `hide`, `toggle`, the generic `try_set_visible` hook) must
//! write the cell; otherwise the call changes nothing a caller can see.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::widgets::Container;

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn wired(initial: bool) -> (Window, Rc<Cell<bool>>) {
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("parse the test font"));
    let cell = Rc::new(Cell::new(initial));
    let w =
        Window::new("Wired", font, Box::new(Container::new())).with_visible_cell(Rc::clone(&cell));
    (w, cell)
}

#[test]
fn rust_only_show_and_hide_drive_a_wired_visible_cell() {
    let (mut w, cell) = wired(false);
    w.show();
    assert!(cell.get(), "show() must set the wired cell");
    assert!(w.is_visible());
    w.hide();
    assert!(!cell.get(), "hide() must clear the wired cell");
    assert!(!w.is_visible());
}

#[test]
fn rust_only_toggle_follows_a_wired_visible_cell() {
    // The cell, not the inherent flag, is the state: a window the cell has
    // shown toggles to hidden.
    let (mut w, cell) = wired(true);
    assert!(w.is_visible());
    w.toggle();
    assert!(!cell.get(), "toggle() of a cell-shown window hides it");
    assert!(!w.is_visible());
    w.toggle();
    assert!(cell.get(), "toggle() of a hidden window shows it");
    assert!(w.is_visible());
}

#[test]
fn rust_only_try_set_visible_still_drives_a_wired_visible_cell() {
    let (mut w, cell) = wired(false);
    assert!(Widget::try_set_visible(&mut w, true));
    assert!(cell.get());
    assert!(Widget::is_visible(&w));
    assert!(Widget::try_set_visible(&mut w, false));
    assert!(!cell.get());
    assert!(!Widget::is_visible(&w));
}
