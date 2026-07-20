//! Window-size sanitisation for the native demo shell.
//!
//! Split out of `main.rs` so the DPI-unit round-trip logic is unit-testable
//! without a live winit/wgpu window (and to keep `main.rs` under the 800-line
//! limit).  Two concerns live here:
//!
//! - [`sanitize_restored_window_size`] guards the *restore* path: a saved
//!   window size read from disk is physical pixels, but a corrupted or
//!   out-of-range value (e.g. from an earlier DPI-ratchet bug, or a zeroed
//!   state file) must not be handed to winit verbatim — it could produce an
//!   invisible window or a surface larger than the GPU's max texture size.
//! - [`clamp_surface_size`] is the last line of defence in `Gpu`: it clamps
//!   the surface configuration to `[1, max_texture_dimension_2d]` so no code
//!   path can panic `Surface::configure` with an over-large extent.
//!
//! The stored size is canonically **physical px** (see the save site in
//! `main.rs`); restoring therefore uses `PhysicalSize`, not `LogicalSize`, so
//! the value round-trips without being re-multiplied by the monitor scale.

/// Fallback window size when there is no saved state (matches the historical
/// first-launch default).
const DEFAULT_W: u32 = 1280;
const DEFAULT_H: u32 = 720;

/// Floor for a restored window so a zeroed / tiny saved size can't create an
/// effectively invisible window.
const MIN_W: u32 = 200;
const MIN_H: u32 = 150;

/// Ceiling used when the primary monitor size is unknown.  Matches the common
/// wgpu `max_texture_dimension_2d` so a restore can't request a surface the
/// GPU will reject.
const FALLBACK_MAX_DIM: u32 = 8192;

/// Clamp a saved (physical-px) window size into a usable, GPU-safe range.
///
/// - `saved` `None` → the first-launch default `(1280, 720)`.
/// - Each dimension is clamped up to the monitor's physical size when known,
///   else to [`FALLBACK_MAX_DIM`]; this is what recovers an already-corrupted
///   state file (the DPI-ratchet bug that grew the stored size every launch).
/// - Each dimension is floored to [`MIN_W`]/[`MIN_H`] so zero/tiny values still
///   yield a visible window.
pub fn sanitize_restored_window_size(
    saved: Option<(u32, u32)>,
    monitor: Option<(u32, u32)>,
) -> (u32, u32) {
    let (w, h) = match saved {
        Some(s) => s,
        None => return (DEFAULT_W, DEFAULT_H),
    };
    // Guard against a monitor smaller than the floor so the clamp range stays
    // valid (`min <= max`).
    let (max_w, max_h) = match monitor {
        Some((mw, mh)) => (mw.max(MIN_W), mh.max(MIN_H)),
        None => (FALLBACK_MAX_DIM, FALLBACK_MAX_DIM),
    };
    (w.clamp(MIN_W, max_w), h.clamp(MIN_H, max_h))
}

/// Clamp a surface configuration size to `[1, max_dim]` on both axes.
///
/// `max_dim` is the device's `max_texture_dimension_2d`.  Applied on every
/// `Surface::configure` so a stray oversized request degrades to the GPU limit
/// instead of a validation panic.
pub fn clamp_surface_size(w: u32, h: u32, max_dim: u32) -> (u32, u32) {
    let max_dim = max_dim.max(1);
    (w.clamp(1, max_dim), h.clamp(1, max_dim))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupted_saved_size_clamps_to_monitor() {
        // The user's exact crash case: a DPI-ratcheted state file grew the
        // stored size past the GPU max; it must clamp down to the monitor.
        let out = sanitize_restored_window_size(Some((10224, 5925)), Some((3408, 1975)));
        assert_eq!(out, (3408, 1975));
    }

    #[test]
    fn no_saved_size_uses_default() {
        assert_eq!(
            sanitize_restored_window_size(None, Some((3408, 1975))),
            (1280, 720)
        );
        assert_eq!(sanitize_restored_window_size(None, None), (1280, 720));
    }

    #[test]
    fn in_bounds_size_is_unchanged() {
        assert_eq!(
            sanitize_restored_window_size(Some((1600, 900)), Some((3408, 1975))),
            (1600, 900)
        );
    }

    #[test]
    fn no_monitor_clamps_to_fallback_max() {
        // Only the width exceeds the fallback ceiling; height is already below.
        assert_eq!(
            sanitize_restored_window_size(Some((10224, 5925)), None),
            (8192, 5925)
        );
        // A reasonable size with no monitor known passes through.
        assert_eq!(
            sanitize_restored_window_size(Some((1280, 720)), None),
            (1280, 720)
        );
    }

    #[test]
    fn zero_or_tiny_size_is_floored() {
        assert_eq!(
            sanitize_restored_window_size(Some((0, 0)), Some((3408, 1975))),
            (200, 150)
        );
        assert_eq!(
            sanitize_restored_window_size(Some((10, 5)), None),
            (200, 150)
        );
    }

    #[test]
    fn monitor_smaller_than_floor_keeps_range_valid() {
        // Degenerate monitor smaller than the floor must not panic the clamp;
        // the floor wins.
        assert_eq!(
            sanitize_restored_window_size(Some((100, 100)), Some((50, 40))),
            (200, 150)
        );
    }

    #[test]
    fn surface_size_clamped_to_max_dim() {
        assert_eq!(clamp_surface_size(10224, 5925, 8192), (8192, 5925));
        assert_eq!(clamp_surface_size(0, 0, 8192), (1, 1));
        assert_eq!(clamp_surface_size(1280, 720, 8192), (1280, 720));
        // Degenerate zero limit still yields a valid 1x1.
        assert_eq!(clamp_surface_size(100, 100, 0), (1, 1));
    }
}
