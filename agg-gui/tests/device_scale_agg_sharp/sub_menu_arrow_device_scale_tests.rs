//! agg-sharp `SubMenuArrowDeviceScaleTests`
//! (Tests/Agg.Tests/Agg.UI/SubMenuArrowDeviceScaleTests.cs): the arrow a
//! sub-menu row paints at its right end is the row's own geometry, so on a 2x
//! display it doubles with the text beside it.
//!
//! The arrow is agg-gui's submenu chevron, drawn in logical units through the
//! App's `ctx.scale(device_scale)`. The row is a popup menu's only row, with
//! a blank label; the arrow is everything painted in the row's right end that
//! differs from the panel behind it, measured in device pixels with C#'s
//! extent, constants and antialiasing slop.

use agg_gui::widgets::menu::{MenuItem, PopupMenu};
use agg_gui::{Point, Rect, Size};

use super::support::{at_device_scale, dominant_color, font, paint_scaled, painted_extent, Region};

// The arrow's geometry at scale one.
const ARROW_WIDTH_AT_SCALE_ONE: f64 = 6.0;
const ARROW_HEIGHT_AT_SCALE_ONE: f64 = 10.0;

/// Antialiasing feathers the sloped edges, so a measured extent can run a
/// pixel past the geometry at each end. That slop does not grow with the
/// arrow, so it stays a fixed tolerance.
const ANTIALIAS_SLOP: f64 = 2.0;

#[test]
fn arrow_keeps_its_shape_at_scale_one() {
    let (width, height) = drawn_arrow_extent(1.0);
    assert!(
        (width - ARROW_WIDTH_AT_SCALE_ONE).abs() <= ANTIALIAS_SLOP,
        "{width}"
    );
    assert!(
        (height - ARROW_HEIGHT_AT_SCALE_ONE).abs() <= ANTIALIAS_SLOP,
        "{height}"
    );
}

#[test]
fn arrow_doubles_with_device_scale() {
    let (width, height) = drawn_arrow_extent(2.0);
    assert!(
        (width - ARROW_WIDTH_AT_SCALE_ONE * 2.0).abs() <= ANTIALIAS_SLOP,
        "the arrow is the menu item's own geometry, so it has to double when everything else \
         does: {width}"
    );
    assert!(
        (height - ARROW_HEIGHT_AT_SCALE_ONE * 2.0).abs() <= ANTIALIAS_SLOP,
        "{height}"
    );
}

/// Renders a sub menu row and returns the device-pixel extent of what it
/// painted at its right end.
fn drawn_arrow_extent(device_scale: f64) -> (f64, f64) {
    const VIEWPORT: Size = Size {
        width: 300.0,
        height: 200.0,
    };
    at_device_scale(device_scale, || {
        let mut menu = PopupMenu::new(vec![MenuItem::submenu(
            "",
            vec![MenuItem::action("Inner", "inner").into()],
        )
        .into()]);
        menu.open_at(Point::new(20.0, 180.0));
        let row = menu.state.layouts(&menu.items, VIEWPORT)[0].rows[0].rect;
        let fb = paint_scaled(VIEWPORT, |ctx| menu.paint(ctx, font(), 14.0, VIEWPORT));
        // clear of the panel's border, which runs along the top and bottom of
        // a menu's only row and down its right side
        let end = Rect::new(
            row.x + row.width - 24.0,
            row.y + 3.0,
            20.0,
            row.height - 6.0,
        );
        let region = Region::of_logical(end, &fb);
        let background = dominant_color(&fb, region);
        painted_extent(&fb, region, background)
    })
}
