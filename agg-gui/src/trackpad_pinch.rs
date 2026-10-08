//! Pinch-to-zoom as a wheel: a trackpad's magnify gesture and a browser's
//! ctrl+wheel pinch, turned into agg-gui wheel **notches**.
//!
//! Ports agg-sharp's `Gui/SystemWindow/WheelDeltaMath.cs` (the magnification
//! and detent conversions), the pinch half of
//! `PlatformBrowser/browser/BrowserWheel.cs`, the `FromTrackpadPinch` mark of
//! `Gui/Args/MouseEventArgs.cs`, and the wheel half of
//! `PlatformMac/mac/MacTrackpadGestures.cs`.
//!
//! agg-sharp's wheel is in Win32 units, 120 per detent; agg-gui's
//! [`Event::MouseWheel`](crate::event::Event::MouseWheel) carries notches
//! (see [`crate::wheel`]), one per detent. The conversions here compute the
//! C# integer wheel delta first and then divide by
//! [`WHEEL_DELTA_PER_NOTCH`], so a pinch zooms exactly as far as it does in
//! agg-sharp, rounding included.
//!
//! A pinch reaches widgets as an ordinary `MouseWheel` (every zoom consumer
//! already reads a forward wheel as zoom in). A native trackpad pinch is
//! additionally *marked*: [`wheel_from_trackpad_pinch`] answers `true` while
//! it is being dispatched, as C#'s `MouseEventArgs.FromTrackpadPinch` does,
//! so a widget that follows the fingers itself can leave the wheel alone. The
//! shells deliver it through [`crate::App::on_trackpad_pinch`]; the web shell
//! converts every DOM wheel through [`browser_wheel_notches`], which routes a
//! ctrl+wheel to [`browser_pinch_notches`] (unmarked, as C#'s browser host
//! leaves it).

use std::cell::Cell;

use crate::wheel::{to_notches, WheelDeltaMode};

/// Win32 wheel units in one detent, which is one agg-gui notch.
pub const WHEEL_DELTA_PER_NOTCH: f64 = 120.0;

/// Wheel units per unit of pinch magnification (C#
/// `WheelDeltaMath.MagnifyWheelDeltaPerUnit`).
///
/// A magnify gesture reports the *incremental* change in scale for one event,
/// in the same units Apple's own sample code accumulates into a zoom factor:
/// magnification 1.0 in total means "twice the size". Consumers of the wheel
/// treat one 120-unit detent as one zoom step, and the 3D view's step closes
/// 20% of the distance to what is under the pointer. Closing a fraction f of
/// that distance scales the view by about 1/(1-f), so matching a
/// magnification of m needs f = m, which is m / 0.2 = 5m detents, i.e. 600m
/// wheel units. A comfortable pinch runs to roughly m = 1, so it travels
/// about five detents - the same order as a comfortable two-finger scroll.
pub const MAGNIFY_WHEEL_DELTA_PER_UNIT: f64 = 600.0;

/// CSS pixels of ctrl-wheel travel that make one unit of pinch magnification
/// (C# `BrowserWheel.PinchCssPixelsPerMagnificationUnit`).
///
/// A trackpad pinch reaches the page as a synthetic ctrl+wheel with no scale
/// on it, so the pinch has to be recovered from the travel. Both Chromium and
/// WebKit synthesize that event from the gesture's scale as roughly
/// `deltaY = -100 * ln(scale)`, and for the small per-event steps a pinch is
/// made of, `ln(scale) = scale - 1`, which is exactly what AppKit calls the
/// incremental magnification. Going through magnification rather than
/// straight to wheel units is what keeps the browser's pinch and the mac's
/// pinch feeling the same.
pub const PINCH_CSS_PIXELS_PER_MAGNIFICATION_UNIT: f64 = 100.0;

/// C# `Math.Round`: halves round to the even neighbour (banker's rounding).
/// Hand-written because `f64::round_ties_even` is newer than agg-gui's MSRV.
fn round_half_even(x: f64) -> f64 {
    if (x - x.trunc()).abs() == 0.5 {
        2.0 * (x / 2.0).round()
    } else {
        x.round()
    }
}

/// C# `WheelDeltaMath.ScrollingDeltaToWheelDelta`: one axis of a scroll
/// event's travel in Win32 wheel units. A precise device (trackpad) is
/// points of travel scaled by `5 x backing_scale`; a detent device is one
/// signed 120-unit detent per event, however accelerated its line count.
/// A non-finite delta is no scroll: `(int)` of a NaN would fling the content.
pub fn scrolling_delta_to_wheel_delta(
    scrolling_delta: f64,
    precise: bool,
    backing_scale: f64,
) -> i32 {
    if !scrolling_delta.is_finite() {
        return 0;
    }
    if precise {
        // C# `(int)Math.Round(...)`: banker's rounding, then a truncating cast
        // (`as` saturates where C# would be undefined; only reachable for a
        // delta of over 400 million points).
        round_half_even(scrolling_delta * backing_scale * 5.0) as i32
    } else {
        // C# `Math.Sign(x) * 120`: zero stays zero.
        if scrolling_delta > 0.0 {
            120
        } else if scrolling_delta < 0.0 {
            -120
        } else {
            0
        }
    }
}

