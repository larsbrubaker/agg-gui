//! Collapse toggling and single-node drag snapping for [`NodeEditor`].
//!
//! Split out of `events.rs` (which calls both from its mouse handlers) to
//! keep that file under the 800-line guardrail. Both honour the host's
//! switches on the editor ([`NodeEditor::with_collapse_enabled`],
//! [`NodeEditor::with_snap_guides`]); the snap gate is read by the caller.

use crate::model::NodeId;

use super::NodeEditor;

impl NodeEditor {
    /// Toggle the per-node collapse flag and invalidate the retained
    /// canvas backbuffer so the change is visible next frame.
    /// Does nothing when the host turned collapse off
    /// ([`NodeEditor::with_collapse_enabled`]).
    pub(super) fn toggle_collapsed(&mut self, id: NodeId) {
        if !self.hosted.collapse_enabled {
            return;
        }
        if !self.collapsed_nodes.insert(id) {
            self.collapsed_nodes.remove(&id);
        }
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
    }
}

/// Run a single-node drag through the snap engine and overwrite
/// `position` with the snapped top-left corner.
///
/// Node positions are stored as `[x, y]` where `y` is the **top** edge
/// in Y-up canvas coords; the snap engine works in `Rect`s whose `y`
/// is the BOTTOM edge.  Conversion happens at the boundaries here so
/// the rest of the drag path keeps thinking in the node convention.
///
/// Guides are written into the framework's thread-local snap
/// registry; `NodeEditor::paint` reads them inside the canvas
/// transform to render alignment / spacing lines.
pub(super) fn snap_single_node(
    moving_id: NodeId,
    position: &mut [f64; 2],
    layouts: &[crate::draw::NodeLayoutInfo],
) {
    use agg_gui::{compute_snap, snap, Rect, SnapId, SnapMode};
    let Some(moving_layout) = layouts.iter().find(|l| l.node_id == moving_id) else {
        return;
    };
    let size = moving_layout.size;
    let raw_top_left = *position;
    let moving_rect = Rect::new(raw_top_left[0], raw_top_left[1] - size[1], size[0], size[1]);
    let targets: Vec<(SnapId, Rect)> = layouts
        .iter()
        .filter(|l| l.node_id != moving_id)
        .map(|l| {
            (
                SnapId(l.node_id.0),
                Rect::new(
                    l.top_left[0],
                    l.top_left[1] - l.size[1],
                    l.size[0],
                    l.size[1],
                ),
            )
        })
        .collect();
    let result = compute_snap(
        moving_rect,
        SnapId(moving_id.0),
        &targets,
        snap::DEFAULT_THRESHOLD,
        SnapMode::Move,
    );
    // Convert the snapped rect back to top-left position.
    *position = [result.rect.x, result.rect.y + result.rect.height];
    snap::set_guides(result.guides);
}
