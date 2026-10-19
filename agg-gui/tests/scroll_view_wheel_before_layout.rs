//! A scroll made by input (a wheel, a scrollbar drag, a keyboard lift) moves
//! a `ScrollView`'s content at once, not at the next layout:
//! layout runs with the next painted frame, and on a slow GPU many input
//! events arrive before it (the web shell holds a frame back while the GPU
//! is busy). A press after a wheel and before any layout must land on what
//! the scroll put under the pointer, and a lookup of a row's place must say
//! where the scroll put it.

use agg_gui::widget::hit_test_subtree;
use agg_gui::{
    Container, Event, FlexColumn, Modifiers, MouseButton, Point, Rect, ScrollBarVisibility,
    ScrollView, Size, SizedBox, Widget,
};

/// A 200 x 100 scroll view over ten 50-high rows.
fn tree() -> Container {
    let mut rows = FlexColumn::new().with_gap(0.0);
    for _ in 0..10 {
        rows = rows.add(Box::new(
            SizedBox::new().with_width(180.0).with_height(50.0),
        ));
    }
    let scroll =
        ScrollView::new(Box::new(rows)).with_bar_visibility(ScrollBarVisibility::AlwaysVisible);
    let mut root = Container::new().add(Box::new(scroll));
    root.set_bounds(Rect::new(0.0, 0.0, 200.0, 100.0));
    root.layout(Size::new(200.0, 100.0));
    root
}

/// The row under `pos` (root coordinates, Y up): the hit path's step into the row column.
fn row_at(root: &Container, pos: Point) -> Option<usize> {
    let path = hit_test_subtree(root, pos)?;
    // root > scroll view > row column > row
    path.get(2).copied()
}

fn wheel(root: &mut Container, delta_y: f64) {
    root.children_mut()[0].on_event(&Event::MouseWheel {
        pos: Point::new(100.0, 50.0),
        delta_y,
        delta_x: 0.0,
        modifiers: Modifiers::default(),
    });
}

#[test]
fn a_press_after_a_wheel_and_before_layout_lands_on_the_scrolled_row() {
    let mut root = tree();
    let near_top = Point::new(90.0, 90.0);
    assert_eq!(
        row_at(&root, near_top),
        Some(0),
        "the top shows the first row"
    );
    // Down the content, with no layout after it.
    wheel(&mut root, -1.0);
    let scrolled = row_at(&root, near_top);
    assert!(
        scrolled.is_some_and(|row| row > 0),
        "the wheel moved the rows: {scrolled:?}"
    );
    // Layout moves nothing further: the press saw what layout shows.
    root.layout(Size::new(200.0, 100.0));
    assert_eq!(row_at(&root, near_top), scrolled);
    // Back to the top, again with no layout.
    wheel(&mut root, 20.0);
    assert_eq!(row_at(&root, near_top), Some(0));
}

#[test]
fn a_scrollbar_drag_moves_the_rows_before_layout() {
    let mut root = tree();
    let near_top = Point::new(90.0, 90.0);
    assert_eq!(row_at(&root, near_top), Some(0));
    let thumb = {
        let scroll = root.children()[0]
            .as_any()
            .and_then(|any| any.downcast_ref::<ScrollView>())
            .expect("the scroll view");
        scroll.scroll_bar_properties().vertical.thumb
    };
    // The view sits at the root's origin, so its local points are the root's.
    let grab = Point::new(thumb.x + thumb.width / 2.0, thumb.y + thumb.height / 2.0);
    let scroll = root.children_mut()[0].as_mut();
    scroll.on_event(&Event::MouseDown {
        pos: grab,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    scroll.on_event(&Event::MouseMove {
        pos: Point::new(grab.x, grab.y - 30.0),
    });
    // Released: while the thumb is held the view takes every hit.
    scroll.on_event(&Event::MouseUp {
        pos: Point::new(grab.x, grab.y - 30.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    let dragged = row_at(&root, near_top);
    assert!(
        dragged.is_some_and(|row| row > 0),
        "the drag moved the rows: {dragged:?}"
    );
    root.layout(Size::new(200.0, 100.0));
    assert_eq!(row_at(&root, near_top), dragged);
}

#[test]
fn a_keyboard_lift_moves_the_rows_before_layout() {
    let mut root = tree();
    let near_top = Point::new(90.0, 90.0);
    assert_eq!(row_at(&root, near_top), Some(0));
    let absorbed = root.children_mut()[0].try_scroll_to_lift(120.0);
    assert!(absorbed > 0.0, "the view took the lift");
    let lifted = row_at(&root, near_top);
    assert!(
        lifted.is_some_and(|row| row > 0),
        "the lift moved the rows: {lifted:?}"
    );
    root.layout(Size::new(200.0, 100.0));
    assert_eq!(row_at(&root, near_top), lifted);
}
