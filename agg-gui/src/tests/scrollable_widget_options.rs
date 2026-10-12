//! agg-sharp's `Tests/Agg.Tests/Agg.UI/ScrollableWidgetOptionsTests.cs`, on
//! [`ScrollView`]: stick-to-bottom, the offset from the top surviving a
//! resize, horizontal scrolling being opt-in, and Shift turning the wheel
//! sideways.
//!
//! agg-sharp lays out on every size assignment; agg-gui lays out when asked,
//! so each "content.Height = ..." or "scroll.Height = ..." is followed by a
//! layout at the new size. agg-sharp's `ScrollOffsetFromTop` is
//! [`ScrollView::scroll_offset`]; `ScrollOffsetFromLeft` is the horizontal
//! offset cell.
//!
//! `AScrollSetDuringBoundsChangedSurvivesTheResize` and
//! `AnXOnlyScrollDuringBoundsChangedKeepsTheOffsetFromTop` are behaviour
//! tests: agg-gui has no `BoundsChanged` event, so the scroll request is made
//! just before the resizing layout, which is when a `BoundsChanged` handler
//! runs. `HorizontalScrollIsOffAndAddsNoChildrenByDefault` checks the
//! default leaves wide content unscrollable sideways and the view's one
//! child alone (agg-gui's bars are painted, never children).
//! `HorizontalScrollShowsABarThatScrollsAndReservesItsHeight` drives a
//! [`ScrollBarStyle::solid`] bar (agg-sharp's bar always reserves its room)
//! through the view's own mouse events, and reads the bar from
//! [`ScrollView::horizontal_bar_report`]: agg-sharp's `ScrollBarWidth` is the
//! style's `bar_width`, and the room reserved at the bottom (C#'s
//! `ScrollArea.DeviceMargin.Bottom`) is the bar with its inner and outer
//! margins.

use std::cell::Cell;
use std::rc::Rc;

use super::scroll_fixtures::Resizable;
use crate::geometry::Point;
use crate::{
    Event, EventResult, Modifiers, MouseButton, Rect, ScrollBarStyle, ScrollView, Size, Widget,
};

struct Made {
    scroll: ScrollView,
    content: Rc<Cell<Size>>,
    h_offset: Rc<Cell<f64>>,
    h_max: Rc<Cell<f64>>,
}

/// A 200 x 100 view over content of the given size, laid out.
fn make(content_width: f64, content_height: f64, horizontal: bool) -> Made {
    let (content, size) = Resizable::new(content_width, content_height);
    let h_offset = Rc::new(Cell::new(0.0));
    let h_max = Rc::new(Cell::new(0.0));
    let mut scroll = ScrollView::new(Box::new(content))
        .horizontal(horizontal)
        .with_h_offset_cell(Rc::clone(&h_offset))
        .with_h_max_scroll_cell(Rc::clone(&h_max));
    lay_out(&mut scroll, 200.0, 100.0);
    Made {
        scroll,
        content: size,
        h_offset,
        h_max,
    }
}

fn lay_out(scroll: &mut ScrollView, width: f64, height: f64) {
    scroll.set_bounds(Rect::new(0.0, 0.0, width, height));
    let _ = scroll.layout(Size::new(width, height));
}

fn set_content_height(m: &mut Made, height: f64) {
    let w = m.content.get().width;
    m.content.set(Size::new(w, height));
    let b = m.scroll.bounds();
    lay_out(&mut m.scroll, b.width, b.height);
}

fn shifted_wheel_down() -> Event {
    Event::MouseWheel {
        pos: Point::new(50.0, 50.0),
        delta_y: -1.0,
        delta_x: 0.0,
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    }
}

#[test]
fn stick_to_bottom_follows_growing_content_until_scrolled_away() {
    let (content, size) = Resizable::new(100.0, 50.0);
    let mut scroll = ScrollView::new(Box::new(content)).with_stick_to_bottom(true);
    lay_out(&mut scroll, 200.0, 100.0);
    let mut m = Made {
        scroll,
        content: size,
        h_offset: Rc::new(Cell::new(0.0)),
        h_max: Rc::new(Cell::new(0.0)),
    };

    set_content_height(&mut m, 300.0);
    assert!(m.scroll.max_scroll_value() > 0.0);
    assert_eq!(m.scroll.scroll_offset(), m.scroll.max_scroll_value());

    set_content_height(&mut m, 400.0);
    assert_eq!(m.scroll.scroll_offset(), m.scroll.max_scroll_value());

    // scrolled away from the bottom: growth leaves the offset alone
    m.scroll.set_scroll_offset(50.0);
    set_content_height(&mut m, 500.0);
    assert_eq!(m.scroll.scroll_offset(), 50.0);

    // back at the bottom: it follows again
    let max = m.scroll.max_scroll_value();
    m.scroll.set_scroll_offset(max);
    set_content_height(&mut m, 600.0);
    assert_eq!(m.scroll.scroll_offset(), m.scroll.max_scroll_value());

    // and the view shrinking keeps it at the bottom
    lay_out(&mut m.scroll, 200.0, 80.0);
    assert_eq!(m.scroll.scroll_offset(), m.scroll.max_scroll_value());
}

