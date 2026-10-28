//! `NodeEditor::arrange_plan`: the editor side of the automatic layout in
//! [`crate::arrange`]. It describes the live graph with the cards' real
//! sizes and socket heights — hosted cards from their measured geometry,
//! simplified cards from their row layout, both through the editor's
//! own `snapshot_layouts` — and the roles from
//! [`crate::NodeGraphModel::arrange_role`], then returns the moves
//! without applying them, so a host can apply them as one undo step.

use std::collections::BTreeMap;

use super::NodeEditor;
use crate::arrange::{arrange, ArrangeEdge, ArrangeNode, ArrangeOptions};
use crate::draw::SocketSide;
use crate::model::NodeId;

#[cfg(test)]
#[path = "tests_arrange.rs"]
mod tests;

impl NodeEditor {
    /// The arranged position (canvas top-left, y up) of every node the
    /// arrangement moves, in the model's node order. Nothing is applied:
    /// the host moves the nodes (through
    /// [`crate::NodeGraphModel::set_node_position`] or its own undoable
    /// command). An empty plan means the graph is already arranged.
    ///
    /// Card sizes and socket heights are the ones the last `layout()`
    /// measured, so call it on an editor that has been laid out.
    pub fn arrange_plan(&self, options: &ArrangeOptions) -> Vec<(NodeId, [f64; 2])> {
        let layouts = self.snapshot_layouts();
        let model = self.model.lock().unwrap();
        let nodes = model.nodes();
        let noodles = model.noodles();
        let roles: Vec<_> = nodes.iter().map(|n| model.arrange_role(n.id)).collect();
        drop(model);

        let layout_of: BTreeMap<u64, usize> = layouts
            .iter()
            .enumerate()
            .map(|(i, l)| (l.node_id.0, i))
            .collect();
        let mut described: Vec<ArrangeNode> = Vec::with_capacity(nodes.len());
        for (n, role) in nodes.iter().zip(roles) {
            let Some(l) = layout_of.get(&n.id.0).map(|&i| &layouts[i]) else {
                continue;
            };
            let top = l.top_left[1];
            // A socket the layout does not place (out of the card) gets
            // the card's middle from the arrange module.
            let offset = |side: SocketSide, name: &str| {
                l.sockets()
                    .find(|s| s.side == side && s.name == name)
                    .map_or(f64::NAN, |s| top - s.center[1])
            };
            described.push(ArrangeNode {
                id: n.id,
                size: l.size,
                role,
                input_ports: n
                    .inputs
                    .iter()
                    .map(|s| offset(SocketSide::Input, &s.name))
                    .collect(),
                output_ports: n
                    .outputs
                    .iter()
                    .map(|s| offset(SocketSide::Output, &s.name))
                    .collect(),
                position: n.position,
            });
        }

        let socket_index = |id: NodeId, outputs: bool, name: &str| {
            nodes.iter().find(|n| n.id == id).map_or(usize::MAX, |n| {
                let list = if outputs { &n.outputs } else { &n.inputs };
                list.iter()
                    .position(|s| s.name == name)
                    .unwrap_or(usize::MAX)
            })
        };
        let edges: Vec<ArrangeEdge> = noodles
            .iter()
            .map(|e| ArrangeEdge {
                from: (e.from_node, socket_index(e.from_node, true, &e.from_socket)),
                to: (e.to_node, socket_index(e.to_node, false, &e.to_socket)),
            })
            .collect();

        let current: BTreeMap<u64, [f64; 2]> = nodes.iter().map(|n| (n.id.0, n.position)).collect();
        arrange(&described, &edges, options)
            .into_iter()
            .filter(|(id, pos)| current.get(&id.0) != Some(pos))
            .collect()
    }
}
