//! agg-sharp's `Tests/Agg.Tests/Agg.UI/ScrollableWidgetTests.cs`, on
//! [`ScrollView`]: the bar is re-decided whenever its mode, the view's size
//! or the content changes, and its track follows the view it scrolls.
//!
//! agg-sharp's `VerticalScrollBar.Visible` and `.Height` are read from
//! [`ScrollView::scroll_bar_properties`] (agg-gui paints its bars instead of
//! adding them as children). `ShowState.Never` / `WhenRequired` are
//! [`ScrollBarVisibility::AlwaysHidden`] / `VisibleWhenNeeded` on a
//! [`ScrollBarStyle::solid`] bar, the always-drawn kind agg-sharp's bar is.
//! agg-gui lays out when asked, so each `container.PerformLayout()` is a
//! layout at the view's size, and a C# `Height = ...` is that layout at the
//! new height.
//!
//! The agg-gui track runs `inner_margin` inside each end of the view rather
//! than spanning it edge to edge, so "the track spans the view" is "the
//! track's ends sit `inner_margin` inside the view's ends".
//!
//! `LimitScrolToContetsTests` is a behaviour test: an empty scroll view paints
//! nothing over a white background, pixel for pixel the same as a plain
//! widget. `FitAroundScrolledContentStaysAnchoredAtTheOrigin`: a fitted
//! agg-gui view is as tall as [`Widget::measure_min_height`] reports, and its
//! own bounds are given by its parent, so the test checks the fitted height
//! is the content's (not where the scrolled content hangs) and that the track
//! then lands at the view's local origin.
//!
//! Rust-only, for two agg-gui `ScrollView` bugs these ports turned up (in
//! `ScrollingAreaDeviceScaleTests.ScrollEndsLeaveExactlyTheDeviceMargin`):
//! `rust_only_a_solid_horizontal_bar_leaves_the_content_top_at_the_view_top`
//! (the content was placed against the viewport's height rather than the
//! view's top, so a Solid horizontal bar left a bar-sized gap above the
//! content and hid its last rows under the bar), and
//! `rust_only_a_never_shown_solid_bar_takes_no_room` (an `AlwaysHidden` Solid
//! bar still reserved its room, as agg-sharp's `ShowState.Never` bar does
//! not).

use super::scroll_fixtures::Fixed;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::widget::paint_subtree;
use crate::{
    Color, FlexColumn, Rect, ScrollBarStyle, ScrollBarVisibility, ScrollView, Size, SizedBox,
    Widget,
};

/// A 200 x 200 view over `content`, with a solid bar shown when required,
/// laid out.
fn make_scrollable(content: Box<dyn Widget>) -> ScrollView {
    let mut scroll = ScrollView::new(content)
        .with_style(ScrollBarStyle::solid())
        .with_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded);
    lay_out(&mut scroll, 200.0);
    scroll
}

/// A column holding one `width` x `height` block, the C# `AddChild`.
fn column_of(width: f64, height: f64) -> Box<dyn Widget> {
    Box::new(FlexColumn::new().add(Box::new(
        SizedBox::new().with_width(width).with_height(height),
    )))
}

fn lay_out(scroll: &mut ScrollView, height: f64) {
    scroll.set_bounds(Rect::new(0.0, 0.0, 200.0, height));
    let _ = scroll.layout(Size::new(200.0, height));
}

/// The track spans the view: its ends sit the style's inner margin inside
/// the view's bottom and top.
fn assert_track_spans_view(scroll: &ScrollView) {
    let track = scroll.scroll_bar_properties().vertical.track;
    let m = ScrollBarStyle::solid().inner_margin;
    let h = scroll.bounds().height;
    assert_eq!(track.y, m, "track {track:?} in a {h} tall view");
    assert_eq!(
        track.y + track.height,
        h - m,
        "track {track:?} in a {h} tall view"
    );
}

fn paint_white(widget: &mut dyn Widget) -> Vec<u8> {
    let mut fb = Framebuffer::new(200, 200);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        paint_subtree(widget, &mut ctx);
    }
    fb.pixels().to_vec()
}

#[test]
fn limit_scrol_to_contets_tests() {
    let mut control = Fixed::new(200.0, 200.0, None);
    control.set_bounds(Rect::new(0.0, 0.0, 200.0, 200.0));
    let _ = control.layout(Size::new(200.0, 200.0));
    let control_pixels = paint_white(&mut control);

    let mut test = ScrollView::new(Box::new(SizedBox::new()));
    test.set_bounds(Rect::new(0.0, 0.0, 200.0, 200.0));
    let _ = test.layout(Size::new(200.0, 200.0));
    let test_pixels = paint_white(&mut test);

    assert!(!control_pixels.is_empty());
    assert!(control_pixels == test_pixels);
}

#[test]
fn show_state_change_reevaluates_visibility_with_no_layout() {
    let mut scroll = make_scrollable(column_of(100.0, 500.0));

    scroll.set_bar_visibility(ScrollBarVisibility::AlwaysHidden);
    assert!(
        !scroll.scroll_bar_properties().vertical.showing,
        "Never means never, however tall the content is"
    );

    scroll.set_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded);
    let bar = scroll.scroll_bar_properties().vertical;
    assert!(
        bar.showing,
        "{} of content does not fit {}, so the bar is required the moment the mode allows it \
         - without waiting for a layout",
        bar.content, bar.viewport
    );
}

