//! OS double-click settings for the shell's click counter.
//!
//! `agg_gui::shell_input::InputForwarder` counts real presses into double and
//! triple clicks under a [`ClickPolicy`] (the longest gap between presses and
//! the farthest the pointer may travel). Its default (400 ms, 6 logical px)
//! is agg-gui's own; a desktop app should honour the user's OS setting, as
//! agg-sharp's WinForms and Cocoa backends did by taking the OS's own click
//! counts. This module reads that setting and turns it into a policy:
//!
//! - macOS: `NSEvent.doubleClickInterval` (seconds). AppKit exposes no
//!   distance, so the default travel tolerance stays.
//! - Windows: `GetDoubleClickTime` (ms) and `SM_CXDOUBLECLK`/`SM_CYDOUBLECLK`,
//!   the size in physical pixels of the rectangle centred on the first click
//!   that the second must land in.
//! - Linux and the rest: no cheap, reliable desktop-independent source, so
//!   agg-gui's default stays (agg-sharp's X11 backend hard-coded 500 ms / 4 px).
//!
//! `run.rs` applies the policy at start; `shell_loop.rs` re-applies it when the
//! window regains focus (the user may have changed the setting meanwhile) or
//! its scale factor changes (the Windows distance is in physical pixels).
//! The OS read is a plain `fn` ([`OsDoubleClickReader`]) so tests swap it out.

use std::time::Duration;

use agg_gui::shell_input::ClickPolicy;

/// The raw OS double-click setting, in the OS's own units. `None` where the
/// OS has no such setting or the read failed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct OsDoubleClick {
    /// Longest gap between the presses of a double click, in seconds.
    pub interval_secs: Option<f64>,
    /// Width and height, in physical pixels, of the rectangle centred on the
    /// first press that the second press must land in.
    pub rect_physical_px: Option<(f64, f64)>,
}

/// Reads the OS double-click setting; [`read_os_double_click`] in production.
pub(crate) type OsDoubleClickReader = fn() -> OsDoubleClick;

/// Shortest interval accepted from the OS. Anything shorter is unusable (no
/// one double-clicks in under 0.1 s), so it is clamped up rather than taken.
pub(crate) const MIN_INTERVAL: Duration = Duration::from_millis(100);
/// Longest interval accepted: Windows' own maximum for `SetDoubleClickTime`.
pub(crate) const MAX_INTERVAL: Duration = Duration::from_millis(5000);
/// Smallest travel tolerance accepted, in logical pixels: a hand never puts
/// two presses on exactly the same pixel.
pub(crate) const MIN_TRAVEL: f64 = 1.0;
/// Largest travel tolerance accepted, in logical pixels.
pub(crate) const MAX_TRAVEL: f64 = 64.0;

/// Turn the OS setting into a [`ClickPolicy`] for a window at `scale`
/// (physical pixels per logical pixel). A missing, zero, negative or
/// non-finite value keeps agg-gui's default for that field; a finite positive
/// value is clamped into [`MIN_INTERVAL`]..=[`MAX_INTERVAL`] or
/// [`MIN_TRAVEL`]..=[`MAX_TRAVEL`].
pub(crate) fn policy_from_os(os: OsDoubleClick, scale: f64) -> ClickPolicy {
    let default = ClickPolicy::default();
    let double_click_time = os
        .interval_secs
        .filter(|s| s.is_finite() && *s > 0.0)
        .map(|s| Duration::from_secs_f64(s.min(MAX_INTERVAL.as_secs_f64())))
        .map(|d| d.clamp(MIN_INTERVAL, MAX_INTERVAL))
        .unwrap_or(default.double_click_time);
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let travel_tolerance = os
        .rect_physical_px
        .filter(|(w, h)| w.is_finite() && h.is_finite() && *w > 0.0 && *h > 0.0)
        // The rectangle is centred on the first press, so the pointer may move
        // half its size; the forwarder measures a radius, so take the tighter
        // axis.
        .map(|(w, h)| (w.min(h) / 2.0 / scale).clamp(MIN_TRAVEL, MAX_TRAVEL))
        .unwrap_or(default.travel_tolerance);
    ClickPolicy {
        double_click_time,
        travel_tolerance,
    }
}

/// The OS setting as a policy for a window at `scale`, read through `read`.
pub(crate) fn os_click_policy(read: OsDoubleClickReader, scale: f64) -> ClickPolicy {
    policy_from_os(read(), scale)
}

/// macOS: `NSEvent.doubleClickInterval`, the user's System Settings value.
#[cfg(target_os = "macos")]
pub(crate) fn read_os_double_click() -> OsDoubleClick {
    OsDoubleClick {
        interval_secs: Some(objc2_app_kit::NSEvent::doubleClickInterval()),
        rect_physical_px: None,
    }
}

