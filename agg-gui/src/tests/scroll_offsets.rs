//! agg-sharp's `Tests/Agg.Tests/Agg.UI/ScrollOffsetsTests.cs`, against
//! [`crate::widgets::scroll_view::scroll_offsets`] and [`ScrollView`]'s
//! down-from-the-top offset.
//!
//! agg-sharp's `SetScrollOffsetFromTop` clamps at once; agg-gui's
//! `set_scroll_offset` stores the request and the next layout clamps it to the
//! travel, so the ScrollView cases lay out before reading the offset back.

use super::*;
use crate::geometry::Rect;
use crate::widgets::scroll_view::scroll_offsets::{target_for_span, visible_rows, ScrollAlignment};

fn scrollable(view_height: f64, content_height: f64) -> ScrollView {
    let content = SizedBox::new().with_height(content_height);
    let mut scroll = ScrollView::new(Box::new(content));
    scroll.set_bounds(Rect::new(0.0, 0.0, 200.0, view_height));
    let _ = scroll.layout(Size::new(200.0, view_height));
    scroll
}

fn relayout(scroll: &mut ScrollView) {
    let size = Size::new(scroll.bounds().width, scroll.bounds().height);
    let _ = scroll.layout(size);
}

#[test]
fn offset_from_top_starts_at_zero_and_clamps_to_the_travel() {
    let mut scroll = scrollable(100.0, 500.0);
    assert_eq!(scroll.max_scroll_value(), 400.0);
    assert_eq!(scroll.scroll_offset(), 0.0);

    scroll.set_scroll_offset(150.0);
    relayout(&mut scroll);
    assert_eq!(scroll.scroll_offset(), 150.0);

    // 150 down from the top: the content's top is 150 above the view's.
    let child = scroll.children()[0].bounds();
    assert_eq!(child.y + child.height - scroll.bounds().height, 150.0);

    scroll.set_scroll_offset(10_000.0);
    relayout(&mut scroll);
    assert_eq!(scroll.scroll_offset(), 400.0);
    scroll.set_scroll_offset(-5.0);
    relayout(&mut scroll);
    assert_eq!(scroll.scroll_offset(), 0.0);
}

#[test]
fn content_shorter_than_the_view_has_no_travel() {
    let mut scroll = scrollable(300.0, 100.0);
    scroll.set_scroll_offset(50.0);
    relayout(&mut scroll);
    assert_eq!(scroll.max_scroll_value(), 0.0);
    assert_eq!(scroll.scroll_offset(), 0.0);
}

#[test]
fn target_for_span_aligns_top_center_and_bottom() {
    // Item 10 of 20-pixel rows in a 100-pixel view.
    use ScrollAlignment::*;
    assert_eq!(target_for_span(200.0, 20.0, 100.0, 0.0, 1000.0, Top), 200.0);
    assert_eq!(
        target_for_span(200.0, 20.0, 100.0, 0.0, 1000.0, Center),
        160.0
    );
    assert_eq!(
        target_for_span(200.0, 20.0, 100.0, 0.0, 1000.0, Bottom),
        120.0
    );

    // Clamped at both ends.
    assert_eq!(target_for_span(0.0, 20.0, 100.0, 50.0, 1000.0, Center), 0.0);
    assert_eq!(target_for_span(990.0, 20.0, 100.0, 0.0, 900.0, Top), 900.0);
}

#[test]
fn bring_into_view_scrolls_only_as_far_as_a_hidden_edge_needs() {
    let b = ScrollAlignment::BringIntoView;
    assert_eq!(target_for_span(160.0, 20.0, 100.0, 100.0, 1000.0, b), 100.0);
    assert_eq!(target_for_span(40.0, 20.0, 100.0, 100.0, 1000.0, b), 40.0);
    assert_eq!(target_for_span(240.0, 20.0, 100.0, 100.0, 1000.0, b), 160.0);
}

#[test]
fn visible_rows_are_only_those_the_band_touches() {
    // 1000 rows of 10 under a top at 10,000: a band from 9,955 to 9,975 touches rows 2 to 4.
    assert_eq!(visible_rows(10_000.0, 9_955.0, 9_975.0, 10.0, 1000), (2, 5));

    // Band above and below the content, an empty band, no rows.
    assert_eq!(
        visible_rows(10_000.0, 10_100.0, 10_200.0, 10.0, 1000),
        (0, 0)
    );
    assert_eq!(
        visible_rows(10_000.0, -500.0, -100.0, 10.0, 1000),
        (1000, 1000)
    );
    assert_eq!(visible_rows(10_000.0, 50.0, 50.0, 10.0, 1000), (0, 0));
    assert_eq!(
        visible_rows(10_000.0, f64::MIN, f64::MAX, 10.0, 1000),
        (0, 1000)
    );
}
