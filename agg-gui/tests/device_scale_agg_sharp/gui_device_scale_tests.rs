//! agg-sharp `GuiDeviceScaleTests` (Tests/Agg.Tests/Agg.UI/GuiDeviceScaleTests.cs):
//! a sweep over widget pieces that were sized in raw device pixels and stayed
//! put on a Retina panel while the text beside them doubled.
//!
//! agg-gui lays out in logical units and the App paints the whole tree
//! through `ctx.scale(device_scale)`, so a size times the scale is the C#
//! device-pixel measurement; the painted checks render through that same
//! scale into a device-pixel framebuffer.
//!
//! - `TreeNodeExpanderGrowsWithDeviceScale`: the expander is the TreeView's
//!   `ExpandToggle` (C#'s "Expand Widget" container). Its laid-out size is
//!   the C# check; the painted arrow is checked to double as well, within
//!   two pixels of antialiasing at each end.
//! - `MenuHorizontalLineGrowsWithDeviceScale`: the rule a popup menu draws for
//!   a separator, measured as the ink it paints down a device-pixel column, so
//!   a hairline split over two half-covered rows does not pass for a 2 px rule.
//! - `ScrollingPopupLeavesRoomForTheScrollBar`: a popup menu too tall for its
//!   window widens its panel by the scroll bar's strip
//!   (`ScrollBarStyle` bar width plus its margins), the width the bar uses.
//!
//! Not ported here:
//! - `SplitterBarAndStartingDistanceGrowWithDeviceScale`: agg-sharp's
//!   distance `Splitter`; agg-gui's `Splitter` is a ratio splitter MatterCAD
//!   does not use. MatterCAD's splitters are mattercad-app's own.
//! - `DropDownListSeparatorGrowsWithDeviceScale`: agg-gui's `ComboBox` has
//!   no separator rows and MatterCAD never adds one to a drop-down (its
//!   `CreateSeparator` calls are on popup menus, covered by
//!   `menu_horizontal_line_grows_with_device_scale`).
//! - `RadioTextButtonUnderlineGrowsWithDeviceScale` and
//!   `DialogButtonMinimumWidthGrowsWithDeviceScale`: MatterCAD theme widgets
//!   (`ThemedRadioTextButton`, `ThemeConfig.CreateDialogButton`), ported in
//!   mattercad-app.
//! - `SimpleChartTextGrowsWithDeviceScale`: `SimpleChartWidget` is used only
//!   by MatterCAD's print-farm job manager, which is out of scope.
//! - `GroupBoxBorderInsetGrowsWithDeviceScale`: GroupBox is unused.

use agg_gui::widgets::current_scroll_style;
use agg_gui::widgets::menu::{MenuAnchorKind, MenuEntry, MenuItem, PopupMenu};
use agg_gui::widgets::tree_view::ExpandToggle;
use agg_gui::{Checkbox, Point, Size};

use super::support::{
    at_device_scale, font, layout_at, leftmost_painted_column, paint_scaled, paint_widget_scaled,
    painted_extent, pixel, Region, WHITE,
};

/// The container is laid out from the toggle inside it, so this guards the
/// size the tree actually lays out with.
#[test]
fn tree_node_expander_grows_with_device_scale() {
    let at_one = at_device_scale(1.0, expander_size);
    let at_two = at_device_scale(2.0, expander_size);
    assert!(
        (at_two.0 - at_one.0 * 2.0).abs() <= 0.001,
        "the expander container has to keep up with the icon button inside it"
    );
    assert!((at_two.1 - at_one.1 * 2.0).abs() <= 0.001);

    let arrow_one = at_device_scale(1.0, expander_arrow);
    let arrow_two = at_device_scale(2.0, expander_arrow);
    assert!(
        arrow_one.0 > 0.0,
        "a collapsed node with children draws its arrow"
    );
    assert!(
        (arrow_two.0 - arrow_one.0 * 2.0).abs() <= 2.0,
        "{arrow_one:?} {arrow_two:?}"
    );
    assert!(
        (arrow_two.1 - arrow_one.1 * 2.0).abs() <= 2.0,
        "{arrow_one:?} {arrow_two:?}"
    );

    /// The expander's device-pixel width and height.
    fn expander_size() -> (f64, f64) {
        let mut expander = ExpandToggle::new(true, false);
        let size = layout_at(&mut expander, Size::new(200.0, 20.0));
        let s = agg_gui::device_scale();
        (size.width * s, size.height * s)
    }

    fn expander_arrow() -> (f64, f64) {
        let mut expander = ExpandToggle::new(true, false);
        layout_at(&mut expander, Size::new(200.0, 20.0));
        let fb = paint_widget_scaled(&mut expander);
        painted_extent(&fb, Region::whole(&fb), WHITE)
    }
}

