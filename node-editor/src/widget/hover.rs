//! Drop-target hover logic for the in-progress noodle drag.
//!
//! Lives in its own file so [`super::mod`] stays under the 800-line
//! cap. The helper finds the best socket to highlight as the user
//! drags a connection: opposite side, not the source node, compatible
//! type, within a snap radius of the cursor.

use crate::draw::{NodeLayoutInfo, SocketLayout, SocketSide};
use crate::model::{NodeGraphModel, NodeId, NoodleView, SocketTypeId};

/// Resolve a noodle's `(from, to)` endpoint sockets against the
/// per-node layouts that paint just produced.
///
/// Looks up each endpoint side-restricted: `from` is the source-side
/// socket of an output, `to` is the target-side socket of an input.
/// This matters when a node carries both an input and an output that
/// share a name — e.g. AtomArtist's unified `Output` node, whose
/// adopted input slot and mirror output socket both take the source
/// socket's name. Without the side filter, the name lookup hits the
/// output-first row order and the noodle's `to` endpoint snaps to the
/// wrong side of the node.
pub(crate) fn resolve_noodle_endpoints<'a>(
    layouts: &'a [NodeLayoutInfo],
    noodle: &NoodleView,
) -> Option<(&'a SocketLayout, &'a SocketLayout)> {
    let from = layouts
        .iter()
        .find(|l| l.node_id == noodle.from_node)
        .and_then(|l| {
            l.sockets()
                .find(|s| s.side == SocketSide::Output && s.name == noodle.from_socket)
        })?;
    let to = layouts
        .iter()
        .find(|l| l.node_id == noodle.to_node)
        .and_then(|l| {
            l.sockets()
                .find(|s| s.side == SocketSide::Input && s.name == noodle.to_socket)
        })?;
    Some((from, to))
}

/// Find a socket within the drop-snap radius of `cursor_canvas` that's
/// a valid drop target for the in-progress connection — the right
/// side (Input if dragging from Output, vice versa), not the source
/// node itself, and compatible socket types. Used to draw the hover
/// halo while the user drags a noodle.
pub(super) fn find_compatible_socket_near<'a>(
    layouts: &'a [NodeLayoutInfo],
    model: &dyn NodeGraphModel,
    cursor_canvas: [f64; 2],
    from_node: NodeId,
    from_side: SocketSide,
    from_socket_type: SocketTypeId,
) -> Option<&'a SocketLayout> {
    let snap_r = crate::draw::SOCKET_HIT_RADIUS * 1.6;
    let want_side = match from_side {
        SocketSide::Output => SocketSide::Input,
        SocketSide::Input => SocketSide::Output,
    };
    let mut best: Option<(&SocketLayout, f64)> = None;
    for l in layouts {
        if l.node_id == from_node {
            continue;
        }
        for s in l.sockets() {
            if s.side != want_side {
                continue;
            }
            let compatible = match from_side {
                SocketSide::Output => model.sockets_compatible(from_socket_type, s.socket_type),
                SocketSide::Input => model.sockets_compatible(s.socket_type, from_socket_type),
            };
            if !compatible {
                continue;
            }
            let dx = s.center[0] - cursor_canvas[0];
            let dy = s.center[1] - cursor_canvas[1];
            let d2 = dx * dx + dy * dy;
            if d2 <= snap_r * snap_r && best.map(|(_, b)| d2 < b).unwrap_or(true) {
                best = Some((s, d2));
            }
        }
    }
    best.map(|(s, _)| s)
}
