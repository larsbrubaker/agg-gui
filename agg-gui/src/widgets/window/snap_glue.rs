//! Snap-layout glue for [`super::Window`]: the drag/resize snap passes, the
//! internal→engine direction mapping, and the [`Snappable`](crate::snap::Snappable)
//! trait impl. Extracted from `window.rs` so that file stays under the 800-line
//! guardrail; the logic is unchanged, it just lives in its own cohesive module.

use super::{snap, ResizeDir, Window};
use crate::geometry::Rect;

impl Window {
    /// Snap pass for a title-bar drag.  Skipped entirely when the
    /// global toggle is off — cheap when not in use.  Replaces
    /// `self.bounds` with the engine's snapped result and writes the
    /// guide list for `SnapOverlay` to paint.
    pub(crate) fn apply_move_snap(&mut self) {
        if !crate::snap::is_enabled() {
            crate::snap::clear_guides();
            return;
        }
        let targets = crate::snap::targets_snapshot();
        let result = crate::snap::compute_snap(
            self.bounds,
            self.snap_id,
            &targets,
            crate::snap::DEFAULT_THRESHOLD,
            crate::snap::SnapMode::Move,
        );
        self.bounds = snap(result.rect);
        crate::snap::set_guides(result.guides);
    }

    /// Snap pass for an edge / corner resize drag.  Only edges that
    /// the active handle is allowed to move can snap — the engine
    /// enforces that internally via `SnapMode::Resize`.
    pub(crate) fn apply_resize_snap(&mut self, dir: ResizeDir) {
        if !crate::snap::is_enabled() {
            crate::snap::clear_guides();
            return;
        }
        let targets = crate::snap::targets_snapshot();
        let edge = resize_dir_to_snap_edge(dir);
        let result = crate::snap::compute_snap(
            self.bounds,
            self.snap_id,
            &targets,
            crate::snap::DEFAULT_THRESHOLD,
            crate::snap::SnapMode::Resize(edge),
        );
        self.bounds = snap(result.rect);
        crate::snap::set_guides(result.guides);
    }
}

/// Map an internal `ResizeDir` to the snap engine's compass-direction
/// enum.  Kept private — the snap engine owns its own enum so the
/// engine isn't coupled to the Window widget.
fn resize_dir_to_snap_edge(dir: ResizeDir) -> crate::snap::ResizeEdge {
    use crate::snap::ResizeEdge as E;
    match dir {
        ResizeDir::N => E::North,
        ResizeDir::NE => E::NorthEast,
        ResizeDir::E => E::East,
        ResizeDir::SE => E::SouthEast,
        ResizeDir::S => E::South,
        ResizeDir::SW => E::SouthWest,
        ResizeDir::W => E::West,
        ResizeDir::NW => E::NorthWest,
    }
}

impl crate::snap::Snappable for Window {
    fn snap_id(&self) -> crate::snap::SnapId {
        self.snap_id
    }
    fn snap_rect(&self) -> Rect {
        self.bounds
    }
    fn set_snap_rect(&mut self, r: Rect) {
        self.bounds = snap(r);
    }
    fn is_snap_source(&self) -> bool {
        self.requested_visible() && !self.maximized
    }
    fn is_snap_target(&self) -> bool {
        // Maximized windows fill the canvas — pulling siblings to
        // their edges would just glue everything to the canvas
        // perimeter, which isn't useful as a layout aid.  Hidden
        // windows aren't valid targets either.
        self.requested_visible() && !self.maximized
    }
}
