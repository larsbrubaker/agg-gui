//! Pinch-to-zoom as a wheel: ports of agg-sharp's
//! `Tests/Agg.Tests/Agg.UI/MagnificationWheelDeltaTests.cs` (whole class),
//! the pinch tests of `BrowserWheelTests.cs`, and
//! `TrackpadPinchFingersTests.ThePinchMarkSurvivesRoutingIntoAChild`, run
//! against [`crate::trackpad_pinch`] and `App::on_trackpad_pinch`.
//!
//! The C# expectations are in Win32 wheel units (120 per detent) and are
//! checked here in those units through the `*_wheel_delta` functions; the
//! notch functions agg-gui's events carry are those divided by 120.

use std::cell::Cell;
use std::rc::Rc;

use crate::trackpad_pinch::{
    browser_pinch_wheel_delta, browser_wheel_notches, magnification_to_notches,
    magnification_to_wheel_delta, scrolling_delta_to_wheel_delta, wheel_from_trackpad_pinch,
    WHEEL_DELTA_PER_NOTCH,
};
use crate::wheel::WheelDeltaMode;
use crate::{App, DrawCtx, Event, EventResult, Modifiers, Rect, Size, Widget};

// --- MagnificationWheelDeltaTests ------------------------------------------

#[test]
fn fingers_apart_zoom_in_and_together_zoom_out() {
    // Positive magnification is fingers moving apart, which has to come out as
    // a forward wheel, because forward is what the 3D view reads as zoom in.
    assert!(magnification_to_wheel_delta(0.05) > 0);
    assert!(magnification_to_wheel_delta(-0.05) < 0);
    assert_eq!(magnification_to_wheel_delta(0.0), 0);
}

#[test]
fn the_zoom_follows_how_far_the_fingers_moved() {
    let small = magnification_to_wheel_delta(0.02);
    let twice_as_far = magnification_to_wheel_delta(0.04);

    // A single event's magnification is a hundredth or so, so it has to
    // survive the trip to integer wheel units.
    assert!(small > 0);
    assert_eq!(twice_as_far, small * 2);

    // and a whole-gesture magnification of 1 is several wheel detents
    assert_eq!(magnification_to_wheel_delta(1.0), small * 50);
}

#[test]
fn a_nonsense_magnification_is_no_zoom() {
    assert_eq!(magnification_to_wheel_delta(f64::NAN), 0);
    assert_eq!(magnification_to_wheel_delta(f64::INFINITY), 0);
}

// --- BrowserWheelTests (the pinch tests) -----------------------------------

/// A ctrl+wheel's wheel delta in C# units, through the shell's routing.
fn ctrl_scroll(mode: WheelDeltaMode, delta_x: f64, delta_y: f64) -> (f64, f64) {
    let (dx, dy) = browser_wheel_notches(delta_x, delta_y, mode, true);
    (dx * WHEEL_DELTA_PER_NOTCH, dy * WHEEL_DELTA_PER_NOTCH)
}

#[test]
fn a_pinch_goes_through_the_magnification_conversion() {
    // 100 CSS pixels of ctrl-wheel is one whole unit of magnification.
    let whole_unit = ctrl_scroll(WheelDeltaMode::Pixel, 0.0, -100.0).1;
    assert_eq!(whole_unit, magnification_to_wheel_delta(1.0) as f64);

    // Fingers apart is a negative deltaY and has to come out as a forward wheel.
    assert!(whole_unit > 0.0);
    assert_eq!(
        ctrl_scroll(WheelDeltaMode::Pixel, 0.0, 100.0).1,
        magnification_to_wheel_delta(-1.0) as f64
    );

    // A single event of a real pinch is a few CSS pixels; it must not round away.
    assert!(ctrl_scroll(WheelDeltaMode::Pixel, 0.0, -4.0).1 > 0.0);
}

/// A pinch is one number, so no sideways travel leaks out of it. (C# also
/// asserts it is never a precise scroll; agg-gui's wheel has no precision
/// flag - every notch is a zoom step to its consumers - so that half has
/// nothing to check here.)
#[test]
fn a_pinch_is_one_axis_and_never_precise() {
    let (dx, _) = ctrl_scroll(WheelDeltaMode::Pixel, 25.0, -8.0);
    assert_eq!(dx, 0.0);
}

#[test]
fn ctrl_over_a_real_wheel_is_a_detent_of_zoom() {
    let dy = ctrl_scroll(WheelDeltaMode::Line, 0.0, -3.0).1;
    assert_eq!(dy, scrolling_delta_to_wheel_delta(3.0, false, 1.0) as f64);
    assert_eq!(browser_pinch_wheel_delta(-3.0, WheelDeltaMode::Line), 120);
}

// --- TrackpadPinchFingersTests.ThePinchMarkSurvivesRoutingIntoAChild --------

/// Leaf that records the pinch mark of every wheel it receives.
struct WheelRecorder {
    bounds: Rect,
    seen: Rc<Cell<Option<bool>>>,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for WheelRecorder {
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
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::MouseWheel { .. } = event {
            self.seen.set(Some(wheel_from_trackpad_pinch()));
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
}

/// Parent placing its one child at (5, 5), 50 x 50, as the C# test does.
struct Parent {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Parent {
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
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        self.children[0].set_bounds(Rect::new(5.0, 5.0, 50.0, 50.0));
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

#[test]
fn the_pinch_mark_survives_routing_into_a_child() {
    let seen = Rc::new(Cell::new(None));
    let child = WheelRecorder {
        bounds: Rect::default(),
        seen: Rc::clone(&seen),
        children: Vec::new(),
    };
    let mut app = App::new(Box::new(Parent {
        bounds: Rect::default(),
        children: vec![Box::new(child)],
    }));
    app.layout(Size::new(100.0, 100.0));

    // (20, 20) Y-up is inside the child; screen Y is down, so 100 - 20 = 80.
    app.on_trackpad_pinch(20.0, 80.0, 0.2, Modifiers::default());
    assert_eq!(seen.get(), Some(true));
    // The mark is only for the pinch's own dispatch.
    assert!(!wheel_from_trackpad_pinch());

    seen.set(None);
    app.on_mouse_wheel(20.0, 80.0, 1.0);
    assert_eq!(seen.get(), Some(false));
}

/// Rust-only: the pinch reaches widgets as the notches the shared
/// conversion gives (0.2 magnification = 120 wheel units = one notch).
#[test]
fn rust_only_a_pinch_wheel_carries_the_magnification_notches() {
    assert_eq!(magnification_to_notches(0.2), 1.0);
    assert_eq!(magnification_to_notches(0.01), 6.0 / 120.0);
}

/// Rust-only: C#'s `Math.Round` is banker's rounding, and the conversions
/// keep it (2.5 wheel units rounds to 2, 7.5 to 8, -2.5 to -2).
#[test]
fn rust_only_wheel_units_round_half_to_even() {
    assert_eq!(scrolling_delta_to_wheel_delta(0.5, true, 1.0), 2);
    assert_eq!(scrolling_delta_to_wheel_delta(0.5, true, 3.0), 8);
    assert_eq!(scrolling_delta_to_wheel_delta(-0.5, true, 1.0), -2);
}
