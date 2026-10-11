//! agg-sharp's `Tests/Agg.Tests/Agg.UI/HorizontalWheelRoutingTests.cs`: a
//! trackpad's sideways component reaches a widget deep in the tree, a widget
//! that uses it takes it from its ancestors, and a [`ScrollView`] scrolls
//! sideways only where something is hidden off the sides, taking the delta
//! only when the content moved.
//!
//! agg-sharp's `ScrollPosition.X` is where the content sits (negative = moved
//! left); agg-gui's horizontal offset is how far the view has travelled
//! right, so `ScrollPosition.X == -h_offset`. Distances are logical units:
//! agg-sharp's `* GuiWidget.DeviceScale` is the device-pixel conversion
//! agg-gui does in its shells. Wheel units convert with
//! [`wheel_units_to_notches`]. agg-sharp's "the event's `WheelDeltaX` is now
//! 0" is, in agg-gui, what the ancestors receive (a [`Recorder`] above the
//! widget) or the [`EventResult`] of a direct `on_event`.
//!
//! `TheClonedEventKeepsBothWheelAxes` tests agg-sharp's re-basing
//! constructor; agg-gui re-bases in the dispatcher, so it is checked through
//! what a child placed off the origin receives.

use std::cell::Cell;
use std::rc::Rc;

use super::scroll_fixtures::{wheel_units_to_notches, Fixed, Recorder};
use super::SizedBox;
use crate::geometry::Point;
use crate::{App, Event, EventResult, Modifiers, Rect, ScrollView, Size, Widget};

/// A 200 x 200 app around `root`; the wheel at canvas point (x, y), y up.
fn app_200(root: Box<dyn Widget>) -> App {
    let mut app = App::new(root);
    app.layout(Size::new(200.0, 200.0));
    app
}

fn wheel_at(app: &mut App, x: f64, y: f64, units_x: f64, units_y: f64) {
    app.on_mouse_wheel_xy_mods(
        x,
        200.0 - y,
        wheel_units_to_notches(units_x),
        wheel_units_to_notches(units_y),
        Modifiers::default(),
    );
}

fn sideways_scroll(units_x: f64) -> Event {
    Event::MouseWheel {
        pos: Point::new(100.0, 100.0),
        delta_y: 0.0,
        delta_x: wheel_units_to_notches(units_x),
        modifiers: Modifiers::default(),
    }
}

/// A panel whose content is twice as wide as the view; returns the view and
/// its horizontal offset.
fn wide_scrollable() -> (ScrollView, Rc<Cell<f64>>) {
    let h = Rc::new(Cell::new(0.0));
    let mut scroll = ScrollView::new(Box::new(
        SizedBox::new().with_width(400.0).with_height(100.0),
    ))
    .horizontal(true)
    .with_h_offset_cell(Rc::clone(&h));
    scroll.set_bounds(Rect::new(0.0, 0.0, 200.0, 200.0));
    let _ = scroll.layout(Size::new(200.0, 200.0));
    (scroll, h)
}

/// agg-sharp's `ScrollPosition.X`.
fn scroll_x(h: &Rc<Cell<f64>>) -> f64 {
    -h.get()
}

#[test]
fn the_cloned_event_keeps_both_wheel_axes() {
    // The child sits off the parent's origin, so the dispatcher re-bases the
    // event into its coordinates; both axes must survive that.
    let leaf = Recorder::new(
        Box::new(Fixed::new(1.0, 1.0, None)),
        Rect::new(0.0, 0.0, 1.0, 1.0),
        false,
    );
    let leaf_seen = Rc::clone(&leaf.seen);
    let root = Recorder::new(Box::new(leaf), Rect::new(3.0, 4.0, 100.0, 100.0), false);
    let mut app = app_200(Box::new(root));

    app.on_mouse_wheel_xy_mods(10.0, 200.0 - 20.0, -45.0, 120.0, Modifiers::default());

    assert_eq!(leaf_seen.get(), Some((-45.0, 120.0)));
}

#[test]
fn a_child_sees_the_horizontal_wheel_and_can_consume_it() {
    let child = Recorder::new(
        Box::new(Fixed::new(1.0, 1.0, None)),
        Rect::new(0.0, 0.0, 1.0, 1.0),
        true,
    );
    let child_seen = Rc::clone(&child.seen);
    let parent = Recorder::new(Box::new(child), Rect::new(50.0, 50.0, 100.0, 100.0), false);
    let parent_seen = Rc::clone(&parent.seen);
    let mut app = app_200(Box::new(parent));

    wheel_at(&mut app, 75.0, 75.0, -30.0, 120.0);

    assert_eq!(
        child_seen.get(),
        Some((wheel_units_to_notches(-30.0), wheel_units_to_notches(120.0)))
    );
    // The child ate the horizontal component, so the parent (a scroll panel,
    // in the real case) is told - otherwise the same gesture would be acted
    // on twice.
    assert_eq!(
        parent_seen.get(),
        Some((0.0, wheel_units_to_notches(120.0)))
    );
}

