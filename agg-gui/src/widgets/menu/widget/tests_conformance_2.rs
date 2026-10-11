//! agg-sharp `PopupMenuConformanceTests` (Tests/Agg.Tests/Agg.UI), the tests
//! not in `tests_conformance.rs`: disabled, check and radio rows, submenus
//! opened by click, the bottom-edge clamp and the non-left presses, through
//! the real `App` and the harness that file defines.
//!
//! C# names rows by automation name (`"Open Menu Item"`); here a row is its
//! index, its click point the centre of its laid-out rect, as C# derives it
//! from the row's real bounds.
//!
//! Fix pinned here: re-choosing the selected radio row fires no action (the
//! C# "no setter call"), which `PopupMenuState::activate_action` used to do.

use crate::event::{Key, Modifiers, MouseButton};
use crate::geometry::{Point, Rect};
use crate::widget::App;

use super::super::geometry::BAR_H;
use super::super::model::{MenuEntry, MenuItem, MenuSelection};
use super::tests_conformance::{
    assert_got_nothing, center, click, counting_button, open_close_items, reset_env, test_font,
    Harness, Window, VIEWPORT,
};
use super::{MenuBar, TopMenu};

fn selection(h: &Harness, row: usize) -> MenuSelection {
    match &h.menu.borrow().items[row] {
        MenuEntry::Item(item) => item.selection,
        MenuEntry::Separator => MenuSelection::None,
    }
}

fn more_with(leaves: Vec<MenuEntry>) -> MenuEntry {
    MenuItem::submenu("More", leaves).into()
}

/// Centre of the first row of the open submenu (panel 1), in window space.
fn center_of_sub_row(h: &Harness, row: usize) -> Point {
    let menu = h.menu.borrow();
    let layouts = menu.state.layouts(&menu.items, VIEWPORT);
    let rect = layouts[1]
        .rows
        .iter()
        .find(|r| r.item_index == Some(row))
        .map(|r| r.rect)
        .expect("submenu row is laid out");
    center(rect)
}

fn press(app: &mut App, p: Point, button: MouseButton) {
    app.on_mouse_down(p.x, VIEWPORT.height - p.y, button, Modifiers::default());
}

#[test]
fn disabled_rows_do_not_fire_actions_and_leave_the_menu_open() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(vec![
        MenuItem::action("Disabled", "disabled").disabled().into(),
        MenuItem::action("Open", "open").into(),
    ]);

    // prove the press really lands on the disabled row
    let disabled = super::tests_conformance::row_rect(&h.menu.borrow(), 0);
    assert!(disabled.height > 0.0);
    assert!(h.menu_contains(center(disabled)));

    click(&mut h.app, center(disabled), MouseButton::Left);

    assert!(
        h.actions.borrow().is_empty(),
        "a disabled row fires nothing"
    );
    assert!(h.is_open(), "the menu stays up for the next attempt");
}

#[test]
fn checkbox_menu_item_toggles_and_keeps_the_menu_open() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(vec![
        MenuItem::action("Check", "check")
            .checked(false)
            .keep_open()
            .into(),
        MenuItem::action("Open", "open").into(),
    ]);

    let row = h.center_of_row(0);
    click(&mut h.app, row, MouseButton::Left);
    assert_eq!(*h.actions.borrow(), vec!["check".to_string()]);
    assert_eq!(selection(&h, 0), MenuSelection::Check { selected: true });
    assert!(h.is_open());

    // a second press toggles back off
    click(&mut h.app, row, MouseButton::Left);
    assert_eq!(h.actions.borrow().len(), 2);
    assert_eq!(selection(&h, 0), MenuSelection::Check { selected: false });
    assert!(h.is_open());
}

#[test]
fn radio_menu_items_are_exclusive_and_keep_the_menu_open() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(vec![
        MenuItem::action("Radio A", "A")
            .radio(true)
            .keep_open()
            .into(),
        MenuItem::action("Radio B", "B")
            .radio(false)
            .keep_open()
            .into(),
    ]);
    assert_eq!(selection(&h, 0), MenuSelection::Radio { selected: true });
    assert_eq!(selection(&h, 1), MenuSelection::Radio { selected: false });

    let b = h.center_of_row(1);
    click(&mut h.app, b, MouseButton::Left);
    assert_eq!(selection(&h, 1), MenuSelection::Radio { selected: true });
    assert_eq!(
        selection(&h, 0),
        MenuSelection::Radio { selected: false },
        "checking one radio item unchecks its siblings"
    );
    assert_eq!(*h.actions.borrow(), vec!["B".to_string()]);
    assert!(h.is_open());

    // re-clicking the already checked row does nothing at all
    click(&mut h.app, b, MouseButton::Left);
    assert_eq!(selection(&h, 1), MenuSelection::Radio { selected: true });
    assert_eq!(selection(&h, 0), MenuSelection::Radio { selected: false });
    assert_eq!(*h.actions.borrow(), vec!["B".to_string()]);
    assert!(h.is_open());
}

/// The fix behind the last half of the radio test, for a radio row that
/// closes its menu: re-choosing it closes the menu but reports no action.
#[test]
fn rust_only_reselecting_a_closing_radio_row_closes_without_an_action() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(vec![
        MenuItem::action("Radio A", "A").radio(true).into(),
        MenuItem::action("Radio B", "B").radio(false).into(),
    ]);
    let a = h.center_of_row(0);
    click(&mut h.app, a, MouseButton::Left);
    assert!(!h.is_open(), "the row still closes its menu");
    assert!(
        h.actions.borrow().is_empty(),
        "no action for the same choice"
    );
    assert_eq!(selection(&h, 0), MenuSelection::Radio { selected: true });
}

