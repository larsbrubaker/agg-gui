//! Scrolling the list to a row the app selected for the user (`BrowserState::select_when_listed`:
//! a copy just made), so the selection they are told about is on screen. The list
//! (`list/mod.rs`) asks after laying out its rows: the row is found by name under the list's
//! `ScrollView`, and the least scroll that shows it whole is written to the scroll position the
//! list keeps across rebuilds.

use std::cell::Cell;

use agg_gui::widgets::scroll_view::scroll_offsets::{target_for_span, ScrollAlignment};
use agg_gui::Widget;

/// Scrolls the `ScrollView` under `children` that holds the row named `row_name` so the row shows
/// whole, writing its offset (down from the top) to `offset`. Answers whether it scrolled.
pub fn reveal_row(children: &[Box<dyn Widget>], row_name: &str, offset: &Cell<f64>) -> bool {
    for child in children {
        let widget = child.as_ref();
        if widget.type_name() == "ScrollView" {
            if let Some(content) = widget.children().first() {
                if let Some((y, height)) = row_in(content.as_ref(), row_name, 0.0) {
                    // Bounds are Y up; the scroll offset counts down from the content's top.
                    let content_height = content.bounds().height;
                    let view_height = widget.bounds().height;
                    let top = content_height - (y + height);
                    let max = (content_height - view_height).max(0.0);
                    let current = offset.get();
                    let target = target_for_span(
                        top,
                        height,
                        view_height,
                        current,
                        max,
                        ScrollAlignment::BringIntoView,
                    );
                    if (target - current).abs() > 0.5 {
                        offset.set(target);
                        agg_gui::animation::request_layout();
                        return true;
                    }
                    return false;
                }
            }
        }
        if reveal_row(widget.children(), row_name, offset) {
            return true;
        }
    }
    false
}

/// The bottom and height of the widget named `row_name` under `widget`, in the coordinates of
/// `widget`'s parent offset by `base` (the sum of the bounds above it), when it is there.
fn row_in(widget: &dyn Widget, row_name: &str, base: f64) -> Option<(f64, f64)> {
    widget.children().iter().find_map(|child| {
        let y = base + child.bounds().y;
        if child.id() == Some(row_name) {
            return Some((y, child.bounds().height));
        }
        row_in(child.as_ref(), row_name, y)
    })
}
