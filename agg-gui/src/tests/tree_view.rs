//! TreeView interaction invalidation tests.
//!
//! The inspector relies on TreeView hover state to publish the highlighted
//! widget bounds.  A row-hover change is visual state, so TreeView must request
//! a draw without the inspector needing custom invalidation glue.

use std::sync::Arc;

use crate::event::{Event, EventResult};
use crate::geometry::{Point, Size};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::tree_view::{NodeIcon, TreeView};

#[test]
fn tree_view_hover_change_requests_draw() {
    crate::animation::clear_draw_request();

    let font = Arc::new(Font::from_slice(super::TEST_FONT).expect("font"));
    let mut tree = TreeView::new(font).with_row_height(20.0);
    tree.add_root("Root", NodeIcon::Package);
    tree.layout(Size::new(200.0, 100.0));
    tree.set_bounds(crate::Rect::new(0.0, 0.0, 200.0, 100.0));

    let result = tree.on_event(&Event::MouseMove {
        pos: Point::new(10.0, 90.0),
    });

    assert_eq!(result, EventResult::Consumed);
    assert!(crate::animation::wants_draw());
}
