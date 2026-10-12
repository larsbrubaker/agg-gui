//! agg-sharp `MenuModelPopupBuilderTests` (Tests/Agg.Tests/Agg.UI/
//! MenuModelPopupBuilderTests.cs), the tests that pin what agg-gui's menu
//! model builds: C#'s `MenuItemModel` list is a `Vec<MenuEntry>` of
//! [`MenuItem`]s and the `PopupMenu` built from it is [`PopupMenu::new`].
//! Each test runs through the real `App` and the harness of
//! `tests_conformance.rs`; a row is its index, its click point the centre of
//! its laid-out rect.
//!
//! Not here, with the reason:
//! - `InvisibleItemsAreNotBuilt`, the `IsVisible` half of
//!   `SubMenuItemsRenderRecursivelyWhenOpened`, and
//!   `PopupSubMenuOverrideWinsOverSubMenuItems`: agg-gui's model has no
//!   visibility gate and no in-app submenu override; both belong to the
//!   builder MatterCAD ports over its own `NamedAction` model
//!   (mattercad-app `custom_widgets/menu_items.rs`).
//! - `ItemsRenderInOrderWithPopupNamingConvention` and
//!   `SubMenuButtonTakesAutomationNameAndEnabledGate`: C# automation names.
//! - Duplicates: `ShortcutTextIsShownBesideTheItem` (`menu/shortcut_text.rs`),
//!   `ToolTipTextReachesTheWidget` (`tests_tooltip.rs`),
//!   `CheckedItemsBuildCheckAndRadioWidgets` (`tests_selection_column.rs`),
//!   `IconGlyphIsDrawnIntoTheIconSlot` (`tests_item_slot.rs`).
//!
//! C#'s check row stays open unless `CloseMenuOnPick` is set; an agg-gui row
//! closes unless it is [`MenuItem::keep_open`], so
//! `check_row_closes_the_menu_only_when_asked_to` states both choices.

use crate::event::MouseButton;
use crate::geometry::Point;

use super::super::model::{MenuEntry, MenuItem};
use super::tests_conformance::{center, click, reset_env, Harness, VIEWPORT};

/// The rows of open panel `level`: `Some(index)` for an item, `None` for a
/// separator.
fn rows(h: &Harness, level: usize) -> Vec<Option<usize>> {
    let menu = h.menu.borrow();
    menu.state.layouts(&menu.items, VIEWPORT)[level]
        .rows
        .iter()
        .map(|r| r.item_index)
        .collect()
}

/// Centre of item `row` of open panel `level`, in window space.
fn center_of(h: &Harness, level: usize, row: usize) -> Point {
    let menu = h.menu.borrow();
    let layouts = menu.state.layouts(&menu.items, VIEWPORT);
    let rect = layouts[level]
        .rows
        .iter()
        .find(|r| r.item_index == Some(row))
        .map(|r| r.rect)
        .expect("row is laid out");
    center(rect)
}

fn item(h: &Harness, row: usize) -> MenuItem {
    match &h.menu.borrow().items[row] {
        MenuEntry::Item(item) => item.clone(),
        MenuEntry::Separator => panic!("row {row} is a separator"),
    }
}

#[test]
fn enabled_gate_reaches_the_widget() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(vec![
        MenuItem::action("Undo", "undo").disabled().into(),
        MenuItem::action("Redo", "redo").into(),
    ]);

    assert!(!item(&h, 0).enabled);
    // No `disabled()` at all means enabled - it must not be read as "false"
    assert!(item(&h, 1).enabled);

    let undo = h.center_of_row(0);
    click(&mut h.app, undo, MouseButton::Left);
    assert!(h.actions.borrow().is_empty());
    let redo = h.center_of_row(1);
    click(&mut h.app, redo, MouseButton::Left);
    assert_eq!(*h.actions.borrow(), vec!["redo".to_string()]);
}

#[test]
fn separator_builds_a_separator_widget() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let h = Harness::show(vec![
        MenuItem::action("Settings", "settings").into(),
        MenuEntry::Separator,
        MenuItem::action("Quit", "quit").into(),
    ]);

    assert_eq!(rows(&h, 0), vec![Some(0), None, Some(2)]);
    let menu = h.menu.borrow();
    let layout = &menu.state.layouts(&menu.items, VIEWPORT)[0];
    assert!(
        layout.rows[1].rect.height < layout.rows[0].rect.height,
        "the separator is a divider line, not a row"
    );
}

#[test]
fn sub_menu_items_render_recursively_when_opened() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(vec![MenuItem::submenu(
        "Open Recent",
        vec![
            MenuItem::action("First.mcx", "first").into(),
            MenuEntry::Separator,
            MenuItem::action("Second.mcx", "second").disabled().into(),
        ],
    )
    .into()]);

    let open_recent = h.center_of_row(0);
    click(&mut h.app, open_recent, MouseButton::Left);
    assert_eq!(h.panels().len(), 2, "the sub menu is up");

    // The children of a submenu get the same separators and gates the top
    // level does
    assert_eq!(rows(&h, 1), vec![Some(0), None, Some(2)]);
    let second = center_of(&h, 1, 2);
    click(&mut h.app, second, MouseButton::Left);
    assert!(
        h.actions.borrow().is_empty(),
        "the disabled leaf fires nothing"
    );
    assert!(h.is_open());
}

#[test]
fn clicking_a_built_item_runs_the_model_action() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut no_action = MenuItem::action("No Action", "");
    no_action.action = None;
    let mut h = Harness::show(vec![
        MenuItem::action("Do It", "do").into(),
        no_action.into(),
    ]);

    // An item with no action must click harmlessly rather than throwing
    let none = h.center_of_row(1);
    click(&mut h.app, none, MouseButton::Left);
    assert!(h.actions.borrow().is_empty());

    let do_it = h.center_of_row(0);
    click(&mut h.app, do_it, MouseButton::Left);
    assert_eq!(*h.actions.borrow(), vec!["do".to_string()]);
}

#[test]
fn check_row_closes_the_menu_only_when_asked_to() {
    let _guard = crate::input_profile::profile_test_lock();
    reset_env();
    let mut h = Harness::show(vec![
        MenuItem::action("Stays", "stays")
            .checked(false)
            .keep_open()
            .into(),
        MenuItem::action("Closes", "closes")
            .checked(false)
            .close_on_activate(true)
            .into(),
    ]);

    // A keep-open check row: the menu stays up for the next pick
    let stays = h.center_of_row(0);
    click(&mut h.app, stays, MouseButton::Left);
    assert!(h.is_open());

    let closes = h.center_of_row(1);
    click(&mut h.app, closes, MouseButton::Left);
    assert!(!h.is_open());
    assert_eq!(h.actions.borrow().len(), 2);
}
