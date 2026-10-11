//! winit input → agg-gui input.
//!
//! The mapping of button/key/modifier *types* lives in
//! `agg_gui::winit_adapter`; what lives here is the behaviour the adapter
//! can't know about: wheel-delta conversion and the shift→horizontal remap,
//! and raw touch translation into `agg_gui::shell_input::ForwarderEvent`s.
//! Both are pure enough to test without a window.

use agg_gui::shell_input::ForwarderEvent;
use agg_gui::wheel::{to_notches, WheelDeltaMode};
use agg_gui::Modifiers;
use winit::event::{MouseScrollDelta, Touch, TouchPhase};

/// Convert a winit wheel delta into agg-gui `(dx, dy)` wheel lines.
///
/// DO NOT negate these values. winit's `MouseScrollDelta` is already in the
/// OS's scroll-direction convention — on Windows the FlipFlopWheel registry
/// setting (and any per-driver "natural scroll" toggle) flips the sign of
/// `WM_MOUSEWHEEL` before winit sees it; on macOS `NSEvent`'s
/// `scrollingDeltaY` honours System Settings → Trackpad → Natural Scrolling.
/// Passing the value straight through is what makes the app respect the OS
/// preference for both old-school and natural-scroll users. This has been
/// regressed multiple times by contributors "fixing" how scrolling feels on
/// their machine; if it feels backwards, the OS preference is the source of
/// truth — don't add a sign flip here.
///
/// `shift` remaps a pure-vertical wheel to horizontal, the convention every
/// desktop toolkit uses for a mouse with no tilt wheel. A device that already
/// reports a horizontal component is left alone.
///
/// `physical_per_logical` is the App's total scale (window scale factor x
/// `ux_scale`). A trackpad's `PixelDelta` is physical pixels of finger travel;
/// dividing it by that scale first makes the content travel exactly as far as
/// the fingers on every display, whatever the user's UI size (agg-sharp's
/// `TrackpadScrollAxisTests.AFingerDragMovesTheContentTheSameDistanceOnEveryDisplay`).
/// Forwarding raw physical pixels scrolled a Retina trackpad at twice the
/// fingers. A wheel's `LineDelta` is detents and carries no size;
/// `accelerated_lines` (macOS) makes each one signed notch, see
/// [`line_delta_notches`].
pub(crate) fn wheel_delta(
    delta: MouseScrollDelta,
    shift: bool,
    physical_per_logical: f64,
    accelerated_lines: bool,
) -> (f64, f64) {
    let scale = if physical_per_logical.is_finite() && physical_per_logical > 0.0 {
        physical_per_logical
    } else {
        1.0
    };
    let (mut dx, mut dy) = match delta {
        MouseScrollDelta::LineDelta(x, y) => (
            line_delta_notches(x as f64, accelerated_lines),
            line_delta_notches(y as f64, accelerated_lines),
        ),
        // Trackpads / precision wheels: logical pixels, then the same pixel →
        // notch conversion the web shells use (`agg_gui::wheel`), forwarded
        // fractionally so scrolling stays continuous.
        MouseScrollDelta::PixelDelta(p) => (
            to_notches(p.x / scale, WheelDeltaMode::Pixel),
            to_notches(p.y / scale, WheelDeltaMode::Pixel),
        ),
    };
    if shift && dx == 0.0 {
        dx = dy;
        dy = 0.0;
    }
    (dx, dy)
}

