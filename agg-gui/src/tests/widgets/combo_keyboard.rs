//! `ComboBox` keyboard, ported from agg-sharp's `DropDownList`
//! (`combo_box/keyboard.rs`): agg-sharp `PopupMenuKeyboardTests.
//! DropDownListKeyboardIsUnaffected` (Tests/Agg.Tests/Agg.UI) through the
//! real `App`, plus the `rust_only_*` pins for the C# bugs that file fixes
//! and for the keys agg-gui adds on a closed list.
//!
//! C#'s `SelectedIndex` is `ComboBox::selected_index` (`None` for -1, the
//! list starting on its "- select -" placeholder).

use super::*;
use crate::text::Font;
use crate::{ComboBox, Key};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

/// An app whose root is `combo`, focused (C# `dropDownList.Focus()`).
fn focused(combo: ComboBox) -> App {
    let mut app = App::new(Box::new(combo));
    app.layout(Size::new(600.0, 400.0));
    app.focus_first();
    assert!(app.focused_path().is_some(), "the combo takes focus");
    app
}

/// C#'s "Fruit" list: Apple, Banana, Cherry, nothing selected.
fn fruit() -> ComboBox {
    ComboBox::new(vec!["Apple", "Banana", "Cherry"], 0, font()).with_no_selection("- select -")
}

fn combo(app: &App) -> &ComboBox {
    app.root()
        .as_any()
        .and_then(|a| a.downcast_ref::<ComboBox>())
        .expect("the root is the combo")
}

fn down(app: &mut App, key: Key) {
    app.on_key_down(key, Modifiers::default());
}

fn up(app: &mut App, key: Key) {
    app.on_key_up(key, Modifiers::default());
}

#[test]
fn drop_down_list_keyboard_is_unaffected() {
    let mut app = focused(fruit());

    // Down on the closed list opens it, exactly as DropDownList.OnKeyUp says
    up(&mut app, Key::ArrowDown);
    assert!(combo(&app).is_open());

    // Stepping and Enter pick a row
    down(&mut app, Key::ArrowDown);
    down(&mut app, Key::ArrowDown);
    down(&mut app, Key::Enter);
    assert_eq!(combo(&app).selected_index(), Some(2));
    assert!(!combo(&app).is_open());

    // Escape closes without changing the selection
    up(&mut app, Key::ArrowDown);
    assert!(combo(&app).is_open());
    down(&mut app, Key::Escape);
    assert!(!combo(&app).is_open());
    assert_eq!(combo(&app).selected_index(), Some(2));

    // Type-ahead still walks the list by prefix
    up(&mut app, Key::ArrowDown);
    down(&mut app, Key::Char('B'));
    down(&mut app, Key::Enter);
    assert_eq!(combo(&app).selected_index(), Some(1));
}

#[test]
fn rust_only_a_reopened_list_steps_from_its_selected_row() {
    // C# keeps `highLightedIndex` from the last visit: here it would still
    // be 2 and Down would land on "D".
    let fired = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&fired);
    let mut app = focused(
        ComboBox::new(vec!["A", "B", "C", "D"], 0, font())
            .on_change(move |i| log.borrow_mut().push(i)),
    );
    up(&mut app, Key::ArrowDown);
    down(&mut app, Key::ArrowDown);
    down(&mut app, Key::ArrowDown);
    down(&mut app, Key::Escape);
    assert_eq!(combo(&app).selected(), 0, "Escape changed nothing");

    up(&mut app, Key::ArrowDown);
    down(&mut app, Key::ArrowDown);
    down(&mut app, Key::Enter);
    assert_eq!(combo(&app).selected(), 1);
    assert_eq!(*fired.borrow(), vec![1]);
}

#[test]
fn rust_only_one_key_is_one_step_on_every_opening() {
    // C# adds its key handlers again on every opening, so on the third
    // opening one Down moved the highlight three rows.
    let mut app = focused(ComboBox::new(vec!["A", "B", "C", "D", "E"], 0, font()));
    for _ in 0..2 {
        up(&mut app, Key::ArrowDown);
        down(&mut app, Key::Escape);
    }
    up(&mut app, Key::ArrowDown);
    down(&mut app, Key::ArrowDown);
    down(&mut app, Key::Enter);
    assert_eq!(combo(&app).selected(), 1);
}

#[test]
fn rust_only_closed_list_keys() {
    let mut app = focused(ComboBox::new(vec!["A", "B", "C"], 1, font()));

    // The press of Down alone does not open (the release does) nor select.
    down(&mut app, Key::ArrowDown);
    assert!(!combo(&app).is_open());
    assert_eq!(combo(&app).selected(), 1);

    // Up on a closed list neither opens it nor changes the selection.
    down(&mut app, Key::ArrowUp);
    up(&mut app, Key::ArrowUp);
    assert!(!combo(&app).is_open());
    assert_eq!(combo(&app).selected(), 1);

    // Enter and Space open it; they never toggle it shut or pick blindly.
    for key in [Key::Enter, Key::Char(' ')] {
        down(&mut app, key.clone());
        assert!(combo(&app).is_open(), "{key:?} opens the closed list");
        down(&mut app, Key::Escape);
        assert!(!combo(&app).is_open());
    }
    assert_eq!(combo(&app).selected(), 1);
}

#[test]
fn rust_only_enter_on_the_selected_row_does_not_fire() {
    // C#'s `SelectedIndex` setter raises SelectionChanged only on a change.
    let fired = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&fired);
    let mut app = focused(
        ComboBox::new(vec!["A", "B"], 1, font()).on_change(move |i| log.borrow_mut().push(i)),
    );
    up(&mut app, Key::ArrowDown);
    down(&mut app, Key::Enter);
    assert!(!combo(&app).is_open());
    assert!(fired.borrow().is_empty());
}

#[test]
fn rust_only_type_ahead_ignores_case_and_backspace_steps_back() {
    let mut app = focused(ComboBox::new(
        vec!["Apple", "Banana", "Blueberry", "Cherry"],
        0,
        font(),
    ));
    up(&mut app, Key::ArrowDown);
    down(&mut app, Key::Char('b'));
    down(&mut app, Key::Char('L'));
    // "bl" highlights Blueberry; Backspace back to "b" highlights Banana.
    down(&mut app, Key::Backspace);
    down(&mut app, Key::Enter);
    assert_eq!(combo(&app).selected(), 1);

    // A prefix nothing starts with leaves the highlight where it was.
    up(&mut app, Key::ArrowDown);
    down(&mut app, Key::Char('c'));
    down(&mut app, Key::Char('z'));
    down(&mut app, Key::Enter);
    assert_eq!(combo(&app).selected(), 3);
}
