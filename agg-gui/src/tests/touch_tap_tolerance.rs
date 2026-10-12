//! agg-sharp `TouchTapToleranceTests` (Tests/Agg.Tests/Agg.UI/
//! TouchTapToleranceTests.cs): a finger's tap tolerance is 8 points, so it
//! scales with the display scale. On a devicePixelRatio 3.5 phone 8 device
//! pixels is about 2 CSS pixels, and ordinary finger jitter turned every tap
//! into a drag, so buttons outside a scroll area never clicked.
//!
//! C#'s `SystemWindow.SetDisplayScale` is agg-gui's
//! [`crate::set_device_scale`]. C# lays the window out in device pixels;
//! agg-gui lays it out in logical units, so every C# size here is divided by
//! the scale, while touches stay in the device pixels the shells deliver.
//! A scroll offset is a logical length, and agg-gui's `ScrollView` snaps it
//! to whole logical units, so C#'s 40 device-pixel pan at 3.5 is
//! `(40 / 3.5).round()` logical units here (10 device pixels at 1x is 10).

use std::cell::Cell;
use std::rc::Rc;

use super::touch_pan_fixtures::{reset, Board, Counts, Finger, Probe};
use crate::widgets::scroll_view::ScrollBarStyle;
use crate::{App, Rect, ScrollView, Size};

const WINDOW: f64 = 400.0;
const FINGERS: Finger = Finger { height: WINDOW };

struct View {
    app: App,
    counts: Rc<Counts>,
    offset: Rc<Cell<f64>>,
}

/// A 400 x 400 window: a 200 x 80 button at (20..220, 300..380) outside any
/// scroll area, and below it a 400 x 200 scroll area over 1000 of content
/// (device pixels).
fn make(display_scale: f64) -> View {
    reset(display_scale);
    let s = display_scale;
    let counts = Rc::new(Counts::default());
    let offset = Rc::new(Cell::new(0.0));
    let scroll = ScrollView::new(Box::new(Board::new(400.0 / s, 1000.0 / s)))
        .with_style(ScrollBarStyle::solid())
        .with_offset_cell(Rc::clone(&offset));
    let root = Board::new(WINDOW / s, WINDOW / s)
        .place(
            Rect::new(20.0 / s, 300.0 / s, 200.0 / s, 80.0 / s),
            Box::new(Probe::new(Rc::clone(&counts))),
        )
        .place(Rect::new(0.0, 0.0, 400.0 / s, 200.0 / s), Box::new(scroll));
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(WINDOW, WINDOW));
    View {
        app,
        counts,
        offset,
    }
}

#[test]
fn on_a_dense_phone_a_tap_with_finger_jitter_clicks_a_button_outside_any_scroll_area() {
    // 20 device px at DPR 3.5 is under 6 points - a still finger, not a drag
    let mut v = make(3.5);
    FINGERS.drag(&mut v.app, (100.0, 320.0), (100.0, 340.0));
    assert_eq!(v.counts.clicks.get(), 1);
}

#[test]
fn on_a_dense_phone_a_drag_past_the_tolerance_still_pans() {
    let mut v = make(3.5);
    FINGERS.drag(&mut v.app, (100.0, 50.0), (100.0, 90.0));
    assert!((v.offset.get() - (40.0_f64 / 3.5).round()).abs() <= 1e-9);
}

#[test]
fn at_one_device_pixel_per_point_ten_pixels_is_a_drag() {
    let mut v = make(1.0);
    FINGERS.drag(&mut v.app, (100.0, 320.0), (100.0, 330.0));
    assert_eq!(v.counts.clicks.get(), 0);

    FINGERS.drag(&mut v.app, (100.0, 50.0), (100.0, 60.0));
    assert!((v.offset.get() - 10.0).abs() <= 1e-9);
}