#[test]
fn horizontal_scroll_is_off_and_adds_no_children_by_default() {
    let mut m = make(600.0, 50.0, false);
    assert_eq!(m.h_max.get(), 0.0);
    let _ = m.scroll.on_event(&Event::MouseWheel {
        pos: Point::new(50.0, 50.0),
        delta_y: 0.0,
        delta_x: -1.0,
        modifiers: Modifiers::default(),
    });
    assert_eq!(m.h_offset.get(), 0.0);
    assert_eq!(m.scroll.children().len(), 1);
}

#[test]
fn horizontal_scroll_shows_a_bar_that_scrolls_and_reserves_its_height() {
    let (content, size) = Resizable::new(600.0, 50.0);
    let h_offset = Rc::new(Cell::new(0.0));
    let h_max = Rc::new(Cell::new(0.0));
    let style = ScrollBarStyle::solid();
    let mut scroll = ScrollView::new(Box::new(content))
        .horizontal(true)
        .with_style(style)
        .with_h_offset_cell(Rc::clone(&h_offset))
        .with_h_max_scroll_cell(Rc::clone(&h_max));
    lay_out(&mut scroll, 200.0, 100.0);

    let bar = scroll.horizontal_bar_report();
    assert!(bar.shown);
    assert_eq!(bar.track.height, style.bar_width);
    let room = style.bar_width + style.inner_margin + style.outer_margin;
    assert!((bar.reserved - room).abs() < 1e-9, "{}", bar.reserved);
    assert_eq!(h_max.get(), 400.0);

    // Dragging the thumb the whole free track scrolls to the right end.
    let thumb = bar.thumb.expect("an overflowing bar has a thumb");
    let grab = Point::new(thumb.x + thumb.width / 2.0, thumb.y + thumb.height / 2.0);
    let left = |pos| Event::MouseDown {
        pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    };
    let up = |pos| Event::MouseUp {
        pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    };
    assert!(scroll.on_event(&left(grab)).is_consumed());
    let to = Point::new(grab.x + bar.track.width, grab.y);
    let _ = scroll.on_event(&Event::MouseMove { pos: to });
    let _ = scroll.on_event(&up(to));
    assert_eq!(h_offset.get(), 400.0);

    // Clicking the track to the left of the thumb pages back towards the start.
    let track = scroll.horizontal_bar_report().track;
    let click = Point::new(1.0, track.y + track.height / 2.0);
    let _ = scroll.on_event(&left(click));
    let _ = scroll.on_event(&up(click));
    assert!(h_offset.get() < 400.0, "{}", h_offset.get());

    // Content that fits hides the bar and gives its room back.
    size.set(Size::new(100.0, 50.0));
    lay_out(&mut scroll, 200.0, 100.0);
    assert_eq!(scroll.children()[0].bounds().x, 0.0);
    let bar = scroll.horizontal_bar_report();
    assert!(!bar.shown);
    assert_eq!(bar.reserved, 0.0);
}

#[test]
fn shift_wheel_scrolls_sideways_only_when_opted_in() {
    let mut off = make(600.0, 50.0, false);
    let _ = off.scroll.on_event(&shifted_wheel_down());
    assert_eq!(off.h_offset.get(), 0.0);

    let mut on = make(600.0, 50.0, true);
    let result = on.scroll.on_event(&shifted_wheel_down());
    assert!(on.h_offset.get() > 0.0);
    assert!(result.is_consumed());
}

#[test]
fn a_scroll_set_during_bounds_changed_survives_the_resize() {
    let mut m = make(100.0, 500.0, false);
    assert_eq!(m.scroll.scroll_offset(), 0.0);
    m.scroll.set_scroll_offset(120.0);
    lay_out(&mut m.scroll, 200.0, 90.0);
    assert_eq!(m.scroll.scroll_offset(), 120.0);
}

#[test]
fn an_x_only_scroll_during_bounds_changed_keeps_the_offset_from_top() {
    let mut m = make(600.0, 500.0, true);
    m.scroll.set_scroll_offset(100.0);
    m.h_offset.set(50.0);
    lay_out(&mut m.scroll, 200.0, 100.0);
    assert_eq!(m.h_offset.get(), 50.0);

    // the x-only request made as the view resizes
    m.h_offset.set(0.0);
    lay_out(&mut m.scroll, 400.0, 150.0);
    assert_eq!(m.scroll.scroll_offset(), 100.0);
    assert_eq!(m.h_offset.get(), 0.0);
}

#[test]
fn shift_wheel_leaves_the_event_alone_when_nothing_is_hidden_sideways() {
    let mut m = make(100.0, 500.0, true);
    let result = m.scroll.on_event(&shifted_wheel_down());
    // the wheel scrolled down, as it would without Shift, and took nothing
    // sideways
    assert_eq!(m.h_offset.get(), 0.0);
    assert!(m.scroll.scroll_offset() > 0.0);
    assert_eq!(result, EventResult::Consumed);
}

#[test]
fn a_resize_with_no_scroll_request_keeps_the_offset_from_top() {
    let mut m = make(100.0, 500.0, false);
    m.scroll.set_scroll_offset(70.0);
    lay_out(&mut m.scroll, 200.0, 100.0);
    lay_out(&mut m.scroll, 200.0, 90.0);
    assert_eq!(m.scroll.scroll_offset(), 70.0);
}

#[test]
fn without_stick_to_bottom_growing_content_keeps_the_top_in_view() {
    let mut m = make(100.0, 50.0, false);
    set_content_height(&mut m, 300.0);
    assert_eq!(m.scroll.scroll_offset(), 0.0);
}
