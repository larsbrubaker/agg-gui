//! agg-sharp `PopupSubMenuScrollTests`
//! (Tests/Agg.Tests/Agg.UI/PopupSubMenuScrollTests.cs), against the menu
//! layout paint and hit testing share ([`PopupMenuState::layouts`],
//! `widgets/menu/geometry.rs` + `overflow.rs`).
//!
//! C#'s 48 px tall items are `MenuItem::row_height(48.0)` rows; C# checks for a
//! `ScrollableWidget` descendant, agg-gui for the panel's
//! [`PopupScroll`](agg_gui::widgets::menu::PopupScroll). C#'s `ShowMenu(anchor,
//! (10, 10))` is a context menu opened at `(10, 10)`.
//!
//! CONVERTED: agg-gui keeps popups 4 px (`MARGIN`) clear of the window edge
//! where C# sits flush, so a popup taller than the window shows its top at
//! `height − 4` rather than `height`.

use agg_gui::widgets::menu::geometry::PopupLayout;
use agg_gui::widgets::menu::{MenuEntry, MenuItem, PopupMenu};
use agg_gui::{Event, MouseButton, Point, Size};

const WINDOW: Size = Size {
    width: 400.0,
    height: 300.0,
};
const MARGIN: f64 = 4.0;

fn tall_items(label: &str, count: usize) -> Vec<MenuEntry> {
    (0..count)
        .map(|i| MenuItem::action(format!("{label} {i}"), format!("{i}")).row_height(48.0))
        .map(MenuEntry::Item)
        .collect()
}

/// A menu whose only row opens `submenu`, opened near the top left with that
/// row clicked open. Returns its layouts.
fn open_sub_menu(submenu: Vec<MenuEntry>) -> Vec<PopupLayout> {
    let mut menu = PopupMenu::new(vec![MenuEntry::Item(MenuItem::submenu(
        "Open Recent",
        submenu,
    ))]);
    menu.open_at(Point::new(10.0, 290.0));
    let row = menu.state.layouts(&menu.items, WINDOW)[0].rows[0].rect;
    let at = Point::new(row.x + 10.0, row.y + row.height * 0.5);
    menu.handle_event(
        &Event::MouseDown {
            pos: at,
            button: MouseButton::Left,
            modifiers: Default::default(),
        },
        WINDOW,
    );
    let layouts = menu.state.layouts(&menu.items, WINDOW);
    assert_eq!(layouts.len(), 2, "the click opened the sub menu");
    layouts
}

/// The panel is inside the window, scrolls, and starts at its first row.
fn assert_clamped_scrolling_at_first_row(layout: &PopupLayout) {
    let bounds = layout.rect;
    // The menu content is ~960 tall - it must be shrunk to fit the 300 tall window
    assert!(bounds.height <= WINDOW.height);
    // and it must be positioned inside the window, not hanging off the top or bottom
    assert!(bounds.top() <= WINDOW.height);
    assert!(bounds.bottom() >= 0.0);
    // The items that no longer fit are reached by scrolling
    assert!(layout.scroll.is_some());
    // It starts at its beginning, not scrolled (or positioned) to its end
    let first = layout.rows[0].rect;
    assert!(first.bottom() < WINDOW.height);
    assert!(first.top() > 0.0);
    assert_eq!(first.top(), bounds.top(), "the first row is the one on top");
}

#[test]
fn tall_sub_menu_is_clamped_to_window_and_scrollable() {
    let layouts = open_sub_menu(tall_items("Recent", 20));
    assert_clamped_scrolling_at_first_row(&layouts[1]);
}

#[test]
fn short_sub_menu_is_unchanged() {
    let submenu = vec![
        MenuEntry::Item(MenuItem::action("Scale", "scale")),
        MenuEntry::Item(MenuItem::action("Rotate", "rotate")),
    ];
    let layouts = open_sub_menu(submenu);
    let sub = &layouts[1];
    // A menu that fits keeps its natural size and gains no scrolling machinery
    assert!(sub.rect.height < WINDOW.height);
    assert!(sub.scroll.is_none());
    assert_eq!(sub.rows.len(), 2);
}

#[test]
fn tall_top_level_menu_is_clamped_to_window_and_scrollable() {
    let mut menu = PopupMenu::new(tall_items("Recent", 20));
    // This is the path every right click menu takes
    menu.open_at(Point::new(10.0, 10.0));
    let layouts = menu.state.layouts(&menu.items, WINDOW);
    assert_clamped_scrolling_at_first_row(&layouts[0]);
}

#[test]
fn popup_taller_than_window_shows_its_top() {
    // A popup that is taller than the window cannot be fully clamped, so the
    // clamp has to choose an end to sacrifice. Showing the top is the only
    // useful choice.
    let mut menu = PopupMenu::new(tall_items("Item", 20));
    menu.open_at(Point::new(10.0, 20.0));
    let bounds = menu.state.layouts(&menu.items, WINDOW)[0].rect;
    assert_eq!(bounds.top(), WINDOW.height - MARGIN);
}
