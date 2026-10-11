//! agg-sharp's `Tests/Agg.Tests/Agg.UI/WheelOcclusionRoutingTests.cs`: the
//! wheel goes to the topmost widget under the pointer and bubbles up through
//! its ancestors, never sideways to a covered sibling. Driven through
//! [`App::on_mouse_wheel_xy_mods`] (screen y down; the canvas is y-up as in
//! agg-sharp). agg-sharp's `WheelDown = -120` is one notch, `delta_y = -1`.
//!
//! `ANestedScrollViewAtItsLimitBubblesToTheOuterOne` lays the inner view out
//! in a [`Fixed`] host, agg-gui's equivalent of the C# fixed-size
//! `ScrollableWidget(150, 100)`.
//!
//! `ANonSelectableOverlayDoesNotBlockTheWheel` duplicates
//! `a_widget_that_declines_the_point_lets_the_wheel_through`: agg-gui has one
//! click-through mechanism, `Widget::hit_test` returning `false`, where
//! agg-sharp has both `PositionWithinLocalBounds` and `Selectable = false`.

use std::cell::Cell;
use std::rc::Rc;

use crate::geometry::Point;
use crate::{App, DrawCtx, Event, EventResult, Modifiers, Rect, ScrollView, Size, Widget};

use super::scroll_fixtures::Fixed;
use super::SizedBox;
use crate::FlexColumn;

/// A fixed-size plain widget; `transparent` declines every point.
struct Plain {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    transparent: bool,
}

impl Widget for Plain {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn hit_test(&self, local_pos: Point) -> bool {
        !self.transparent
            && local_pos.x >= 0.0
            && local_pos.y >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y <= self.bounds.height
    }
}

/// A 200 x 200 canvas placing child 0 at (0, 0)-(150, 150) and child 1 at
/// (100, 100)-(200, 200), on top.
struct Canvas {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Canvas {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        let rects = [
            Rect::new(0.0, 0.0, 150.0, 150.0),
            Rect::new(100.0, 100.0, 100.0, 100.0),
        ];
        for (child, r) in self.children.iter_mut().zip(rects) {
            let _ = child.layout(Size::new(r.width, r.height));
            child.set_bounds(r);
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn plain(transparent: bool) -> Box<dyn Widget> {
    Box::new(Plain {
        bounds: Rect::default(),
        children: Vec::new(),
        transparent,
    })
}

/// The canvas with a scroll view (content 600 tall) behind `front`; returns the
/// app and the back view's offset.
fn back_scroller_under(front: Box<dyn Widget>) -> (App, Rc<Cell<f64>>) {
    let back_offset = Rc::new(Cell::new(0.0));
    let back = ScrollView::new(Box::new(
        SizedBox::new().with_width(130.0).with_height(600.0),
    ))
    .with_offset_cell(Rc::clone(&back_offset));
    let canvas = Canvas {
        bounds: Rect::default(),
        children: vec![Box::new(back), front],
    };
    let mut app = App::new(Box::new(canvas));
    app.layout(Size::new(200.0, 200.0));
    (app, back_offset)
}

/// A wheel turned towards the user at canvas point (x, y), y up.
fn wheel_down(app: &mut App, x: f64, y: f64) {
    app.on_mouse_wheel_xy_mods(x, 200.0 - y, 0.0, -1.0, Modifiers::default());
}

#[test]
fn a_window_in_front_keeps_the_wheel_from_the_scroll_view_behind_it() {
    let (mut app, back) = back_scroller_under(plain(false));
    let start = back.get();
    // (120, 120) is inside both; the front one is on top there.
    wheel_down(&mut app, 120.0, 120.0);
    assert_eq!(back.get(), start);
}

#[test]
fn the_scroll_view_behind_scrolls_where_nothing_covers_it() {
    let (mut app, back) = back_scroller_under(plain(false));
    let start = back.get();
    // (50, 50) is only over the back one.
    wheel_down(&mut app, 50.0, 50.0);
    assert_ne!(back.get(), start);
}

#[test]
fn a_front_scroll_view_at_its_limit_still_keeps_the_wheel_from_its_sibling() {
    // The front view has nothing more to show in the wheel's direction, so it
    // leaves the delta for its ancestors - but the sibling it covers is not an
    // ancestor and must not take it.
    let front_offset = Rc::new(Cell::new(0.0));
    let mut front = ScrollView::new(Box::new(
        SizedBox::new().with_width(80.0).with_height(400.0),
    ))
    .with_offset_cell(Rc::clone(&front_offset));
    front.set_scroll_offset(100_000.0);
    let (mut app, back) = back_scroller_under(Box::new(front));
    app.layout(Size::new(200.0, 200.0));
    let front_at_end = front_offset.get();
    let back_start = back.get();
    assert!(
        front_at_end > 0.0 && front_at_end < 100_000.0,
        "{front_at_end}"
    );

    wheel_down(&mut app, 120.0, 120.0);

    assert_eq!(front_offset.get(), front_at_end);
    assert_eq!(back.get(), back_start);
}

#[test]
fn a_widget_that_declines_the_point_lets_the_wheel_through() {
    let (mut app, back) = back_scroller_under(plain(true));
    let start = back.get();
    wheel_down(&mut app, 120.0, 120.0);
    assert_ne!(back.get(), start);
}

#[test]
fn a_nested_scroll_view_at_its_limit_bubbles_to_the_outer_one() {
    let inner_offset = Rc::new(Cell::new(0.0));
    let mut inner = ScrollView::new(Box::new(
        SizedBox::new().with_width(100.0).with_height(400.0),
    ))
    .with_offset_cell(Rc::clone(&inner_offset));
    inner.set_scroll_offset(100_000.0);
    let column = FlexColumn::new()
        .add(Box::new(Fixed::new(150.0, 100.0, Some(Box::new(inner)))))
        .add(Box::new(
            SizedBox::new().with_width(150.0).with_height(600.0),
        ));
    let outer_offset = Rc::new(Cell::new(0.0));
    let outer = ScrollView::new(Box::new(column)).with_offset_cell(Rc::clone(&outer_offset));
    let mut app = App::new(Box::new(outer));
    app.layout(Size::new(200.0, 200.0));
    let inner_at_end = inner_offset.get();
    let outer_start = outer_offset.get();
    assert!(
        inner_at_end > 0.0 && inner_at_end < 100_000.0,
        "{inner_at_end}"
    );

    // the pointer over the inner view: the column's first 100 rows, at the top
    wheel_down(&mut app, 75.0, 150.0);

    assert_eq!(inner_offset.get(), inner_at_end);
    assert_ne!(outer_offset.get(), outer_start);
}
