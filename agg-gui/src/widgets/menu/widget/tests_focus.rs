//! Keyboard focus across a menu's life: agg-sharp
//! `PopupMenuConformanceTests.ClosingAMenuByChoosingAnItemRestoresFocusToTheAnchor`
//! (Tests/Agg.Tests/Agg.UI) through the real `App`, on the harness
//! `tests_conformance.rs` defines, plus the menu-bar case a user hits with
//! Edit > Paste.
//!
//! C# moves focus into the menu when it opens and hands it back to the
//! anchor when a row is chosen. agg-gui's menu never takes focus: the host
//! returns [`Widget::keeps_focus_on_press`], so the presses that open the
//! menu and choose a row leave focus on whatever had it. The C# assertion —
//! after choosing, focus is where it was before the menu — is the same.
//!
//! Fix pinned here: `App`'s click-to-focus rule cleared focus on every press
//! that landed on a non-focusable widget, menu hosts included, so choosing
//! a menu row unfocused the text field the row was meant to act on.

use std::cell::RefCell;
use std::rc::Rc;

use crate::event::MouseButton;
use crate::geometry::{Point, Rect};
use crate::widget::{App, Widget};
use crate::widgets::TextField;

use super::super::geometry::BAR_H;
use super::super::model::MenuItem;
use super::tests_conformance::{center, click, reset_env, test_font, Harness, Window, VIEWPORT};
use super::{MenuBar, TopMenu};

/// The harness's backdrop button: the window's first child, under the menu.
const BENEATH: &[usize] = &[0];

#[test]
fn closing_a_menu_by_choosing_an_item_restores_focus_to_the_anchor() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::closed(vec![MenuItem::action("Open", "open").into()]);

    // focus a real widget first, then show the menu over it
    click(&mut h.app, Point::new(300.0, 100.0), MouseButton::Left);
    assert_eq!(h.app.focused_path(), Some(BENEATH), "the press focused it");
    h.menu.borrow_mut().open_at_local(Point::ORIGIN);
    h.app.layout(VIEWPORT);
    assert!(h.is_open());

    let open = h.center_of_row(0);
    click(&mut h.app, open, MouseButton::Left);

    assert!(!h.is_open(), "choosing a row closes the menu");
    assert_eq!(*h.actions.borrow(), vec!["open".to_string()]);
    assert_eq!(
        h.app.focused_path(),
        Some(BENEATH),
        "focus is back where it was before the menu"
    );
}

/// A text field under a menu bar, as an app's Edit menu acts on it.
fn field_and_bar(actions: Rc<RefCell<Vec<String>>>) -> App {
    let bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new(
            "Edit",
            vec![MenuItem::action("Paste", "paste").into()],
        )],
        move |action| actions.borrow_mut().push(action.to_string()),
    );
    let bar_rect = Rect::new(0.0, VIEWPORT.height - BAR_H, VIEWPORT.width, BAR_H);
    let children: Vec<Box<dyn Widget>> = vec![Box::new(TextField::new(test_font())), Box::new(bar)];
    let mut app = App::new(Box::new(Window {
        bounds: Rect::default(),
        children,
        rects: vec![Rect::new(20.0, 200.0, 200.0, 24.0), bar_rect],
    }));
    app.layout(VIEWPORT);
    app
}

#[test]
fn rust_only_choosing_a_menu_bar_item_keeps_the_text_field_focused() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let actions = Rc::new(RefCell::new(Vec::new()));
    let mut app = field_and_bar(Rc::clone(&actions));
    let field: &[usize] = &[0];
    let open = |app: &App| app.root().children()[1].has_active_modal();

    click(
        &mut app,
        center(Rect::new(20.0, 200.0, 200.0, 24.0)),
        MouseButton::Left,
    );
    assert_eq!(app.focused_path(), Some(field), "the field took focus");

    // opening the menu from its title keeps the field focused
    let title = Point::new(8.0, VIEWPORT.height - BAR_H + 8.0);
    click(&mut app, title, MouseButton::Left);
    assert!(open(&app));
    assert_eq!(app.focused_path(), Some(field), "opening Edit keeps focus");

    // the first row sits just under the title
    click(
        &mut app,
        Point::new(30.0, VIEWPORT.height - BAR_H - 12.0),
        MouseButton::Left,
    );
    assert!(!open(&app));
    assert_eq!(*actions.borrow(), vec!["paste".to_string()]);
    assert_eq!(
        app.focused_path(),
        Some(field),
        "Paste has a focused field to paste into"
    );
}

#[test]
fn rust_only_a_press_on_a_plain_widget_still_clears_focus() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut app = field_and_bar(Rc::new(RefCell::new(Vec::new())));
    click(&mut app, Point::new(120.0, 212.0), MouseButton::Left);
    assert_eq!(app.focused_path(), Some(&[0usize][..]));

    // the window background is neither focusable nor a menu host
    click(&mut app, Point::new(400.0, 50.0), MouseButton::Left);
    assert_eq!(app.focused_path(), None, "empty space defocuses the field");
}
