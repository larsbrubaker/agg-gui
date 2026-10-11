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
