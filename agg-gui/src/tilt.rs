//! # Device-tilt input — platform shell → app
//!
//! Games and viewers can steer from the device's orientation sensor.
//! Apps can't touch the DOM/OS sensor APIs, so the flow mirrors
//! [`crate::fullscreen`]: the app calls [`request_enable`] once; the
//! platform shell polls [`take_enable_request`], performs whatever
//! permission dance the platform demands (iOS requires
//! `DeviceOrientationEvent.requestPermission()` from a user gesture),
//! installs the sensor listener, and reports readiness through
//! [`set_enabled`]. Each sensor event lands via [`set_reading`] as a
//! SCREEN-SPACE tilt vector — shells fold the display's rotation in,
//! so `x` is always "tilted toward the right edge of the screen" and
//! `y` "toward the bottom edge", in degrees, regardless of how the
//! device is held.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static ENABLE_PENDING: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(false);
static HAS_READING: AtomicBool = AtomicBool::new(false);
static READ_X: AtomicU64 = AtomicU64::new(0);
static READ_Y: AtomicU64 = AtomicU64::new(0);

/// App side: ask the shell to start delivering tilt readings. Safe to
/// call unconditionally at startup — shells without a sensor simply
/// never report enabled.
pub fn request_enable() {
    ENABLE_PENDING.store(true, Ordering::Relaxed);
}

/// Shell side: consume a pending enable request, if any.
pub fn take_enable_request() -> bool {
    ENABLE_PENDING.swap(false, Ordering::Relaxed)
}

/// Shell side: report whether the sensor listener is live (permission
/// granted and events flowing).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if !on {
        HAS_READING.store(false, Ordering::Relaxed);
    }
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Shell side: publish a screen-space tilt reading, in degrees.
/// `x` > 0 means the device leans toward the screen's right edge,
/// `y` > 0 toward its bottom edge.
pub fn set_reading(x: f64, y: f64) {
    READ_X.store(x.to_bits(), Ordering::Relaxed);
    READ_Y.store(y.to_bits(), Ordering::Relaxed);
    HAS_READING.store(true, Ordering::Relaxed);
}

/// App side: the most recent tilt reading, or `None` before the first
/// sensor event (or after the listener is torn down).
pub fn reading() -> Option<(f64, f64)> {
    if !HAS_READING.load(Ordering::Relaxed) {
        return None;
    }
    Some((
        f64::from_bits(READ_X.load(Ordering::Relaxed)),
        f64::from_bits(READ_Y.load(Ordering::Relaxed)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_reading_and_state_roundtrip() {
        assert!(!take_enable_request());
        request_enable();
        assert!(take_enable_request());
        assert!(!take_enable_request(), "request must coalesce and clear");

        assert_eq!(reading(), None);
        set_reading(12.5, -3.25);
        assert_eq!(reading(), Some((12.5, -3.25)));
        set_enabled(true);
        assert!(is_enabled());
        set_enabled(false);
        assert!(!is_enabled());
        assert_eq!(reading(), None, "disable clears the stale reading");
    }
}