#[test]
fn shrinking_the_view_shows_the_bar_and_growing_it_back_hides_it() {
    let mut scroll = make_scrollable(column_of(100.0, 150.0));
    assert!(
        !scroll.scroll_bar_properties().vertical.showing,
        "150 of content fits the 200 tall view"
    );

    lay_out(&mut scroll, 100.0);
    assert!(
        scroll.scroll_bar_properties().vertical.showing,
        "the same content no longer fits the 100 the view is now"
    );

    lay_out(&mut scroll, 200.0);
    assert!(
        !scroll.scroll_bar_properties().vertical.showing,
        "and it fits again, so the bar has nothing to scroll to"
    );
}

#[test]
fn removing_the_tall_content_hides_the_bar() {
    let mut scroll = make_scrollable(column_of(100.0, 500.0));
    assert!(
        scroll.scroll_bar_properties().vertical.showing,
        "500 of content in a 200 tall view needs a bar"
    );

    scroll.children_mut()[0].children_mut().remove(0);
    lay_out(&mut scroll, 200.0);

    let bar = scroll.scroll_bar_properties().vertical;
    assert!(
        bar.content <= scroll.bounds().height,
        "the scrolling area cannot stay as tall as content that has been taken out of it \
         ({} of content)",
        bar.content
    );
    assert!(!bar.showing, "there is nothing left to scroll to");
}

#[test]
fn bar_height_follows_the_view_across_a_hidden_phase() {
    let mut scroll = make_scrollable(column_of(100.0, 500.0));
    assert_track_spans_view(&scroll);

    scroll.set_bar_visibility(ScrollBarVisibility::AlwaysHidden);
    lay_out(&mut scroll, 120.0);
    scroll.set_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded);

    assert!(
        scroll.scroll_bar_properties().vertical.showing,
        "500 of content still does not fit the 120 tall view"
    );
    // a track sized against the height the view used to be cannot reach the
    // bottom of the content
    assert_track_spans_view(&scroll);
}

#[test]
fn visible_bar_height_follows_the_view() {
    let mut scroll = make_scrollable(column_of(100.0, 500.0));
    lay_out(&mut scroll, 90.0);
    assert!(scroll.scroll_bar_properties().vertical.showing);
    assert_track_spans_view(&scroll);
}

#[test]
fn fit_around_scrolled_content_stays_anchored_at_the_origin() {
    let mut scroll = make_scrollable(column_of(100.0, 500.0));

    // show the top of the content, which puts it below the view's origin
    scroll.set_scroll_offset(0.0);
    lay_out(&mut scroll, 200.0);
    assert!(
        scroll.children()[0].bounds().y < 0.0,
        "the test only means anything while the content is scrolled off the bottom"
    );

    // fit: as tall as the content it holds, measured from its own origin,
    // not from where the content scrolled to
    let fitted = scroll.measure_min_height(200.0);
    assert_eq!(fitted, 500.0, "fit is as tall as the content it holds");
    lay_out(&mut scroll, fitted);
    assert_eq!(scroll.children()[0].bounds().y, 0.0);

    // the host then pins the height, the way the docked sheet editor does
    lay_out(&mut scroll, 300.0);
    let track = scroll.scroll_bar_properties().vertical.track;
    assert_eq!(
        track.y,
        ScrollBarStyle::solid().inner_margin,
        "the track is laid out at the local origin and has to land on the view (got {track:?})"
    );
    assert_track_spans_view(&scroll);
}

/// A 200 x 100 two-way view over 600 x 500 content with a solid bar shown as
/// `show` says, laid out, and its vertical offset cell.
fn two_way(show: ScrollBarVisibility) -> ScrollView {
    let mut scroll = ScrollView::new(Box::new(
        SizedBox::new().with_width(600.0).with_height(500.0),
    ))
    .horizontal(true)
    .with_style(ScrollBarStyle::solid())
    .with_bar_visibility(show);
    scroll.set_bounds(Rect::new(0.0, 0.0, 200.0, 100.0));
    let _ = scroll.layout(Size::new(200.0, 100.0));
    scroll
}

fn lay_out_two_way(scroll: &mut ScrollView) {
    let _ = scroll.layout(Size::new(200.0, 100.0));
}

#[test]
fn rust_only_a_solid_horizontal_bar_leaves_the_content_top_at_the_view_top() {
    let mut scroll = two_way(ScrollBarVisibility::VisibleWhenNeeded);
    let reserved = scroll.scroll_bar_properties().horizontal.reserved;
    assert!(reserved > 0.0, "the solid bar takes the bottom of the view");

    // scrolled to the top, the content's top is the view's top: the bar takes
    // the bottom of the view, not a strip above the content
    let content = scroll.children()[0].bounds();
    assert_eq!(content.y + content.height, 100.0, "{content:?}");

    // scrolled to the end, the content's bottom rests on the bar, not under it
    scroll.set_scroll_offset(100_000.0);
    lay_out_two_way(&mut scroll);
    let content = scroll.children()[0].bounds();
    assert_eq!(content.y, reserved, "{content:?}");
}

#[test]
fn rust_only_a_never_shown_solid_bar_takes_no_room() {
    let mut scroll = two_way(ScrollBarVisibility::AlwaysHidden);
    assert_eq!(scroll.scroll_bar_properties().horizontal.reserved, 0.0);
    assert_eq!(scroll.scroll_bar_properties().vertical.reserved, 0.0);

    scroll.set_scroll_offset(100_000.0);
    lay_out_two_way(&mut scroll);
    let content = scroll.children()[0].bounds();
    assert_eq!(
        content.y, 0.0,
        "the content reaches the view's bottom: {content:?}"
    );
}
