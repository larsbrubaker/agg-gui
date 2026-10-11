//! agg-sharp's `Tests/Agg.Tests/Agg.UI/TrackpadScrollAxisTests.cs`, the cases
//! on `WheelDeltaMath.ScrollingDeltaToWheelDelta`, run against
//! [`crate::trackpad_pinch::scrolling_delta_to_wheel_delta`] in the C#'s Win32
//! wheel units (120 per detent).
//!
//! The rest of the class lives where agg-gui does that work, the native shell
//! (`agg-gui-shell/src/input.rs` tests):
//! `EveryWheelNotchIsOneDetentNoMatterHowFastItIsTurned` (also checked here on
//! the shared conversion), `ADiagonalScrollFillsBothAxes`,
//! `APurelyVerticalScrollLeavesTheHorizontalAxisAtZero` and
//! `AFingerDragMovesTheContentTheSameDistanceOnEveryDisplay`.
//! `ATrackpadScrollTellsTheWidgetItAlreadyCarriesTheDisplayScale` is N/A:
//! agg-gui has no `WheelDeltaIsPreciseScroll` flag because its shell converts
//! a trackpad's travel to logical pixels before the wheel is dispatched, so no
//! consumer has a display scale left to apply.

use crate::trackpad_pinch::scrolling_delta_to_wheel_delta;

#[test]
fn a_trackpad_scroll_tracks_the_fingers_one_to_one() {
    assert_eq!(scrolling_delta_to_wheel_delta(10.0, true, 2.0), 100);
    assert_eq!(scrolling_delta_to_wheel_delta(10.0, true, 1.0), 50);
}

#[test]
fn a_real_wheel_still_clicks_in_detents() {
    assert_eq!(scrolling_delta_to_wheel_delta(1.0, false, 2.0), 120);
    assert_eq!(scrolling_delta_to_wheel_delta(-3.0, false, 1.0), -120);
}

#[test]
fn every_wheel_notch_is_one_detent_no_matter_how_fast_it_is_turned() {
    assert_eq!(scrolling_delta_to_wheel_delta(6.0, false, 1.0), 120);
    assert_eq!(scrolling_delta_to_wheel_delta(0.1, false, 1.0), 120);
    assert_eq!(scrolling_delta_to_wheel_delta(-0.1, false, 1.0), -120);
    assert_eq!(scrolling_delta_to_wheel_delta(0.0, false, 1.0), 0);
}

#[test]
fn the_sign_is_carried_straight_through() {
    assert!(scrolling_delta_to_wheel_delta(-10.0, true, 1.0) < 0);
    assert_eq!(scrolling_delta_to_wheel_delta(0.0, true, 1.0), 0);
}

#[test]
fn a_nonsense_delta_is_no_scroll() {
    assert_eq!(scrolling_delta_to_wheel_delta(f64::NAN, true, 1.0), 0);
    assert_eq!(scrolling_delta_to_wheel_delta(f64::INFINITY, false, 1.0), 0);
}

/// The web shell's half of `AFingerDragMovesTheContentTheSameDistanceOnEveryDisplay`:
/// a DOM pixel delta is CSS pixels (already free of the device pixel ratio),
/// so at a `ux_scale` of 2 the same finger travel is half as many logical
/// pixels, and half as many notches. Line deltas and pinches carry no size.
#[test]
fn rust_only_a_browser_trackpad_scroll_is_divided_by_ux_scale() {
    use crate::trackpad_pinch::{browser_wheel_notches, browser_wheel_notches_at_ux_scale};
    use crate::wheel::WheelDeltaMode;

    let at_one = browser_wheel_notches_at_ux_scale(20.0, 80.0, WheelDeltaMode::Pixel, false, 1.0);
    let at_two = browser_wheel_notches_at_ux_scale(20.0, 80.0, WheelDeltaMode::Pixel, false, 2.0);
    assert_eq!(at_two, (at_one.0 / 2.0, at_one.1 / 2.0));
    assert!(at_one.1 < 0.0, "DOM scroll-down is a backward wheel");
    assert_eq!(
        browser_wheel_notches(20.0, 80.0, WheelDeltaMode::Pixel, false),
        at_one
    );

    let lines = browser_wheel_notches_at_ux_scale(0.0, 3.0, WheelDeltaMode::Line, false, 2.0);
    assert_eq!(lines, (0.0, -3.0));
    let pinch = |ux| browser_wheel_notches_at_ux_scale(0.0, 10.0, WheelDeltaMode::Pixel, true, ux);
    assert_eq!(pinch(2.0), pinch(1.0));
    // A broken scale is no scale.
    assert_eq!(
        browser_wheel_notches_at_ux_scale(20.0, 80.0, WheelDeltaMode::Pixel, false, 0.0),
        at_one
    );
}