/// One axis of a wheel's `LineDelta` in notches.
///
/// macOS accelerates line deltas (one physical notch reports about 0.1 lines
/// turned slowly and many lines turned fast) and offers no notch count, so
/// there (`accelerated`) one event is one signed notch, as agg-sharp's
/// `WheelDeltaMath.ScrollingDeltaToWheelDelta` does for a non-precise device;
/// letting the acceleration through turned one fast notch into a huge zoom
/// jump. Elsewhere winit's line delta is already unaccelerated notches
/// (fractional for a high-resolution wheel) and passes through. A non-finite
/// delta is no scroll.
pub(crate) fn line_delta_notches(lines: f64, accelerated: bool) -> f64 {
    if !lines.is_finite() {
        0.0
    } else if accelerated {
        agg_gui::trackpad_pinch::scrolling_delta_to_wheel_delta(lines, false, 1.0) as f64
            / agg_gui::trackpad_pinch::WHEEL_DELTA_PER_NOTCH
    } else {
        lines
    }
}

/// A raw touch as the forwarder event the shell hands its `InputForwarder`.
///
/// Raw touches only: agg-gui core aggregates multi-finger gestures AND replays
/// the primary finger as mouse events (`touch_emulation.rs`), mirroring the
/// wasm shell. winit registers for touch on Windows, which suppresses the OS's
/// mouse promotion — so without this a touchscreen does nothing at all.
/// `location` is already in the same physical-pixel space as `CursorMoved`.
/// Device id is pinned to 0: winit's `DeviceId` is opaque, and telling two
/// touchscreens apart isn't worth a lossy hash of it.
pub(crate) fn touch_event(touch: Touch) -> ForwarderEvent {
    ForwarderEvent::Touch {
        phase: match touch.phase {
            TouchPhase::Started => agg_gui::TouchPhase::Start,
            TouchPhase::Moved => agg_gui::TouchPhase::Move,
            TouchPhase::Ended => agg_gui::TouchPhase::End,
            TouchPhase::Cancelled => agg_gui::TouchPhase::Cancel,
        },
        device: agg_gui::TouchDeviceId(0),
        id: agg_gui::TouchId(touch.id),
        x: touch.location.x,
        y: touch.location.y,
        force: touch.force.map(|f| f.normalized() as f32),
    }
}

/// Shift state of the current modifiers, for [`wheel_delta`].
pub(crate) fn shift_held(mods: Modifiers) -> bool {
    mods.shift
}

/// Physical-pixel cursor position relative to the window's client area,
/// queried live from the OS.
///
/// Used by the `DroppedFile` arm because winit's tracked cursor is stale
/// during an OLE drag on Windows: the OS owns the pointer, no `CursorMoved`
/// is emitted, and winit's `IDropTarget::Drop` discards the drop point.
/// Returns `None` off-Windows or when the window position is unavailable —
/// the caller falls back to the last tracked cursor position.
#[cfg(windows)]
pub(crate) fn live_cursor_in_window(window: &winit::window::Window) -> Option<(f64, f64)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut pt = POINT { x: 0, y: 0 };
    // SAFETY: GetCursorPos writes into the POINT we own; no other
    // preconditions.
    if unsafe { GetCursorPos(&mut pt) } == 0 {
        return None;
    }
    let client_origin = window.inner_position().ok()?;
    Some((
        (pt.x - client_origin.x) as f64,
        (pt.y - client_origin.y) as f64,
    ))
}

