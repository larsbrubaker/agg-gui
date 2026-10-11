//! agg-sharp `NarrowSubMenuPlacementTests`
//! (Tests/Agg.Tests/Agg.UI/NarrowSubMenuPlacementTests.cs), against the menu
//! system's cascade placement ([`stack_layout_with_width`] in
//! `widgets/menu/geometry.rs`, the one layout paint and hit-testing share).
//!
//! C# opens View, then its Color row, in a `MenuBarWidget` docked at the top
//! of the window and reads the screen bounds of the row and the sub menu. Here
//! the same View menu is laid out with View open at the bar's bottom edge and
//! the Color row on the open path; the menus are 250 wide, wider than half of
//! the 411 px window, as C#'s long labels make them.
//!
//! On a phone-width window the sub menu fits neither to the right of its
//! parent nor to the left. It used to be clamped back on screen right over
//! the parent, covering the row the user had just tapped; it now hangs below
//! that row.
//!
//! CONVERTED: agg-gui rows span the whole panel and a cascade overlaps its
//! parent's right edge by 2 px (`tests_fit.rs` pins it), where C#'s rows are
//! inset from the panel so its sub menu starts at or past the row's right
//! edge. The wide-window test pins agg-gui's cascade position (row right
//! − 2) instead of `>= row right`.

use agg_gui::widgets::menu::geometry::{stack_layout_with_width, PopupLayout};
use agg_gui::widgets::menu::{MenuAnchorKind, MenuEntry, MenuItem, MenuMetrics};
use agg_gui::{Point, Rect, Size};

const MENU_W: f64 = 250.0;
const BAR_H: f64 = 24.0;
/// Index of the Color row in the View menu.
const COLOR: usize = 1;

fn view_menu() -> Vec<MenuEntry> {
    vec![
        MenuEntry::Item(MenuItem::action("Show the long descriptive panel", "panel")),
        MenuEntry::Item(MenuItem::submenu(
            "Color",
            vec![
                MenuEntry::Item(MenuItem::action("A rather long colour choice name", "a")),
                MenuEntry::Item(MenuItem::action("Another long colour choice name", "b")),
            ],
        )),
        MenuEntry::Item(MenuItem::action("Rows under the opening row", "rows")),
    ]
}

/// Lays out View (opened from a bar at the top of a `width` x `height`
/// window) with its Color sub menu open: `(window, parent panel, Color row,
/// sub menu)`.
fn open_color_sub_menu(width: f64, height: f64) -> (Size, Rect, Rect, Rect) {
    let window = Size::new(width, height);
    let layouts: Vec<PopupLayout> = stack_layout_with_width(
        &view_menu(),
        Point::new(0.0, height - BAR_H),
        MenuAnchorKind::Bar,
        &[COLOR],
        window,
        &|_, _: &MenuMetrics| MENU_W,
    );
    assert_eq!(layouts.len(), 2, "View and its Color sub menu are open");
    let row = layouts[0]
        .rows
        .iter()
        .find(|r| r.item_index == Some(COLOR))
        .expect("the Color row")
        .rect;
    (window, layouts[0].rect, row, layouts[1].rect)
}

#[test]
fn a_sub_menu_that_fits_neither_side_opens_below_its_row_on_a_narrow_window() {
    // Lars's phone: about 411 CSS pixels across
    let (window, parent, row, sub) = open_color_sub_menu(411.0, 800.0);

    // The fixture really is the case under test: neither side of the parent
    // has room
    assert!(parent.x + parent.width + sub.width > window.width);
    assert!(parent.x - sub.width < 0.0);

    let overlaps_row = sub.x < row.x + row.width
        && sub.x + sub.width > row.x
        && sub.y < row.y + row.height
        && sub.y + sub.height > row.y;
    assert!(
        !overlaps_row,
        "the sub menu must not cover the row that opened it"
    );
    assert!(
        sub.y + sub.height <= row.y,
        "with room below the row, the sub menu hangs from it"
    );
    assert!(sub.x >= 0.0);
    assert!(sub.x + sub.width <= window.width);
    assert!(sub.y >= 0.0);
    assert!(sub.y + sub.height <= window.height);
}

#[test]
fn a_sub_menu_on_a_wide_window_still_opens_to_the_right_of_its_row() {
    let (_window, _parent, row, sub) = open_color_sub_menu(1200.0, 800.0);
    assert_eq!(
        sub.x,
        row.x + row.width - 2.0,
        "a sub menu with room to the right opens there"
    );
    assert_eq!(
        sub.y + sub.height,
        row.y + row.height,
        "its top lines up with the row that opened it"
    );
}