/// Windows: `GetDoubleClickTime` and the `SM_CXDOUBLECLK`/`SM_CYDOUBLECLK`
/// rectangle (physical pixels; winit makes the process per-monitor DPI aware).
#[cfg(windows)]
pub(crate) fn read_os_double_click() -> OsDoubleClick {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXDOUBLECLK, SM_CYDOUBLECLK,
    };
    // Safety: all three are argument-free (or take a plain index) reads of
    // system settings with no pointers involved; zero means "unavailable",
    // which `policy_from_os` treats as missing.
    let (ms, cx, cy) = unsafe {
        (
            GetDoubleClickTime(),
            GetSystemMetrics(SM_CXDOUBLECLK),
            GetSystemMetrics(SM_CYDOUBLECLK),
        )
    };
    OsDoubleClick {
        interval_secs: Some(ms as f64 / 1000.0),
        rect_physical_px: Some((cx as f64, cy as f64)),
    }
}

/// Elsewhere (Linux, BSD): no desktop-independent setting, so agg-gui's
/// default policy stays.
#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn read_os_double_click() -> OsDoubleClick {
    OsDoubleClick::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(interval_secs: Option<f64>, rect: Option<(f64, f64)>) -> OsDoubleClick {
        OsDoubleClick {
            interval_secs,
            rect_physical_px: rect,
        }
    }

    #[test]
    fn missing_settings_keep_the_default_policy() {
        assert_eq!(policy_from_os(os(None, None), 1.0), ClickPolicy::default());
        assert_eq!(policy_from_os(os(None, None), 2.0), ClickPolicy::default());
    }

    #[test]
    fn seconds_become_the_double_click_time() {
        let p = policy_from_os(os(Some(0.5), None), 1.0);
        assert_eq!(p.double_click_time, Duration::from_millis(500));
        assert_eq!(p.travel_tolerance, ClickPolicy::default().travel_tolerance);
        // Windows' default, read as ms / 1000.
        let p = policy_from_os(os(Some(500.0 / 1000.0), None), 1.0);
        assert_eq!(p.double_click_time, Duration::from_millis(500));
    }

    #[test]
    fn the_rectangle_becomes_a_logical_radius() {
        // Windows' default 4 x 4 physical rectangle: 2 px each way at 100 %.
        let p = policy_from_os(os(None, Some((4.0, 4.0))), 1.0);
        assert_eq!(p.travel_tolerance, 2.0);
        // A 16 x 12 rectangle at 200 %: the tighter axis, halved, in logical px.
        let p = policy_from_os(os(None, Some((16.0, 12.0))), 2.0);
        assert_eq!(p.travel_tolerance, 3.0);
    }

    #[test]
    fn nonsense_intervals_keep_the_default() {
        let default = ClickPolicy::default().double_click_time;
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let p = policy_from_os(os(Some(bad), None), 1.0);
            assert_eq!(p.double_click_time, default, "interval {bad}");
        }
    }

    #[test]
    fn out_of_range_intervals_are_clamped() {
        let p = policy_from_os(os(Some(0.001), None), 1.0);
        assert_eq!(p.double_click_time, MIN_INTERVAL);
        let p = policy_from_os(os(Some(60.0), None), 1.0);
        assert_eq!(p.double_click_time, MAX_INTERVAL);
        let p = policy_from_os(os(Some(1e300), None), 1.0);
        assert_eq!(p.double_click_time, MAX_INTERVAL);
    }

    #[test]
    fn nonsense_rectangles_keep_the_default() {
        let default = ClickPolicy::default().travel_tolerance;
        for bad in [
            (0.0, 4.0),
            (4.0, -2.0),
            (f64::NAN, 4.0),
            (4.0, f64::INFINITY),
        ] {
            let p = policy_from_os(os(None, Some(bad)), 1.0);
            assert_eq!(p.travel_tolerance, default, "rect {bad:?}");
        }
    }

    #[test]
    fn out_of_range_rectangles_are_clamped() {
        let p = policy_from_os(os(None, Some((1.0, 1.0))), 2.0);
        assert_eq!(p.travel_tolerance, MIN_TRAVEL);
        let p = policy_from_os(os(None, Some((1000.0, 1000.0))), 1.0);
        assert_eq!(p.travel_tolerance, MAX_TRAVEL);
    }

    #[test]
    fn a_nonsense_scale_is_treated_as_one() {
        for bad in [0.0, -2.0, f64::NAN] {
            let p = policy_from_os(os(None, Some((8.0, 8.0))), bad);
            assert_eq!(p.travel_tolerance, 4.0, "scale {bad}");
        }
    }

    #[test]
    fn os_click_policy_uses_the_given_reader() {
        fn fake() -> OsDoubleClick {
            OsDoubleClick {
                interval_secs: Some(0.25),
                rect_physical_px: Some((10.0, 10.0)),
            }
        }
        let p = os_click_policy(fake, 1.0);
        assert_eq!(p.double_click_time, Duration::from_millis(250));
        assert_eq!(p.travel_tolerance, 5.0);
    }

    #[test]
    fn the_production_reader_yields_a_sane_policy() {
        let p = os_click_policy(read_os_double_click, 1.0);
        assert!((MIN_INTERVAL..=MAX_INTERVAL).contains(&p.double_click_time));
        assert!((MIN_TRAVEL..=MAX_TRAVEL).contains(&p.travel_tolerance));
    }
}
