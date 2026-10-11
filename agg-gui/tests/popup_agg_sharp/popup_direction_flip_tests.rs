//! agg-sharp `PopupDirectionFlipTests`
//! (Tests/Agg.Tests/Agg.UI/PopupDirectionFlipTests.cs): use the preferred
//! direction if it fits, otherwise the opposite one if it fits, otherwise the
//! roomier side with a scroll bar.
//!
//! C# opens a `DropDownList` through `PopupLayoutEngine`. agg-gui's combo box
//! has fixed-height rows and no preferred direction (it already opens
//! downward unless the room above is larger and the list does not fit below,
//! the same rule for a downward preference), so the rule is ported against
//! the menu, which runs `PopupLayoutEngine`'s logic for a menu opened from a
//! bar title: [`MenuAnchorKind::Bar`] prefers down (C# `Direction.Down`),
//! [`MenuAnchorKind::BottomBar`] up (`Direction.Up`). The anchor is a 26 px
//! tall title whose bottom is C#'s `anchorBottom`; items are 50 px rows.
//!
//! CONVERTED: C# subtracts 5 px from the room of a list that scrolls; agg-gui
//! keeps every popup 4 px (`MARGIN`) clear of the window edge, so a scrolling
//! list opening downward is `anchor bottom − 4` tall instead of `− 5`.

use agg_gui::widgets::menu::geometry::PopupLayout;
use agg_gui::widgets::menu::{MenuAnchorKind, MenuEntry, MenuItem, PopupMenuState};
use agg_gui::{Point, Size};

const WINDOW: Size = Size {
    width: 400.0,
    height: 300.0,
};
const ANCHOR_H: f64 = 26.0;
const MARGIN: f64 = 4.0;

/// Opens `item_count` 50 px rows from a title whose bottom is
/// `anchor_bottom`, preferring `kind`'s direction. Returns the panel layout
/// and the content height.
fn open_drop_down(
    item_count: usize,
    anchor_bottom: f64,
    kind: MenuAnchorKind,
) -> (PopupLayout, f64) {
    let items: Vec<MenuEntry> = (0..item_count)
        .map(|i| {
            MenuEntry::Item(MenuItem::action(format!("Item {i}"), format!("{i}")).row_height(50.0))
        })
        .collect();
    let anchor_y = match kind {
        MenuAnchorKind::BottomBar => anchor_bottom + ANCHOR_H,
        _ => anchor_bottom,
    };
    let mut state = PopupMenuState::default();
    state.open_at(Point::new(10.0, anchor_y), kind);
    state.set_anchor_span(ANCHOR_H);
    state.set_space_origin(Some(Point::ORIGIN));
    let layout = state.layouts(&items, WINDOW).remove(0);
    (layout, item_count as f64 * 50.0)
}

#[test]
fn fits_above_but_not_below_opens_up_without_scroll() {
    let (popup, content) = open_drop_down(2, 60.0, MenuAnchorKind::Bar);
    // It did not fit below (60 of space for a 100 tall list) but fits easily above
    assert!((popup.rect.height - content).abs() < 0.001);
    assert!((popup.rect.y - (60.0 + ANCHOR_H)).abs() < 0.001);
    assert!(popup.scroll.is_none());
}

#[test]
fn fits_below_opens_down() {
    let (popup, content) = open_drop_down(2, 200.0, MenuAnchorKind::Bar);
    assert!((popup.rect.height - content).abs() < 0.001);
    // Opening down puts the top of the popup at the bottom of the anchor
    assert!((popup.rect.top() - 200.0).abs() < 0.001);
}

#[test]
fn fits_neither_side_opens_toward_the_larger_side_with_scroll() {
    // 5 x 50 = 250 tall in a 300 tall window - 136 below the anchor, 130 above it
    let (popup, content) = open_drop_down(5, 140.0, MenuAnchorKind::Bar);
    assert!(popup.rect.height < content);
    assert!((popup.rect.height - (140.0 - MARGIN)).abs() < 0.001);
    assert!(popup.scroll.is_some());
    // Below is the roomier side, so it stays open downward
    assert!((popup.rect.top() - 140.0).abs() < 0.001);
}

#[test]
fn up_preferred_but_only_fits_below_flips_down() {
    let (popup, content) = open_drop_down(2, 210.0, MenuAnchorKind::BottomBar);
    assert!((popup.rect.height - content).abs() < 0.001);
    assert!((popup.rect.top() - 210.0).abs() < 0.001);
    assert!(popup.rect.y >= 0.0);
}
