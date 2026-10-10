//! Noodle endpoint resolution against the per-node layouts.
//!
//! Lives in its own file so [`super::mod`] stays under the 800-line
//! cap. The drop target a dragged noodle snaps to is chosen in
//! [`super::connect`].

use crate::draw::{NodeLayoutInfo, SocketLayout, SocketSide};
use crate::model::NoodleView;

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