/// C# `WheelDeltaMath.MagnificationToWheelDelta`: one magnify event's
/// incremental magnification in Win32 wheel units. The sign is carried
/// straight through, so fingers apart (positive) is a forward wheel, which
/// is zoom in.
pub fn magnification_to_wheel_delta(magnification: f64) -> i32 {
    if !magnification.is_finite() {
        return 0;
    }
    round_half_even(magnification * MAGNIFY_WHEEL_DELTA_PER_UNIT) as i32
}

/// A magnification as agg-gui wheel notches: the C# wheel delta over
/// [`WHEEL_DELTA_PER_NOTCH`].
pub fn magnification_to_notches(magnification: f64) -> f64 {
    magnification_to_wheel_delta(magnification) as f64 / WHEEL_DELTA_PER_NOTCH
}

/// C# `BrowserWheel.ApplyPinch`'s wheel delta: a ctrl+wheel's DOM `deltaY`
/// as Win32 wheel units.
///
/// Every engine synthesizes a trackpad pinch as a wheel event with `ctrlKey`
/// set, and nothing tells it from a real Ctrl held over a real wheel - which
/// is fine, because both mean zoom to a browser user. What they do not share
/// is a magnitude, which is why `deltaMode` still decides: a pinch is pixels
/// and carries recoverable travel, while Ctrl over a real wheel is lines in
/// Firefox and becomes one signed detent per event. Negated as a scroll is:
/// fingers apart (zoom in) reports a negative `deltaY` and has to arrive as
/// a forward wheel. Sideways travel is dropped - a pinch is one number - and
/// the result is never a precise scroll: these are zoom steps, not distance.
pub fn browser_pinch_wheel_delta(delta_y: f64, mode: WheelDeltaMode) -> i32 {
    if mode == WheelDeltaMode::Pixel {
        magnification_to_wheel_delta(-delta_y / PINCH_CSS_PIXELS_PER_MAGNIFICATION_UNIT)
    } else {
        scrolling_delta_to_wheel_delta(-delta_y, false, 1.0)
    }
}

/// [`browser_pinch_wheel_delta`] in agg-gui notches, for the web shell's
/// `MouseWheel`.
pub fn browser_pinch_notches(delta_y: f64, mode: WheelDeltaMode) -> f64 {
    browser_pinch_wheel_delta(delta_y, mode) as f64 / WHEEL_DELTA_PER_NOTCH
}

/// C# `BrowserWheel.ApplyWheelEvent` in agg-gui notches: one DOM `wheel`
/// event's `(delta_x, delta_y)` as agg-gui wheel notches. A ctrl+wheel is a
/// pinch ([`browser_pinch_notches`], one axis only); anything else is a
/// scroll through [`crate::wheel::to_notches`], with the DOM's
/// positive-scroll-down signs flipped to agg-gui's positive = forward wheel.
pub fn browser_wheel_notches(
    delta_x: f64,
    delta_y: f64,
    mode: WheelDeltaMode,
    ctrl_key: bool,
) -> (f64, f64) {
    if ctrl_key {
        (0.0, browser_pinch_notches(delta_y, mode))
    } else {
        (to_notches(-delta_x, mode), to_notches(-delta_y, mode))
    }
}

thread_local! {
    static FROM_TRACKPAD_PINCH: Cell<bool> = const { Cell::new(false) };
}

/// `true` while the `MouseWheel` being dispatched came from a trackpad pinch
/// (C# `MouseEventArgs.FromTrackpadPinch`). A widget that follows the fingers
/// itself skips such a wheel so it zooms once; everything else - the 3D view
/// included - reads it as the ordinary zoom it is. A thread-local rather than
/// an `Event` field so no existing `MouseWheel` pattern or constructor changes,
/// the same shape as [`crate::current_modifiers`].
pub fn wheel_from_trackpad_pinch() -> bool {
    FROM_TRACKPAD_PINCH.with(Cell::get)
}

/// Marks the wheel dispatched inside `f` as a trackpad pinch, restoring the
/// previous mark afterwards (so the mark never leaks onto a later wheel).
pub(crate) fn with_trackpad_pinch_mark<R>(f: impl FnOnce() -> R) -> R {
    let previous = FROM_TRACKPAD_PINCH.with(|c| c.replace(true));
    let result = f();
    FROM_TRACKPAD_PINCH.with(|c| c.set(previous));
    result
}
