//! Rust-only tests of [`super::stack_layout_with_width`] on a phone-width
//! window, for the placements `widget/tests_touch_narrow.rs` (a top
//! `MenuBar`) does not reach:
//!
//! - a submenu that fits beside its parent on neither side hangs from its
//!   row (`overflow.rs` `place_hanging`) for a context menu and for a menu
//!   opened upward from a bottom bar too, flipping to the other side of the
//!   row rather than sliding over it, so the row that closes it stays
//!   tappable;
//! - a panel is capped at the window's width less its margins only when the
//!   window is known: with a zero (not yet known) viewport it keeps its
//!   natural width.

use super::*;
use crate::geometry::{Point, Rect, Size};
use crate::widgets::menu::model::{MenuEntry, MenuItem};
use crate::widgets::menu::MenuAnchorKind;

/// Wider than half the window, so a submenu fits on neither side.
const MENU_W: f64 = 250.0;
const WINDOW: Size = Size {
    width: 300.0,
    height: 400.0,
};
/// The row with the submenu: the last of five, so a menu anchored near the
/// window's bottom puts it near the bottom too.
const OPENER: usize = 4;

fn menu() -> Vec<MenuEntry> {
    let mut items: Vec<MenuEntry> = (0..OPENER)
        .map(|i| MenuItem::action(format!("Row {i}"), format!("r{i}")).into())
        .collect();
    let children = (0..6)
        .map(|i| MenuItem::action(format!("Child {i}"), format!("c{i}")).into())
        .collect();
    items.push(MenuItem::submenu("More", children).into());
    items
}

fn desktop() {
    crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
    crate::touch_state::clear_last_touch_event_for_testing();
    crate::ux_scale::set_ux_scale(1.0);
}

/// `(window, opening row, submenu)` with More open from `anchor`.
fn open_more(anchor: Point, kind: MenuAnchorKind) -> (Rect, Rect, Rect) {
    let layouts = stack_layout_with_width(&menu(), anchor, kind, &[OPENER], WINDOW, &|_, _| MENU_W);
    assert_eq!(layouts.len(), 2, "the submenu is open");
    let row = layouts[0]
        .rows
        .iter()
        .find(|r| r.item_index == Some(OPENER))
        .expect("the More row")
        .rect;
    let window = Rect::new(0.0, 0.0, WINDOW.width, WINDOW.height);
    (window, row, layouts[1].rect)
}

fn assert_hangs_clear_of(window: Rect, row: Rect, sub: Rect, what: &str) {
    let eps = 1e-6;
    let overlaps = sub.y < row.y + row.height - eps && sub.y + sub.height > row.y + eps;
    assert!(
        !overlaps,
        "{what}: submenu {sub:?} must not cover its row {row:?}"
    );
    assert!(
        sub.y >= window.y - eps && sub.y + sub.height <= window.y + window.height + eps,
        "{what}: submenu {sub:?} stays in the window"
    );
}

#[test]
fn a_context_submenu_with_no_room_below_its_row_hangs_above_it() {
    let _guard = crate::input_profile::profile_test_lock();
    desktop();
    // Opened near the window's bottom-left: the menu is clamped up, and
    // More's row ends a little above the window's bottom.
    let (window, row, sub) = open_more(Point::new(10.0, 60.0), MenuAnchorKind::Context);
    let below = row.y - MARGIN;
    assert!(
        sub.height > below,
        "the fixture leaves too little room below the row ({below} for {})",
        sub.height
    );
    assert_hangs_clear_of(window, row, sub, "context menu");
    assert_eq!(
        sub.y,
        row.y + row.height,
        "the submenu stands on the row's top"
    );
}

#[test]
fn a_context_submenu_with_room_below_its_row_hangs_from_it() {
    let _guard = crate::input_profile::profile_test_lock();
    desktop();
    let (window, row, sub) = open_more(Point::new(10.0, 390.0), MenuAnchorKind::Context);
    assert_hangs_clear_of(window, row, sub, "context menu");
    assert_eq!(sub.y + sub.height, row.y, "the submenu hangs from the row");
}

#[test]
fn a_bottom_bar_submenu_opens_above_its_row_or_below_when_there_is_no_room() {
    let _guard = crate::input_profile::profile_test_lock();
    desktop();
    // A bar along the window's bottom: the menu opens upward from its top
    // edge, its rows top to bottom, so More (the last row) sits on the bar.
    let (window, row, sub) = open_more(Point::new(10.0, 30.0), MenuAnchorKind::BottomBar);
    assert_hangs_clear_of(window, row, sub, "bottom bar, room above");
    assert_eq!(sub.y, row.y + row.height, "upward: it stands on the row");

    // The bar high enough that the menu reaches near the window's top and
    // More's row has too little room above it: the submenu flips below.
    let (window, row, sub) = open_more(Point::new(10.0, 270.0), MenuAnchorKind::BottomBar);
    let above = WINDOW.height - MARGIN - (row.y + row.height);
    assert!(
        sub.height > above,
        "the fixture leaves no room above the row"
    );
    assert_hangs_clear_of(window, row, sub, "bottom bar, no room above");
    assert_eq!(sub.y + sub.height, row.y, "flipped: it hangs from the row");
}

#[test]
fn an_unknown_window_does_not_cap_a_panels_width() {
    let _guard = crate::input_profile::profile_test_lock();
    desktop();
    let layouts = stack_layout_with_width(
        &menu(),
        Point::new(10.0, 300.0),
        MenuAnchorKind::Context,
        &[],
        Size::new(0.0, 0.0),
        &|_, _| MENU_W,
    );
    assert_eq!(
        layouts[0].rows[0].rect.width, MENU_W,
        "with no window known the rows keep their natural width"
    );
}
