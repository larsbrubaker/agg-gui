//! Virtual trackpad fingers: ports of agg-sharp's
//! `Tests/Agg.Tests/Agg.UI/TrackpadPinchFingersTests.cs` (the mark-routing
//! test lives in `trackpad_pinch.rs`), run against
//! [`crate::trackpad_pinch_fingers`] and the [`crate::touch_state::TouchState`]
//! recogniser the fingers feed, plus `App::on_trackpad_magnify` end to end.
//!
//! C#'s `Feed` updates `MultiTouchGesture` once per frame; agg-gui aggregates
//! once per paint, so `feed` here applies one event's frames and then
//! aggregates once, as `App` does between two paints. The C# expectations
//! hold unchanged, which is what the baseline latch in `TouchState` is for.

use crate::geometry::Point;
use crate::touch_state::{MultiTouchInfo, TouchState};
use crate::trackpad_pinch_fingers::{
    TrackpadGesturePhase, TrackpadPinchFingers, VIRTUAL_TRACKPAD_DEVICE,
};

const POINTER: Point = Point { x: 200.0, y: 150.0 };

/// Applies one event's frames to the recogniser and aggregates once.
fn feed(gesture: &mut TouchState, frames: &[Vec<Point>]) -> Option<MultiTouchInfo> {
    for frame in frames {
        gesture.apply_virtual_frame(VIRTUAL_TRACKPAD_DEVICE, frame);
    }
    gesture.update_gesture();
    gesture.current()
}

fn distance(a: Point, b: Point) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

#[test]
fn a_pinch_zooms_by_the_magnification_about_the_pointer() {
    let mut fingers = TrackpadPinchFingers::new(50.0);
    let mut gesture = TouchState::new();

    // The first event already carries a change: it is not lost to the
    // recogniser's baseline frame.
    let info = feed(
        &mut gesture,
        &fingers.magnify(POINTER, 0.2, TrackpadGesturePhase::Began),
    )
    .expect("two fingers");
    assert!((info.zoom_delta - 1.2).abs() < 1e-9, "{}", info.zoom_delta);

    let info = feed(
        &mut gesture,
        &fingers.magnify(POINTER, -0.1, TrackpadGesturePhase::Changed),
    )
    .expect("two fingers");
    assert_eq!(info.num_touches, 2);
    assert!((info.zoom_delta - 0.9).abs() < 1e-9, "{}", info.zoom_delta);
    assert!(info.rotation_delta.abs() < 1e-9);
    assert!(distance(info.translation_delta, Point::new(0.0, 0.0)) < 1e-9);
    assert!(distance(info.center_pos, POINTER) < 1e-9);
}

#[test]
fn a_rotation_turns_counter_clockwise_by_the_degrees_reported() {
    let mut fingers = TrackpadPinchFingers::new(50.0);
    let mut gesture = TouchState::new();
    feed(
        &mut gesture,
        &fingers.rotate(POINTER, 0.0, TrackpadGesturePhase::Began),
    );

    let info = feed(
        &mut gesture,
        &fingers.rotate(POINTER, 30.0, TrackpadGesturePhase::Changed),
    )
    .expect("two fingers");
    assert!((info.rotation_delta - std::f64::consts::PI / 6.0).abs() < 1e-9);
    assert!((info.zoom_delta - 1.0).abs() < 1e-9);

    // Pinch and rotate are one gesture: a magnify mid-rotation keeps the
    // angle and the anchor.
    let moved = Point::new(POINTER.x + 5.0, POINTER.y);
    let info = feed(
        &mut gesture,
        &fingers.magnify(moved, 0.5, TrackpadGesturePhase::Changed),
    )
    .expect("two fingers");
    assert!((info.zoom_delta - 1.5).abs() < 1e-9);
    assert!(info.rotation_delta.abs() < 1e-9);
    assert!(distance(info.center_pos, POINTER) < 1e-9);
}

