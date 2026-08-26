//! Window-bounds persistence: the restore-path sanitiser, the store trait an
//! app implements to choose its own file/format, and the save gate.
//!
//! The shell restores the window size at startup and writes it back as it
//! changes; *where* it lives (a dot-file, a settings blob, the registry) is the
//! app's business, so the shell only talks to [`WindowBoundsStore`].
//!
//! The stored size is canonically **physical px**: winit's `Resized` reports
//! physical pixels, so restoring must use `PhysicalSize` too (see
//! [`crate::WindowSize`]). Restoring a physical size as a logical one
//! re-multiplies it by the monitor scale factor on every launch — the
//! DPI-ratchet bug that eventually asked for a surface larger than the GPU's
//! `max_texture_dimension_2d` and crashed `Surface::configure`.
//! [`sanitize_restored_window_size`] is what recovers an already-corrupted
//! saved value.

/// Fallback window size when there is no saved state and the caller gave no
/// initial size (matches the historical first-launch default).
const DEFAULT_W: u32 = 1280;
const DEFAULT_H: u32 = 720;

/// Floor for a restored window so a zeroed / tiny saved size can't create an
/// effectively invisible window.
const MIN_W: u32 = 200;
const MIN_H: u32 = 150;

/// Ceiling used when the primary monitor size is unknown. Matches the common
/// wgpu `max_texture_dimension_2d` so a restore can't request a surface the
/// GPU will reject.
const FALLBACK_MAX_DIM: u32 = 8192;

/// A persisted window geometry. Sizes are **physical pixels**.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedBounds {
    pub width: u32,
    pub height: u32,
    /// Whether the window was maximized. The size is then the last *windowed*
    /// size, not the maximized rect — restoring the maximized rect as a
    /// windowed size would fill the screen with a non-maximized window.
    pub maximized: bool,
}

/// Where the shell restores window bounds from and saves them to.
///
/// Implement it on whatever already owns the app's settings; the shell calls
/// [`load`](WindowBoundsStore::load) once before the window is created and
/// [`save`](WindowBoundsStore::save) when the geometry changes (gated on no
/// mouse button being held, so a drag-resize doesn't thrash the disk) and once
/// more on exit.
pub trait WindowBoundsStore {
    /// Bounds from the last run, or `None` for a first launch.
    fn load(&self) -> Option<SavedBounds>;
    /// Persist `bounds`. Called from the event loop, so keep it cheap — an app
    /// that already batches its own state can just record the value here and
    /// let its own save path write it.
    fn save(&self, bounds: SavedBounds);
}

/// Clamp a saved (physical-px) window size into a usable, GPU-safe range.
///
/// - `saved` `None` → the first-launch default `(1280, 720)`.
/// - Each dimension is clamped up to the monitor's physical size when known,
///   else to a conservative 8192 ceiling (the common wgpu
///   `max_texture_dimension_2d`); this is what recovers an already-corrupted
///   saved value (the DPI-ratchet bug that grew the stored size every launch).
/// - Each dimension is floored to 200x150 so zero/tiny values still yield a
///   visible window.
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

/// Diff-and-gate policy for writing bounds back, mirroring
/// `agg_gui::persistence::AutoSave` for a value type rather than a blob:
/// write only when the bounds actually changed AND no mouse button is held,
/// so a drag-resize doesn't hit the store on every intermediate size.
#[derive(Default)]
pub(crate) struct BoundsAutoSave {
    last_saved: Option<SavedBounds>,
}

impl BoundsAutoSave {
    /// Seed with what was loaded at startup so the first tick doesn't write
    /// the same value straight back.
    pub(crate) fn seed(&mut self, bounds: Option<SavedBounds>) {
        self.last_saved = bounds;
    }

    /// Should `current` be persisted now? Returns `true` at most once per
    /// distinct value.
    pub(crate) fn should_save(&mut self, idle: bool, current: SavedBounds) -> bool {
        if !idle || self.last_saved == Some(current) {
            return false;
        }
        self.last_saved = Some(current);
        true
    }
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
    fn bounds_are_saved_once_per_change_and_never_mid_drag() {
        let b = |w, h| SavedBounds {
            width: w,
            height: h,
            maximized: false,
        };
        let mut auto = BoundsAutoSave::default();
        auto.seed(Some(b(1280, 720)));
        // Unchanged from what was loaded: nothing to write.
        assert!(!auto.should_save(true, b(1280, 720)));
        // Changed, but a mouse button is down (drag-resize in flight).
        assert!(!auto.should_save(false, b(1000, 700)));
        // Same change once the drag ends: written exactly once.
        assert!(auto.should_save(true, b(1000, 700)));
        assert!(!auto.should_save(true, b(1000, 700)));
        // Maximizing at the same size is still a change.
        assert!(auto.should_save(
            true,
            SavedBounds {
                width: 1000,
                height: 700,
                maximized: true
            }
        ));
    }

    #[test]
    fn first_save_happens_when_nothing_was_loaded() {
        let mut auto = BoundsAutoSave::default();
        auto.seed(None);
        assert!(auto.should_save(
            true,
            SavedBounds {
                width: 800,
                height: 600,
                maximized: false
            }
        ));
    }
}