#[test]
fn menu_horizontal_line_grows_with_device_scale() {
    let at_one = at_device_scale(1.0, line_height);
    let at_two = at_device_scale(2.0, line_height);
    assert!(at_one > 0.0, "the separator draws its rule");
    assert!(
        (at_two - at_one * 2.0).abs() <= 0.001,
        "a one pixel rule disappears on a Retina panel, so it is a one point rule instead: \
         {at_one} then {at_two}"
    );

    /// The device-pixel height of the separator's rule, as ink.
    fn line_height() -> f64 {
        const VIEWPORT: Size = Size {
            width: 300.0,
            height: 300.0,
        };
        let mut menu = PopupMenu::new(vec![
            MenuItem::action("A", "a").into(),
            MenuEntry::Separator,
            MenuItem::action("B", "b").into(),
        ]);
        menu.open_at(Point::new(20.0, 280.0));
        let row = menu.state.layouts(&menu.items, VIEWPORT)[0].rows[1].rect;
        let fb = paint_scaled(VIEWPORT, |ctx| menu.paint(ctx, font(), 14.0, VIEWPORT));
        let mut region = Region::of_logical(row, &fb);
        // the middle of the rule, clear of the row's ends and the panel edge
        let s = agg_gui::device_scale();
        region.x0 += (20.0 * s) as u32;
        region.x1 -= (20.0 * s) as u32;
        let background = pixel(&fb, region.x0, region.y0.saturating_sub(1));
        // Antialiasing splits a rule that straddles a pixel edge over two
        // half-covered rows, so the rule's height is its ink down the middle
        // column: how far each row's red channel is from the background's,
        // summed (a full-coverage row counts its whole contrast). The units
        // cancel in the doubling the test checks.
        let x = (region.x0 + region.x1) / 2;
        (region.y0..region.y1)
            .map(|y| (f64::from(pixel(&fb, x, y)[0]) - f64::from(background[0])).abs())
            .sum()
    }
}

#[test]
fn scrolling_popup_leaves_room_for_the_scroll_bar() {
    let at_one = at_device_scale(1.0, extra_width);
    let at_two = at_device_scale(2.0, extra_width);
    assert!(
        (at_one - scroll_bar_width_at_scale(1.0)).abs() <= 0.001,
        "the room made for the scroll bar has to be the width the scroll bar actually uses: \
         {at_one}"
    );
    assert!(
        (at_two - scroll_bar_width_at_scale(2.0)).abs() <= 0.001,
        "{at_two}"
    );

    /// How much wider, in device pixels, a popup makes itself once it has to
    /// scroll.
    fn extra_width() -> f64 {
        let width_in = |window_h: f64| {
            let items: Vec<MenuEntry> = (0..8)
                .map(|i| MenuItem::action(format!("Item {i}"), format!("{i}")).into())
                .collect();
            let mut menu = PopupMenu::new(items);
            menu.state
                .open_at(Point::new(10.0, window_h - 40.0), MenuAnchorKind::Bar);
            menu.state.set_anchor_span(20.0);
            menu.state.set_space_origin(Some(Point::ORIGIN));
            let layout = menu
                .state
                .layouts(&menu.items, Size::new(400.0, window_h))
                .remove(0);
            (layout.rect.width, layout.scroll.is_some())
        };
        let (unscrolled, fits_scrolled) = width_in(2000.0);
        let (scrolled, has_to_scroll) = width_in(100.0);
        assert!(
            !fits_scrolled && has_to_scroll,
            "only the short window scrolls"
        );
        (scrolled - unscrolled) * agg_gui::device_scale()
    }

    fn scroll_bar_width_at_scale(device_scale: f64) -> f64 {
        at_device_scale(device_scale, || {
            let style = current_scroll_style();
            (style.bar_width + style.outer_margin + style.inner_margin) * agg_gui::device_scale()
        })
    }
}

#[test]
fn check_box_frame_inset_grows_with_device_scale() {
    // One point of inset is a single pixel at scale 1, so the two scales are
    // picked far enough apart that the gap is several pixels wide and cannot
    // be confused with antialiasing.
    let at_one = at_device_scale(1.0, leftmost_painted);
    let at_four = at_device_scale(4.0, leftmost_painted);
    assert!(
        (at_four - at_one * 4.0).abs() <= 1.0,
        "the gap between the widget edge and the box is the view's own geometry: \
         {at_one} then {at_four}"
    );

    fn leftmost_painted() -> f64 {
        // An empty label leaves the box as the only thing the view paints.
        let mut check_box = Checkbox::new("", font(), false);
        layout_at(&mut check_box, Size::new(200.0, 100.0));
        let fb = paint_widget_scaled(&mut check_box);
        leftmost_painted_column(&fb)
    }
}
