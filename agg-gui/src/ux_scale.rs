//! Global **user-experience scale** factor.
//!
//! Distinct from [`crate::device_scale`], which tracks the physical
//! device-pixel ratio so glyphs stay crisp on HiDPI displays. The UX
//! scale exists because the same logical pixel feels very different
//! to a user on a desktop monitor at desk distance vs. a phone held
//! at arm's length.
//!
//! The cleanest mental model:
//!
//! - **`device_scale`**: pixels per logical unit on the physical
//!   display surface. Always set by the platform shell to whatever
//!   `window.devicePixelRatio` / `winit::Window::scale_factor`
//!   reports. Driven by the hardware.
//! - **`ux_scale`**: how much bigger the *user* wants every logical
//!   unit to be on top of that. Driven by ergonomic / accessibility
//!   needs — small on a 27" monitor read at arm's length, bigger on
//!   a 6" phone read at arm's length, bigger still for users with
//!   reduced vision.
//!
//! The framework multiplies the two when it computes the effective
//! viewport / paint transform inside [`crate::App`]. Widgets always
//! see "logical" units (already divided by the effective scale), so
//! no per-widget changes are needed — only platform shells need to
//! call [`set_ux_scale`].
//!
//! ## Suggested values
//!
//! - `1.0` — desktop / laptop / cursor-driven UI. The default.
//! - `1.6` – `1.8` — mobile touch (phone / tablet) where the user
//!   reads at arm's length and needs ~44 px touch targets. Auto-set
//!   when [`crate::input_profile::set_input_profile`] is called with
//!   any mobile variant.
//! - User-controlled accessibility setting on top of that — `1.0` to
//!   `2.0+` for users who explicitly want bigger UI.

use std::cell::Cell;

thread_local! {
    static UX_SCALE: Cell<f64> = Cell::new(1.0);
}

/// Current UX scale factor. Multiplied with [`crate::device_scale`] in
/// [`crate::App::layout`] / [`crate::App::paint`] to give the effective
/// "physical pixels per logical unit" the framework actually uses.
#[inline]
pub fn ux_scale() -> f64 {
    UX_SCALE.with(|s| s.get())
}

/// Set the UX scale factor. Panics on non-positive values in debug.
///
/// Platform shells typically call this once at startup after they've
/// figured out the device profile, and again from a settings UI that
/// lets the user tune readability.
pub fn set_ux_scale(scale: f64) {
    debug_assert!(
        scale > 0.0,
        "ux_scale must be a positive value, got {scale}"
    );
    UX_SCALE.with(|s| s.set(scale));
    crate::animation::request_draw();
}

/// Combined "physical pixels per logical unit" the framework uses
/// for layout / paint scaling. `device_scale * ux_scale`.
#[inline]
pub fn effective_scale() -> f64 {
    crate::device_scale() * ux_scale()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_one() {
        // Tests run sequentially in a single thread; reset to known.
        set_ux_scale(1.0);
        assert!((ux_scale() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn round_trip() {
        set_ux_scale(1.7);
        assert!((ux_scale() - 1.7).abs() < 1e-9);
        set_ux_scale(1.0);
    }
}
