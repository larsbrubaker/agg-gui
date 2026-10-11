//! Scroll-to and visible-row maths for scrolled views: where to scroll to put
//! a span (a row, an item) in view, and which rows a band of the view touches.
//!
//! Ports agg-sharp's `Gui/ScrollableWidget/ScrollOffsets.cs` (`ScrollAlignment`,
//! `TargetForSpan`, `VisibleRows`). The `ScrollableWidget` extension methods
//! of that file (`ScrollOffsetFromTop` and friends) are
//! [`super::ScrollView::scroll_offset`] / `set_scroll_offset` /
//! `max_scroll_value` here, whose offset is already "down from the top".
//! Everything is in the caller's units (logical pixels in agg-gui).
//! Used by virtual lists that draw only their visible rows; the tree view's
//! own `scroll_for_row` predates this and keeps its two-mode `ScrollAlign`.

/// Where [`target_for_span`] puts a span in the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollAlignment {
    /// The span's top at the view's top.
    Top,
    /// The span centred in the view.
    Center,
    /// The span's bottom at the view's bottom.
    Bottom,
    /// The least scroll that shows the whole span - none at all when it is
    /// already visible.
    BringIntoView,
}

/// The scroll offset that puts the span `span_top..span_top + span_height`
/// (measured down from the content's top) where `alignment` asks, in a view
/// `viewport_height` tall currently scrolled `current_offset`, clamped to
/// `0..=max_scroll`.
pub fn target_for_span(
    span_top: f64,
    span_height: f64,
    viewport_height: f64,
    current_offset: f64,
    max_scroll: f64,
    alignment: ScrollAlignment,
) -> f64 {
    let span_bottom = span_top + span_height;
    let target = match alignment {
        ScrollAlignment::Top => span_top,
        ScrollAlignment::Center => span_top - (viewport_height - span_height) * 0.5,
        ScrollAlignment::Bottom => span_bottom - viewport_height,
        ScrollAlignment::BringIntoView => {
            if span_top < current_offset {
                span_top
            } else if span_bottom > current_offset + viewport_height {
                span_bottom - viewport_height
            } else {
                current_offset
            }
        }
    };
    // C# `Math.Max(0, Math.Min(maxScroll, target))`, not `clamp` (which panics
    // on a negative max).
    0f64.max(max_scroll.min(target))
}

/// The rows, of `row_count` rows `row_height` tall stacked down from
/// `content_top` (y grows up, as agg-sharp's does), that touch the band
/// `visible_bottom..visible_top`, as `[first, end)`. Drawing only these keeps
/// a draw's cost to what is on screen however many rows there are.
pub fn visible_rows(
    content_top: f64,
    visible_bottom: f64,
    visible_top: f64,
    row_height: f64,
    row_count: usize,
) -> (usize, usize) {
    if row_count == 0 || row_height <= 0.0 || visible_top <= visible_bottom {
        return (0, 0);
    }
    // Clamped in doubles first: a band far outside the content would overflow
    // the integer cast.
    let count = row_count as f64;
    let first_row = ((content_top - visible_top) / row_height).floor();
    let end_row = ((content_top - visible_bottom) / row_height).ceil();
    let first = 0f64.max(count.min(first_row));
    let end = first.max(count.min(end_row));
    (first as usize, end as usize)
}
