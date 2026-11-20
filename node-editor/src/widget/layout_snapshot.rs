//! The editor's view of the model's nodes as laid-out cards, for
//! [`NodeEditor`]: [`NodeEditor::snapshot_layouts`] reads every node's
//! layout from the model (hit-testing, noodles, snapping, arranging and the
//! paint cache read through it), and [`NodeEditor::rebuild_children`] turns
//! those layouts into the editor's child widgets — a `NodeWidget` per plain
//! card, plus the hosted cards' layer (`hosted`).
//!
//! Split out of `mod.rs` for the 800-line guardrail.

use std::rc::Rc;

use agg_gui::Widget;

use super::{presentation, NodeEditor};
use crate::draw::{layout_node_with_state, NodeLayoutInfo};
use crate::model::NodeId;
use crate::widget::nodes::{NodePaintContext, NodeWidget};

impl NodeEditor {
    /// Compute layouts for every node currently in the model. Layouts
    /// are returned in a deterministic order (selected last so they
    /// paint on top). Bound-input editors are suppressed for sockets
    /// that already have an incoming edge — that's the "data flowing
    /// in" rule from NodeDesigner's row layout.
    pub(super) fn snapshot_layouts(&self) -> Vec<NodeLayoutInfo> {
        let model = self.model.lock().unwrap();
        let nodes = model.nodes();
        let noodles = model.noodles();
        let ext_sel = model.primary_selection();
        drop(model);
        // Index of connected input sockets keyed by `(node_id, socket_name)`.
        let connected: std::collections::HashSet<(NodeId, String)> = noodles
            .iter()
            .map(|e| (e.to_node, e.to_socket.clone()))
            .collect();
        let mut layouts: Vec<NodeLayoutInfo> = nodes
            .iter()
            .map(|n| {
                let collapsed = self.collapsed_nodes.contains(&n.id);
                layout_node_with_state(
                    n,
                    |sock| connected.contains(&(n.id, sock.to_string())),
                    collapsed,
                )
            })
            .collect();
        self.order_layouts(&nodes, &mut layouts, ext_sel);
        let model = self.model.lock().unwrap();
        presentation::apply_socket_styles(&*model, &noodles, &mut layouts);
        layouts
    }

    /// Tear down `self.children` and build a fresh `Vec<NodeWidget>`
    /// from `layouts`.  Bounds are in **screen-space** — the canvas
    /// pan/zoom is baked into each NodeWidget's position and size so
    /// the framework's per-child translate (which adds bounds in
    /// screen-space, not in pre-transform space) lands at the right
    /// pixels AND `collect_inspector_nodes` sees the on-screen rect.
    pub(super) fn rebuild_children(&mut self, layouts: &[NodeLayoutInfo], ext_sel: Option<NodeId>) {
        let visuals = agg_gui::current_visuals();
        let palette = self.resolve_palette(&visuals);
        let model = self.model.lock().unwrap();
        let mut node_ctx = NodePaintContext::from_model(palette, &*model);
        node_ctx.noodle_style = self.presentation.noodle_style;
        drop(model);

        let scale = self.canvas_scale;
        let offset = self.canvas_offset;
        // Sockets off: draw the dots of a copy marked hidden; hit-testing
        // reads the live layouts, so the sockets still take presses.
        let undrawn;
        let layouts = if self.presentation.show_sockets {
            layouts
        } else {
            undrawn = presentation::undrawn_sockets(layouts);
            &undrawn[..]
        };
        let layer = self.take_hosted_layer();
        let mut new_children: Vec<Box<dyn Widget>> = Vec::with_capacity(layouts.len() + 1);
        for l in layouts
            .iter()
            .filter(|l| !self.hosted.cards.contains_key(&l.node_id))
        {
            let selected = self.shows_ring(l.node_id, ext_sel);
            let nw = NodeWidget::from_layout_transformed(
                l,
                selected,
                node_ctx.clone(),
                scale,
                offset,
                Rc::clone(&self.pending_collapse),
            );
            new_children.push(Box::new(nw));
        }
        new_children.extend(layer);
        self.children = new_children;
    }
}
