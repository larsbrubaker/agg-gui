//! winit input → agg-gui input.
//!
//! The mapping of button/key/modifier *types* lives in
//! `agg_gui::winit_adapter`; what lives here is the behaviour the adapter
//! can't know about: wheel-delta conversion and the shift→horizontal remap,
//! and raw touch forwarding. Both are pure enough to test without a window.

use agg_gui::{App, Modifiers};
use winit::event::{MouseScrollDelta, Touch, TouchPhase};

/// Pixel deltas (trackpads, precision wheels) per notional wheel line. Matches
/// the browser-shell conversion in agg-gui's web shells, so a scroll feels the
/// same on both.
const PIXELS_PER_LINE: f64 = 40.0;

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
pub(crate) fn wheel_delta(delta: MouseScrollDelta, shift: bool) -> (f64, f64) {
    let (mut dx, mut dy) = match delta {
        MouseScrollDelta::LineDelta(x, y) => (x as f64, y as f64),
        MouseScrollDelta::PixelDelta(p) => (p.x / PIXELS_PER_LINE, p.y / PIXELS_PER_LINE),
    };
    if shift && dx == 0.0 {
        dx = dy;
        dy = 0.0;
    }
    (dx, dy)
}

/// Forward a raw touch to the app.
///
/// Raw touches only: agg-gui core aggregates multi-finger gestures AND replays
/// the primary finger as mouse events (`touch_emulation.rs`), mirroring the
/// wasm shell. winit registers for touch on Windows, which suppresses the OS's
/// mouse promotion — so without this a touchscreen does nothing at all.
/// `location` is already in the same physical-pixel space as `CursorMoved`.
/// Device id is pinned to 0: winit's `DeviceId` is opaque, and telling two
/// touchscreens apart isn't worth a lossy hash of it.
pub(crate) fn dispatch_touch(app: &mut App, touch: Touch) {
    let dev = agg_gui::TouchDeviceId(0);
    let tid = agg_gui::TouchId(touch.id);
    let (x, y) = (touch.location.x, touch.location.y);
    let force = touch.force.map(|f| f.normalized() as f32);
    match touch.phase {
        TouchPhase::Started => app.on_touch_start(dev, tid, x, y, force),
        TouchPhase::Moved => app.on_touch_move(dev, tid, x, y, force),
        TouchPhase::Ended => app.on_touch_end(dev, tid),
        TouchPhase::Cancelled => app.on_touch_cancel(dev, tid),
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
            wheel_delta(MouseScrollDelta::LineDelta(0.0, 3.0), false),
            (0.0, 3.0)
        );
        assert_eq!(
            wheel_delta(MouseScrollDelta::LineDelta(0.0, -3.0), false),
            (0.0, -3.0)
        );
    }

    #[test]
    fn pixel_deltas_convert_to_lines() {
        assert_eq!(
            wheel_delta(
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 40.0)),
                false
            ),
            (0.0, 1.0)
        );
    }

    #[test]
    fn shift_remaps_a_vertical_wheel_to_horizontal() {
        assert_eq!(
            wheel_delta(MouseScrollDelta::LineDelta(0.0, 2.0), true),
            (2.0, 0.0)
        );
    }

    #[test]
    fn shift_leaves_a_device_that_already_scrolls_horizontally_alone() {
        // A tilt wheel / trackpad reporting both axes must not have its
        // vertical component stolen.
        assert_eq!(
            wheel_delta(MouseScrollDelta::LineDelta(1.0, 2.0), true),
            (1.0, 2.0)
        );
    }
}
