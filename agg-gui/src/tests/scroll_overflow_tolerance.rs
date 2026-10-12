//! A scroll view counts its content as overflowing only past agg-sharp's
//! `ScrollableWidget.OverflowTolerance` (1e-6): content a rounding error
//! larger than the view (a window fitted to it) shows no bar and does not
//! scroll; content larger by more than the tolerance does.

use crate::widgets::OVERFLOW_TOLERANCE;
use crate::{Rect, ScrollBarStyle, ScrollBarVisibility, ScrollView, Size, SizedBox, Widget};

/// A 200 x 200 view, solid bar shown when needed, over a `w` x `h` block.
fn view_over(w: f64, h: f64) -> ScrollView {
    let content = SizedBox::new().with_width(w).with_height(h);
    let mut scroll = ScrollView::new(Box::new(content))
        .horizontal(true)
        .with_style(ScrollBarStyle::solid())
        .with_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded);
    scroll.set_bounds(Rect::new(0.0, 0.0, 200.0, 200.0));
    let _ = scroll.layout(Size::new(200.0, 200.0));
    scroll
}

#[test]
fn rust_only_content_within_the_overflow_tolerance_shows_no_bar() {
    let over = OVERFLOW_TOLERANCE / 10.0;
    let scroll = view_over(200.0 + over, 200.0 + over);
    let bars = scroll.scroll_bar_properties();
    assert!(
        bars.vertical.content > 200.0,
        "the content is a hair taller"
    );
    assert!(
        !bars.vertical.showing,
        "no vertical bar for a rounding error: {:?} {:?}",
        bars.vertical, bars.horizontal
    );
    assert!(
        !bars.horizontal.showing,
        "no horizontal bar for a rounding error: {:?}",
        bars.horizontal
    );
}

#[test]
fn rust_only_content_past_the_overflow_tolerance_shows_a_bar() {
    let scroll = view_over(100.0, 200.0 + 1e-5);
    assert!(scroll.scroll_bar_properties().vertical.showing);
}
