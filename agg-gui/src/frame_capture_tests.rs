//! Tests for the host-agnostic frame-capture queue (`frame_capture.rs`): the
//! request asks for a frame, is taken once, delivers once, and converts its
//! window-coordinate region to framebuffer pixels.

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::animation;

type Seen = Rc<RefCell<Vec<FrameCaptureResult>>>;

fn recorder() -> (Seen, impl FnOnce(FrameCaptureResult) + 'static) {
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    (seen, move |r| sink.borrow_mut().push(r))
}

#[test]
fn asking_for_a_capture_asks_an_idle_app_for_a_frame() {
    animation::clear_draw_request();
    let _ = take_frame_capture_requests();
    assert!(!animation::peek_draw_signals().0);

    let (_seen, cb) = recorder();
    request_frame_capture(None, cb);

    assert!(
        animation::peek_draw_signals().0,
        "an idle app must draw the frame the capture waits for"
    );
    assert!(has_pending_frame_capture());
    let _ = take_frame_capture_requests();
}

#[test]
fn requests_are_taken_once_and_delivered_once() {
    let _ = take_frame_capture_requests();
    let (seen, cb) = recorder();
    let region = Rect::new(1.0, 2.0, 3.0, 4.0);
    request_frame_capture(Some(region), cb);

    let taken = take_frame_capture_requests();
    assert_eq!(taken.len(), 1);
    assert!(!has_pending_frame_capture(), "the frame took the request");
    assert!(take_frame_capture_requests().is_empty());
    assert_eq!(taken[0].region(), Some(region));

    let frame = CapturedFrame {
        rgba: vec![1, 2, 3, 4],
        width: 1,
        height: 1,
    };
    for req in taken {
        req.deliver(Ok(frame.clone()));
    }
    assert_eq!(*seen.borrow(), vec![Ok(frame)]);
}

#[test]
fn a_request_dropped_undelivered_reports_unavailable() {
    let _ = take_frame_capture_requests();
    let (seen, cb) = recorder();
    request_frame_capture(None, cb);
    drop(take_frame_capture_requests());
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1, "the callback runs exactly once");
    assert!(matches!(seen[0], Err(FrameCaptureError::Unavailable(_))));
}

#[test]
fn no_region_is_the_whole_frame() {
    assert_eq!(
        region_in_pixels(None, 2.0, 640, 480),
        Ok(PixelRegion {
            x: 0,
            y: 0,
            width: 640,
            height: 480
        })
    );
}

#[test]
fn a_region_is_scaled_and_flipped_to_top_down_pixels() {
    // A 100x50 logical frame at scale 2 is 200x100 pixels. The logical rect
    // (10, 5)-(40, 25) Y-up is pixels x 20..80 and, counted from the top,
    // y 100-50=50 .. 100-10=90.
    assert_eq!(
        region_in_pixels(Some(Rect::new(10.0, 5.0, 30.0, 20.0)), 2.0, 200, 100),
        Ok(PixelRegion {
            x: 20,
            y: 50,
            width: 60,
            height: 40
        })
    );
}

#[test]
fn a_region_keeps_partly_covered_edge_pixels_and_is_clipped_to_the_frame() {
    assert_eq!(
        region_in_pixels(Some(Rect::new(0.5, 0.5, 1.0, 1.0)), 1.0, 4, 4),
        Ok(PixelRegion {
            x: 0,
            y: 2,
            width: 2,
            height: 2
        })
    );
    assert_eq!(
        region_in_pixels(Some(Rect::new(-10.0, -10.0, 1000.0, 1000.0)), 1.0, 8, 6),
        Ok(PixelRegion {
            x: 0,
            y: 0,
            width: 8,
            height: 6
        })
    );
}

#[test]
fn an_empty_or_off_screen_region_is_an_error() {
    for r in [
        Rect::new(0.0, 0.0, 0.0, 10.0),
        Rect::new(0.0, 0.0, 10.0, -1.0),
        Rect::new(100.0, 0.0, 10.0, 10.0),
        Rect::new(0.0, -20.0, 10.0, 10.0),
        Rect::new(f64::NAN, 0.0, 10.0, 10.0),
    ] {
        assert_eq!(
            region_in_pixels(Some(r), 1.0, 50, 50),
            Err(FrameCaptureError::RegionEmpty),
            "{r:?}"
        );
    }
    assert_eq!(
        region_in_pixels(None, 1.0, 0, 50),
        Err(FrameCaptureError::RegionEmpty)
    );
}