#[cfg(not(windows))]
pub(crate) fn live_cursor_in_window(_window: &winit::window::Window) -> Option<(f64, f64)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalPosition;

    #[test]
    fn line_deltas_pass_through_unnegated() {
        // The OS preference (natural scroll / FlipFlopWheel) is already baked
        // into the sign; a flip here would fight it.
        assert_eq!(
            wheel_delta(MouseScrollDelta::LineDelta(0.0, 3.0), false, 1.0, false),
            (0.0, 3.0)
        );
        assert_eq!(
            wheel_delta(MouseScrollDelta::LineDelta(0.0, -3.0), false, 1.0, false),
            (0.0, -3.0)
        );
    }

    #[test]
    fn pixel_deltas_convert_to_lines() {
        assert_eq!(
            wheel_delta(
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 40.0)),
                false,
                1.0,
                false
            ),
            (0.0, 1.0)
        );
    }

    #[test]
    fn shift_remaps_a_vertical_wheel_to_horizontal() {
        assert_eq!(
            wheel_delta(MouseScrollDelta::LineDelta(0.0, 2.0), true, 1.0, false),
            (2.0, 0.0)
        );
    }

    #[test]
    fn shift_leaves_a_device_that_already_scrolls_horizontally_alone() {
        // A tilt wheel / trackpad reporting both axes must not have its
        // vertical component stolen.
        assert_eq!(
            wheel_delta(MouseScrollDelta::LineDelta(1.0, 2.0), true, 1.0, false),
            (1.0, 2.0)
        );
    }

    /// agg-sharp `TrackpadScrollAxisTests.AFingerDragMovesTheContentTheSameDistanceOnEveryDisplay`:
    /// 10 points of finger travel is 10 points of content travel - 10 device
    /// pixels at 1x, 20 at 2x - and the user's UI size (1.6) does not enter.
    /// winit's `PixelDelta` is physical, so a 2x display reports 20 for 10
    /// points; a `ScrollView` scrolls `PIXELS_PER_NOTCH` logical units per notch.
    #[test]
    fn a_finger_drag_moves_the_content_the_same_distance_on_every_display() {
        use agg_gui::wheel::PIXELS_PER_NOTCH;
        for (backing, ux) in [(1.0, 1.6), (2.0, 1.6), (2.0, 1.0)] {
            let total = backing * ux;
            let finger_px = 10.0 * backing;
            let (_, notches) = wheel_delta(
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, finger_px)),
                false,
                total,
                false,
            );
            let device_px = notches * PIXELS_PER_NOTCH * total;
            assert!(
                (device_px - 10.0 * backing).abs() < 0.001,
                "backing {backing} ux {ux}: {device_px}"
            );
        }
    }

    /// A wheel's detents carry no size: the display scale leaves them alone.
    #[test]
    fn rust_only_line_deltas_ignore_the_display_scale() {
        assert_eq!(
            wheel_delta(MouseScrollDelta::LineDelta(0.0, 1.0), false, 2.0, false),
            (0.0, 1.0)
        );
    }

    /// agg-sharp `TrackpadScrollAxisTests.EveryWheelNotchIsOneDetentNoMatterHowFastItIsTurned`,
    /// as the macOS shell sees it: an accelerated line delta is one signed notch.
    #[test]
    fn every_wheel_notch_is_one_detent_no_matter_how_fast_it_is_turned() {
        assert_eq!(line_delta_notches(6.0, true), 1.0);
        assert_eq!(line_delta_notches(0.1, true), 1.0);
        assert_eq!(line_delta_notches(-0.1, true), -1.0);
        assert_eq!(line_delta_notches(0.0, true), 0.0);
        assert_eq!(line_delta_notches(f64::INFINITY, true), 0.0);
        // Unaccelerated platforms keep a high-resolution wheel's fractions.
        assert_eq!(line_delta_notches(0.25, false), 0.25);
    }

    /// agg-sharp `TrackpadScrollAxisTests.ADiagonalScrollFillsBothAxes`: both
    /// axes go through the same scale, so a diagonal gesture keeps its angle.
    #[test]
    fn a_diagonal_scroll_fills_both_axes() {
        let (dx, dy) = wheel_delta(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(-4.0, 10.0)),
            false,
            1.0,
            false,
        );
        assert_eq!((dx * 40.0, dy * 40.0), (-4.0, 10.0));
    }

    /// agg-sharp `TrackpadScrollAxisTests.APurelyVerticalScrollLeavesTheHorizontalAxisAtZero`.
    #[test]
    fn a_purely_vertical_scroll_leaves_the_horizontal_axis_at_zero() {
        let (dx, dy) = wheel_delta(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 10.0)),
            false,
            2.0,
            false,
        );
        assert_eq!(dx, 0.0);
        assert_eq!(dy * 40.0, 5.0);
    }
}
