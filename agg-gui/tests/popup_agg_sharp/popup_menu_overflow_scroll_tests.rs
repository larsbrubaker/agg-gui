//! agg-sharp `PopupMenuOverflowScrollTests`
//! (Tests/Agg.Tests/Agg.UI/PopupMenuOverflowScrollTests.cs): any popup menu
//! without the vertical space for its items gets a scroll bar, on every open,
//! and when its space changes after it was shown.
//!
//! C# shows the menu through a `PopupLayoutEngine` (`Direction.Down`) from a
//! 60 x 20 anchor 20 px below the top of a 400 x 300 window. agg-gui's
//! equivalent is a menu opened from a bar title: [`MenuAnchorKind::Bar`] at the
//! anchor's bottom-left with the anchor's height as its span, in root space.
//! agg-gui recomputes the layout from the item tree and window on every event
//! and frame, so a menu that grows or a window that shrinks is re-fitted by
//! the next layout; the host keeps the anchor on its title (the anchor follows
//! the top edge, as C#'s does). C#'s `ShowPopup` path is a context menu.
//! Items are 48 px rows; a scroll bar is the panel's
//! [`PopupScroll`](agg_gui::widgets::menu::PopupScroll), which agg-gui always
//! paints.
//!
//! `OptingOutOfScrollingKeepsTheMenuUnscrolled`: agg-gui menus always scroll
//! when they overflow; the opt out C# offers is for popups that are not menus
//! (colour pickers, docking tabs), which in agg-gui go through the plain
//! [`Popup`] controller, so the test pins that a tall `Popup` keeps its own
//! size.

use agg_gui::widgets::menu::geometry::PopupLayout;
use agg_gui::widgets::menu::{MenuAnchorKind, MenuEntry, MenuItem, PopupMenu};
use agg_gui::{Point, Popup, Rect, Size};

const WINDOW: Size = Size {
    width: 400.0,
    height: 300.0,
};
const ANCHOR_H: f64 = 20.0;

fn items(count: usize) -> Vec<MenuEntry> {
    (0..count)
        .map(|i| {
            MenuEntry::Item(MenuItem::action(format!("Item {i}"), format!("{i}")).row_height(48.0))
        })
        .collect()
}

/// The anchor's bottom: 20 below the top of a `window_h` tall window, 20 tall.
fn anchor_bottom(window_h: f64) -> f64 {
    window_h - 20.0 - ANCHOR_H
}

fn show_menu_popup(item_count: usize) -> PopupMenu {
    let mut menu = PopupMenu::new(items(item_count));
    open_from_anchor(&mut menu, WINDOW.height);
    menu
}

fn open_from_anchor(menu: &mut PopupMenu, window_h: f64) {
    menu.state.open_at(
        Point::new(10.0, anchor_bottom(window_h)),
        MenuAnchorKind::Bar,
    );
    menu.state.set_anchor_span(ANCHOR_H);
    menu.state.set_space_origin(Some(Point::ORIGIN));
}

fn popup(menu: &PopupMenu, window: Size) -> PopupLayout {
    menu.state.layouts(&menu.items, window).remove(0)
}

fn assert_popup_is_scrolled_inside_window(window: Size, popup: &PopupLayout) {
    let bounds = popup.rect;
    assert!(bounds.height <= window.height);
    assert!(bounds.bottom() >= 0.0);
    assert!(bounds.top() <= window.height);
    // and the items that no longer fit are reachable
    assert!(popup.scroll.is_some(), "the scroll bar is shown");
}

#[test]
fn tall_menu_from_button_near_top_scrolls() {
    let menu = show_menu_popup(16);
    assert_popup_is_scrolled_inside_window(WINDOW, &popup(&menu, WINDOW));
}

#[test]
fn tall_menu_scrolls_on_every_open_not_just_the_first() {
    let mut menu = show_menu_popup(16);
    assert_popup_is_scrolled_inside_window(WINDOW, &popup(&menu, WINDOW));
    menu.close();
    menu.items = items(16);
    open_from_anchor(&mut menu, WINDOW.height);
    assert_popup_is_scrolled_inside_window(WINDOW, &popup(&menu, WINDOW));
}

#[test]
fn menu_that_grows_after_being_shown_scrolls() {
    // A menu can be filled after the popup is up; the first layout said it fit
    let mut menu = show_menu_popup(2);
    assert!(popup(&menu, WINDOW).scroll.is_none());
    menu.items.extend(items(16));
    assert_popup_is_scrolled_inside_window(WINDOW, &popup(&menu, WINDOW));
}

#[test]
fn menu_reclamps_when_the_window_shrinks_under_it() {
    let mut menu = show_menu_popup(4);
    assert!(popup(&menu, WINDOW).scroll.is_none());
    let small = Size::new(WINDOW.width, 120.0);
    // The anchor follows the top edge as the window shrinks
    menu.state.anchor = Point::new(10.0, anchor_bottom(small.height));
    assert_popup_is_scrolled_inside_window(small, &popup(&menu, small));
}

#[test]
fn menu_shown_through_show_popup_scrolls() {
    let mut menu = PopupMenu::new(items(16));
    menu.open_at(Point::new(10.0, anchor_bottom(WINDOW.height)));
    let bounds = popup(&menu, WINDOW);
    assert!(bounds.rect.height <= WINDOW.height);
    assert!(bounds.rect.bottom() >= 0.0);
    assert!(bounds.rect.top() <= WINDOW.height);
    assert!(bounds.scroll.is_some());
}

#[test]
fn short_menu_gains_no_scroll_bar() {
    let menu = show_menu_popup(2);
    let popup = popup(&menu, WINDOW);
    assert!(popup.scroll.is_none());
    assert!(popup.rect.bottom() >= 0.0);
    assert!(popup.rect.top() <= WINDOW.height);
}

#[test]
fn opting_out_of_scrolling_keeps_the_menu_unscrolled() {
    // A plain popup sized for 16 48 px rows keeps its own height
    let size = Size::new(150.0, 16.0 * 48.0);
    let mut popup = Popup::new();
    popup.set_anchor(Rect::new(
        10.0,
        anchor_bottom(WINDOW.height),
        60.0,
        ANCHOR_H,
    ));
    popup.set_size(size);
    popup.open();
    assert!((popup.rect(WINDOW).height - size.height).abs() < 0.001);
}