#[test]
fn the_end_lifts_the_second_finger_at_the_pointer() {
    let mut fingers = TrackpadPinchFingers::new(50.0);
    let mut gesture = TouchState::new();
    feed(
        &mut gesture,
        &fingers.magnify(POINTER, 0.1, TrackpadGesturePhase::Began),
    );

    let end = fingers.magnify(POINTER, 0.0, TrackpadGesturePhase::Ended);
    assert_eq!(end.last(), Some(&vec![POINTER]));
    assert!(!fingers.active());
    assert!(feed(&mut gesture, &end).is_none());
    assert_eq!(gesture.active_count(), 0, "every virtual finger lifted");

    // The next pinch starts over where the pointer now is.
    let elsewhere = Point::new(20.0, 30.0);
    let info = feed(
        &mut gesture,
        &fingers.magnify(elsewhere, 0.1, TrackpadGesturePhase::Began),
    )
    .expect("two fingers");
    assert!(distance(info.center_pos, elsewhere) < 1e-9);
}

#[test]
fn a_full_pinch_in_never_collapses_the_fingers() {
    let mut fingers = TrackpadPinchFingers::new(50.0);
    let frames = fingers.magnify(POINTER, -1.5, TrackpadGesturePhase::Began);
    let last = frames.last().expect("frames");
    assert!(distance(last[1], last[0]) > 2.0);
}

// `AScenePanZoomLeavesAPinchWheelToTheFingers` is not ported: agg-gui has no
// `ScenePanZoom` widget. The same rule (a finger-following widget leaves the
// marked wheel to the fingers) is pinned by the demo ports of
// `MultiTouchWindowTests.ATrackpadPinchIsAMultiTouchZoomNotAScroll` and
// `LionWindowTests.ATrackpadPinchZoomsOnceFromItsFingersNotItsWheel`
// (demo-ui) and by `app_trackpad_magnify_reaches_widgets_as_one_gesture`.

/// `App::on_trackpad_magnify`: the wheel goes out marked, the fingers reach
/// the widget under the pointer as one `Event::MultiTouch` per paint on the
/// virtual device, and the end lifts them without driving the mouse
/// emulation (no `MouseDown`).
#[test]
fn app_trackpad_magnify_reaches_widgets_as_one_gesture() {
    use crate::{
        App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Modifiers, Rect, Size, Widget,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Recorder {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        seen: Rc<RefCell<Vec<String>>>,
    }
    impl Widget for Recorder {
        fn type_name(&self) -> &'static str {
            "Recorder"
        }
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
            let entry = match event {
                Event::MultiTouch { info } => {
                    assert_eq!(info.device_id, VIRTUAL_TRACKPAD_DEVICE);
                    format!("touch {:.6}", info.zoom_delta)
                }
                Event::MouseWheel { .. } => format!(
                    "wheel {}",
                    crate::trackpad_pinch::wheel_from_trackpad_pinch()
                ),
                Event::MouseDown { .. } => "down".to_string(),
                _ => return EventResult::Ignored,
            };
            self.seen.borrow_mut().push(entry);
            EventResult::Consumed
        }
    }

    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut app = App::new(Box::new(Recorder {
        bounds: Rect::default(),
        children: Vec::new(),
        seen: Rc::clone(&seen),
    }));
    app.layout(Size::new(400.0, 300.0));
    let paint = |app: &mut App| {
        let mut fb = Framebuffer::new(400, 300);
        let mut ctx = GfxCtx::new(&mut fb);
        app.paint(&mut ctx);
    };

    app.on_trackpad_magnify(
        200.0,
        150.0,
        0.2,
        TrackpadGesturePhase::Began,
        Modifiers::default(),
    );
    paint(&mut app);
    app.on_trackpad_magnify(
        200.0,
        150.0,
        0.0,
        TrackpadGesturePhase::Ended,
        Modifiers::default(),
    );
    paint(&mut app);

    assert_eq!(
        *seen.borrow(),
        vec![
            "wheel true".to_string(),
            "touch 1.200000".to_string(),
            "wheel true".to_string()
        ]
    );
    assert_eq!(app.active_touch_count(), 0);
}
