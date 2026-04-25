//! Global device-pixel-ratio (DPI) scale factor.
//!
//! All layout values (`margin`, `padding`, `min_size`, `max_size`) are stored
//! in **logical (device-independent) units**.  Layout algorithms call
//! [`device_scale`] once per frame (or per layout pass) and multiply logical
//! values by the factor to obtain physical pixel values.
//!
//! # Typical usage
//!
//! ```rust,ignore
//! // At startup / when the window moves to a different monitor:
//! agg_gui::device_scale::set_device_scale(window.scale_factor());
//!
//! // Inside a layout algorithm:
//! let physical_margin = child.margin().scale(device_scale());
//! ```
//!
//! # Thread safety
//!
//! GUI layout always runs on the main thread.  The value is stored in a
//! thread-local [`Cell`] so reads are zero-cost (no atomic, no lock).
//! On WASM there is only one thread, so this works correctly there too.

use std::cell::Cell;

thread_local! {
    static DEVICE_SCALE: Cell<f64> = Cell::new(1.0);
}

/// Return the current device scale factor (default `1.0`).
///
/// This is the ratio of physical pixels to logical pixels.  For a 2× HiDPI
/// display this is `2.0`; for a standard display it is `1.0`.
#[inline]
pub fn device_scale() -> f64 {
    DEVICE_SCALE.with(|s| s.get())
}

/// Set the device scale factor.
///
/// Call this at application startup and whenever the window moves to a monitor
/// with a different DPI.  A value of `1.0` means one logical unit equals one
/// physical pixel.
///
/// # Panics
///
/// Panics in debug builds if `scale` is not positive.
pub fn set_device_scale(scale: f64) {
    debug_assert!(
        scale > 0.0,
        "DeviceScale must be a positive value, got {scale}"
    );
    DEVICE_SCALE.with(|s| s.set(scale));
}