#[test]
fn clicking_a_sub_menu_parent_opens_it_and_its_rows_are_hit_testable() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(vec![
        MenuItem::action("Open", "open").into(),
        more_with(vec![
            MenuItem::action("Leaf", "leaf").into(),
            MenuItem::action("Checked", "checked").into(),
        ]),
    ]);

    let more = h.center_of_row(1);
    click(&mut h.app, more, MouseButton::Left);
    assert_eq!(h.panels().len(), 2, "the submenu is up");
    assert!(h.is_open(), "the parent is held open with its submenu");

    let leaf = center_of_sub_row(&h, 0);
    assert!(h.menu_contains(leaf));
    click(&mut h.app, leaf, MouseButton::Left);

    assert_eq!(*h.actions.borrow(), vec!["leaf".to_string()]);
    assert!(
        !h.is_open(),
        "choosing a leaf tears down the whole menu stack, not just the sub menu"
    );
}

/// C# takes focus off the submenu; agg-gui menus hold no focus, and the
/// user's way out of only the submenu is Left (agg-sharp `OnKeyDown`).
#[test]
fn closing_a_sub_menu_without_a_selection_leaves_no_sub_menu() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(vec![more_with(vec![
        MenuItem::action("Leaf", "leaf").into()
    ])]);
    let more = h.center_of_row(0);
    click(&mut h.app, more, MouseButton::Left);
    assert_eq!(h.panels().len(), 2);

    h.app.on_key_down(Key::ArrowLeft, Modifiers::default());

    assert_eq!(h.panels().len(), 1, "the submenu closed");
    assert!(h.menu.borrow().state.open_path.is_empty());
    assert!(h.is_open(), "the parent stays");
    // the next click opens a fresh one
    click(&mut h.app, more, MouseButton::Left);
    assert_eq!(h.panels().len(), 2);
}

#[test]
fn menu_opened_at_the_bottom_edge_stays_inside_the_window() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let h = Harness::show_at(
        vec![
            MenuItem::action("One", "1").into(),
            MenuItem::action("Two", "2").into(),
            MenuItem::action("Three", "3").into(),
        ],
        Point::new(10.0, 2.0),
    );
    let menu = h.menu.borrow();
    let layouts = menu.state.layouts(&menu.items, VIEWPORT);
    let panel = layouts[0].rect;
    assert!(panel.y >= 0.0);
    assert!(panel.y + panel.height <= VIEWPORT.height);
    // every row is reachable, not merely the panel rectangle
    for row in &layouts[0].rows {
        assert!(row.rect.y >= 0.0);
        assert!(row.rect.y + row.rect.height <= VIEWPORT.height);
    }
}

#[test]
fn right_click_is_ignored_by_the_menu_machinery_when_no_menu_is_open() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::closed(open_close_items());
    click(&mut h.app, Point::new(50.0, 50.0), MouseButton::Right);
    assert_eq!(
        h.beneath.mouse_downs.get(),
        1,
        "the press reaches the target"
    );
    assert!(!h.is_open(), "and opens no menu");
}

#[test]
fn right_press_dismisses_the_menu_without_activating_the_row_under_it() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(open_close_items());
    let row = h.center_of_row(0);
    click(&mut h.app, row, MouseButton::Right);
    assert!(!h.is_open());
    assert!(
        h.actions.borrow().is_empty(),
        "the press only dismisses - it must not also choose the row it landed on"
    );
    assert_got_nothing(&h.beneath, "the widget beneath the menu");
}

#[test]
fn middle_press_dismisses_the_menu() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(open_close_items());
    let row = h.center_of_row(0);
    press(&mut h.app, row, MouseButton::Middle);
    assert!(!h.is_open());
}

/// C#'s `PopupWidget`-hosted menu is, in agg-gui, a menu opened from a host
/// widget's title: the menu bar's.
#[test]
fn right_press_dismisses_a_popup_widget_hosted_menu() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let bar = MenuBar::new(
        test_font(),
        vec![TopMenu::new("File", open_close_items())],
        |_| {},
    );
    let bar_rect = Rect::new(0.0, VIEWPORT.height - BAR_H, VIEWPORT.width, BAR_H);
    let (beneath_button, beneath, _observer) = counting_button("Beneath");
    let mut app = App::new(Box::new(Window {
        bounds: Rect::default(),
        children: vec![beneath_button, Box::new(bar)],
        rects: vec![
            Rect::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height),
            bar_rect,
        ],
    }));
    app.layout(VIEWPORT);
    let open = |app: &App| app.root().children()[1].has_active_modal();
    click(
        &mut app,
        Point::new(8.0, bar_rect.y + 8.0),
        MouseButton::Left,
    );
    assert!(open(&app));

    // the first row sits just under the title
    let row = Point::new(30.0, bar_rect.y - 12.0);
    press(&mut app, row, MouseButton::Right);

    assert!(
        !open(&app),
        "a right press over a row closes the hosted menu"
    );
    assert_got_nothing(&beneath, "the widget beneath");
}

#[test]
fn right_press_over_a_sub_menu_row_closes_the_whole_chain() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(vec![more_with(vec![
        MenuItem::action("Leaf", "leaf").into()
    ])]);
    let more = h.center_of_row(0);
    click(&mut h.app, more, MouseButton::Left);
    assert_eq!(h.panels().len(), 2);

    let leaf = center_of_sub_row(&h, 0);
    click(&mut h.app, leaf, MouseButton::Right);

    assert!(h.actions.borrow().is_empty());
    assert!(
        !h.is_open(),
        "a dismissing press drops the whole open chain, not just its deepest level"
    );
}
