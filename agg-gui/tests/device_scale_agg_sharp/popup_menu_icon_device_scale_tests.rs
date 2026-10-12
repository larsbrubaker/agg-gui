//! agg-sharp `PopupMenuIconDeviceScaleTests`
//! (Tests/Agg.Tests/Agg.UI/PopupMenuIconDeviceScaleTests.cs): a menu item's
//! radio, check and disabled marks follow a display-scale change under a live
//! menu, so they stay the size of the text beside them.
//!
//! C# caches each mark as an image built at the device scale and checks the
//! cached image's width (16 design units: 16 px at 1x, 32 at 2x). agg-gui
//! has no cache: the marks are vector paths in logical units, drawn on every
//! paint through the App's `ctx.scale(device_scale)`. The tests keep C#'s
//! shape - one menu, painted at 1x, then painted again after the scale
//! changed under it - and measure the mark as painted device pixels: the
//! mark agg-gui draws (a [`RADIO_RADIUS`] circle with a 1 unit outline, or the
//! check mark) at its own size at 1x, and twice that at 2x. Antialiasing
//! feathers the edges by up to a pixel at each end, a slop that does not grow
//! with the mark, so it is a fixed tolerance of 2 (as `SubMenuArrowDeviceScaleTests`).
//!
//! The menu uses a [`SelectionColumn`], as MatterCAD's menus do, so a radio
//! row draws its circle the way agg-sharp's `RadioMenuItem` does.

use agg_gui::widgets::menu::paint::RADIO_RADIUS;
use agg_gui::widgets::menu::{MenuItem, MenuStyle, PopupMenu, SelectionColumn};
use agg_gui::{set_device_scale, Point, Rect, Size};

use super::support::{at_device_scale, dominant_color, font, paint_scaled, painted_extent, Region};

const VIEWPORT: Size = Size {
    width: 300.0,
    height: 200.0,
};
const MARK_X: f64 = 12.0;
const ANTIALIAS_SLOP: f64 = 2.0;

fn menu_with(item: MenuItem) -> PopupMenu {
    let style = MenuStyle {
        selection_column: Some(SelectionColumn {
            mark_x: MARK_X,
            icon_x: 24.0,
            icon_label_x: 44.0,
        }),
        ..MenuStyle::default()
    };
    let mut menu = PopupMenu::new(vec![item.into()]).with_style(style);
    menu.open_at(Point::new(20.0, 180.0));
    menu
}

/// Paints the menu at the current device scale and returns the painted width
/// and height of the mark in the row's selection column.
fn drawn_mark(menu: &mut PopupMenu) -> (f64, f64) {
    let row = menu.state.layouts(&menu.items, VIEWPORT)[0].rows[0].rect;
    let fb = paint_scaled(VIEWPORT, |ctx| menu.paint(ctx, font(), 14.0, VIEWPORT));
    // clear of the panel's border, which runs along the top and bottom of a
    // menu's only row
    let column = Rect::new(row.x + MARK_X - 9.0, row.y + 3.0, 18.0, row.height - 6.0);
    let region = Region::of_logical(column, &fb);
    let background = dominant_color(&fb, region);
    painted_extent(&fb, region, background)
}

fn assert_follows_scale(at_one: (f64, f64), at_two: (f64, f64), expected_one: (f64, f64)) {
    assert!(
        (at_one.0 - expected_one.0).abs() <= ANTIALIAS_SLOP
            && (at_one.1 - expected_one.1).abs() <= ANTIALIAS_SLOP,
        "the mark is its design size in pixels on a 1x display: {at_one:?}"
    );
    assert!(
        (at_two.0 - expected_one.0 * 2.0).abs() <= ANTIALIAS_SLOP
            && (at_two.1 - expected_one.1 * 2.0).abs() <= ANTIALIAS_SLOP,
        "the text beside it doubled when the menu moved to a 2x display: {at_two:?}"
    );
}

/// The radio circle's diameter plus its 1 unit outline.
fn radio_size() -> (f64, f64) {
    let d = RADIO_RADIUS * 2.0 + 1.0;
    (d, d)
}

#[test]
fn radio_menu_item_icon_follows_a_display_scale_change() {
    at_device_scale(1.0, || {
        let mut menu = menu_with(MenuItem::action("Radio", "r").radio(true));
        let at_one = drawn_mark(&mut menu);
        set_device_scale(2.0);
        let at_two = drawn_mark(&mut menu);
        assert_follows_scale(at_one, at_two, radio_size());
    });
}

#[test]
fn checkbox_menu_item_icon_follows_a_display_scale_change() {
    at_device_scale(1.0, || {
        let mut menu = menu_with(MenuItem::action("Check", "c").checked(true));
        let at_one = drawn_mark(&mut menu);
        set_device_scale(2.0);
        let at_two = drawn_mark(&mut menu);
        // the check mark's path spans 10 x 7.5 units, stroked 1.6 wide
        assert_follows_scale(at_one, at_two, (11.6, 9.1));
    });
}

/// A disabled item draws a faded mark; it has to follow the scale with the
/// enabled items beside it.
#[test]
fn a_disabled_menu_item_icon_follows_a_display_scale_change() {
    at_device_scale(1.0, || {
        let mut menu = menu_with(MenuItem::action("Radio", "r").radio(true).disabled());
        let at_one = drawn_mark(&mut menu);
        set_device_scale(2.0);
        let at_two = drawn_mark(&mut menu);
        assert_follows_scale(at_one, at_two, radio_size());
    });
}
