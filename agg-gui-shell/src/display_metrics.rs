//! What the shell believes about the display its window is on: the display
//! scale and the display's size, guarded against the garbage an OS can report
//! mid hot-plug, and the first-launch window size derived from them.
//!
//! Ports agg-sharp `Gui/SystemWindow/UiScale.cs` (`Usable`,
//! `ScaledWindowSize`) and the guards in `SystemWindow.SetDisplayScale` /
//! `SetDisplayUsableSize`. `run.rs` applies these when it creates the window,
//! `shell_loop.rs` on every `ScaleFactorChanged` and `Resized`.

/// The display scale to compose with for a reported `display_scale`.
///
/// A zero or NaN scale would silently divide the whole UI to nothing, and a
/// monitor caught mid hot-plug can report one, so anything not finite and
/// positive is a 1x display (C# `UiScale.Usable`).
pub fn usable_display_scale(display_scale: f64) -> f64 {
    if display_scale.is_nan() || display_scale.is_infinite() || display_scale <= 0.0 {
        1.0
    } else {
        display_scale
    }
}

/// The size, in physical pixels, of the display the window is on, keeping the
/// last good measurement (C# `SystemWindow.DisplayUsableSize`).
///
/// A window dragged off screen, or caught mid hot-plug, measures nothing.
/// Taking that as the new truth would throw away the only description of the
/// monitor the window is on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DisplayUsableSize {
    size: Option<(f64, f64)>,
}

impl DisplayUsableSize {
    /// No display measured yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a measurement; one that is not finite and positive in both
    /// dimensions is ignored (C# `SetDisplayUsableSize`).
    pub fn set(&mut self, width: f64, height: f64) {
        let usable = |v: f64| !v.is_nan() && !v.is_infinite() && v > 0.0;
        if usable(width) && usable(height) {
            self.size = Some((width, height));
        }
    }

    /// The last good measurement, `None` before the first.
    pub fn get(&self) -> Option<(f64, f64)> {
        self.size
    }
}

/// A window size in device pixels for a design size in logical points: the
/// design size scaled by `device_scale` (C#'s `Math.Round`, ties to even),
/// then clamped to `desktop_size` so a 150% laptop does not open a window
/// taller than its screen. A non-positive desktop dimension means the host
/// could not measure it and is not clamped to; that would ask for a window
/// with no pixels in it (C# `UiScale.ScaledWindowSize`).
pub fn scaled_window_size(
    design_width: f64,
    design_height: f64,
    device_scale: f64,
    desktop_size: (i32, i32),
) -> (i32, i32) {
    // C# `(int)Math.Round(x)`: `as` saturates where C# would wrap, which only
    // matters for sizes no display has.
    let mut width = (design_width * device_scale).round_ties_even() as i32;
    let mut height = (design_height * device_scale).round_ties_even() as i32;
    if desktop_size.0 > 0 {
        width = width.min(desktop_size.0);
    }
    if desktop_size.1 > 0 {
        height = height.min(desktop_size.1);
    }
    (width, height)
}

#[cfg(test)]
#[path = "display_metrics_tests.rs"]
mod tests;