#[test]
fn a_scroll_panel_wider_than_its_view_scrolls_sideways_and_takes_the_delta() {
    let (mut scrollable, h) = wide_scrollable();

    let result = scrollable.on_event(&sideways_scroll(-30.0));

    // -30 wheel units is 6 pixels of finger travel, and negative means the
    // content moves left.
    assert!((scroll_x(&h) - -6.0).abs() < 0.001, "{}", scroll_x(&h));
    // it scrolled, so nothing above may scroll on the same gesture
    assert!(result.is_consumed());
}

#[test]
fn the_content_follows_the_fingers() {
    let (mut scrollable, h) = wide_scrollable();

    let _ = scrollable.on_event(&sideways_scroll(-50.0));
    let after_fingers_left = scroll_x(&h);
    assert!(after_fingers_left < 0.0);

    let _ = scrollable.on_event(&sideways_scroll(20.0));
    assert!(scroll_x(&h) > after_fingers_left);
}

#[test]
fn the_content_stops_at_either_end() {
    let (mut scrollable, h) = wide_scrollable();

    // far past the right hand end
    let _ = scrollable.on_event(&sideways_scroll(-100_000.0));
    let at_the_end = scroll_x(&h);

    // the content's right edge has come in to the view's, and no further
    assert!(at_the_end < 0.0);
    let content = scrollable.children()[0].bounds();
    assert!(content.x + content.width >= scrollable.bounds().width - 0.001);

    // and having run out, the gesture is left for an ancestor rather than
    // swallowed
    let past_the_end = scrollable.on_event(&sideways_scroll(-100_000.0));
    assert!((scroll_x(&h) - at_the_end).abs() < 0.001);
    assert_eq!(past_the_end, EventResult::Ignored);

    // back the other way stops at the start
    let _ = scrollable.on_event(&sideways_scroll(100_000.0));
    assert!(scroll_x(&h).abs() < 0.001);
}

#[test]
fn a_panel_with_nothing_hidden_sideways_leaves_the_delta_alone() {
    // Nothing is clipped off the sides, so this panel cannot use the gesture
    // - and must not eat it, or a scroll panel outside it that could would
    // never see it.
    let h = Rc::new(Cell::new(0.0));
    let mut scrollable = ScrollView::new(Box::new(
        SizedBox::new().with_width(100.0).with_height(400.0),
    ))
    .horizontal(true)
    .with_h_offset_cell(Rc::clone(&h));
    scrollable.set_bounds(Rect::new(0.0, 0.0, 200.0, 200.0));
    let _ = scrollable.layout(Size::new(200.0, 200.0));
    let starting_scroll = scroll_x(&h);

    let result = scrollable.on_event(&sideways_scroll(-30.0));

    assert!((scroll_x(&h) - starting_scroll).abs() < 0.001);
    assert_eq!(result, EventResult::Ignored);
}

#[test]
fn an_inner_panel_that_only_scrolls_up_and_down_hands_the_gesture_out() {
    // A tall inner panel (vertical only: its content is held to its width)
    // inside a wide outer one. The wheel is over the inner panel, which has
    // plenty to scroll vertically and nothing sideways, so the sideways
    // component has to survive it and reach the panel that can act on it.
    let inner_h = Rc::new(Cell::new(0.0));
    let inner = ScrollView::new(Box::new(
        SizedBox::new().with_width(400.0).with_height(800.0),
    ))
    .with_h_offset_cell(Rc::clone(&inner_h));
    let outer_h = Rc::new(Cell::new(0.0));
    let outer = ScrollView::new(Box::new(Fixed::new(400.0, 200.0, Some(Box::new(inner)))))
        .horizontal(true)
        .with_h_offset_cell(Rc::clone(&outer_h));
    let root = Recorder::new(Box::new(outer), Rect::new(0.0, 0.0, 200.0, 200.0), false);
    let root_seen = Rc::clone(&root.seen);
    let mut app = app_200(Box::new(root));

    wheel_at(&mut app, 100.0, 100.0, -30.0, 0.0);

    assert!(scroll_x(&inner_h).abs() < 0.001);
    assert!(
        (scroll_x(&outer_h) - -6.0).abs() < 0.001,
        "{}",
        scroll_x(&outer_h)
    );
    // nothing above the outer panel sees it
    assert_eq!(root_seen.get(), None);
}
