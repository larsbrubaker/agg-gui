//! Paint-cache fingerprint helpers — hash one composed row so the
//! canvas's child-widget tree invalidates whenever a row's
//! user-visible state changes.
//!
//! Pulled out of `widget/mod.rs` to keep that file under the
//! project-wide 800-line cap, together with
//! [`NodeEditor::compute_fingerprint`], which `layout()` calls.

use std::hash::Hash;

use crate::draw::NodeLayoutInfo;
use crate::draw::{NodeRow, PropLayout};
use crate::model::{NodeId, PropertyValue};

use super::NodeEditor;

/// Hash a single composed row's user-visible state into the canvas's
/// paint fingerprint. Property values must participate so that
/// drag-mutating a slider invalidates the cached child widget tree
/// and the pill repaints with the fresh number.
pub(super) fn hash_row<H: std::hash::Hasher>(row: &NodeRow, h: &mut H) {
    match row {
        NodeRow::Output(s) => {
            s.name.hash(h);
            s.display_label.hash(h);
            s.socket_type.0.hash(h);
            s.shape.hash(h);
        }
        NodeRow::Input { socket, editor, .. } => {
            socket.name.hash(h);
            socket.display_label.hash(h);
            socket.socket_type.0.hash(h);
            socket.shape.hash(h);
            socket.landed.hash(h);
            if let Some(e) = editor {
                hash_prop_layout(e, h);
            }
        }
        NodeRow::Property(p) => {
            hash_prop_layout(p, h);
        }
    }
}

fn hash_prop_layout<H: std::hash::Hasher>(p: &PropLayout, h: &mut H) {
    p.name.hash(h);
    p.display_label.hash(h);
    match &p.current {
        PropertyValue::Number(n) => {
            0u8.hash(h);
            n.to_bits().hash(h);
        }
        PropertyValue::Bool(b) => {
            1u8.hash(h);
            b.hash(h);
        }
        PropertyValue::Color(c) => {
            2u8.hash(h);
            for v in c.iter() {
                v.to_bits().hash(h);
            }
        }
        PropertyValue::Text(s) => {
            4u8.hash(h);
            s.hash(h);
        }
        PropertyValue::Other { display } => {
            3u8.hash(h);
            display.hash(h);
        }
    }
}

impl NodeEditor {
    /// Hash of every input that affects how the children's paint looks
    /// across one frame.  Mismatch between the previous fingerprint and
    /// the new one drives both the children rebuild and the GL FBO
    /// invalidation — paint outputs change ⇒ the cached texture must
    /// regenerate.
    ///
    /// Pan/zoom IS part of the fingerprint: layout bakes them into the
    /// child widgets' screen-space bounds (so the inspector tree picks
    /// them up correctly via `collect_inspector_nodes`), which means a
    /// pan/zoom change demands a children rebuild.
    pub(super) fn compute_fingerprint(
        &self,
        layouts: &[NodeLayoutInfo],
        ext_sel: Option<NodeId>,
    ) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        layouts.len().hash(&mut h);
        for l in layouts {
            l.node_id.0.hash(&mut h);
            l.top_left[0].to_bits().hash(&mut h);
            l.top_left[1].to_bits().hash(&mut h);
            l.size[0].to_bits().hash(&mut h);
            l.size[1].to_bits().hash(&mut h);
            l.display_name.hash(&mut h);
            l.category.hash(&mut h);
            l.rows.len().hash(&mut h);
            // Row content must participate in the fingerprint — without
            // it, dragging a slider mutates the underlying value but
            // the cached child widgets keep their stale `PropLayout`
            // and the value pill never repaints with the new number.
            for row in &l.rows {
                hash_row(row, &mut h);
            }
            let sel = self.selected.contains(&l.node_id) || ext_sel == Some(l.node_id);
            sel.hash(&mut h);
            l.collapsed.hash(&mut h);
            // An error arriving from an *asynchronous* host evaluation
            // changes nothing else about the layout, so without this the
            // badge would only appear (or clear) on the next unrelated
            // interaction that happened to dirty the fingerprint.
            l.error.hash(&mut h);
            l.warning.hash(&mut h);
        }
        self.canvas_offset[0].to_bits().hash(&mut h);
        self.canvas_offset[1].to_bits().hash(&mut h);
        self.canvas_scale.to_bits().hash(&mut h);
        // Theme epoch participates: every child NodeWidget bakes the
        // active `CanvasPalette` into its `NodePaintContext` at
        // rebuild time, so a light↔dark flip with no other model
        // change must still trigger `rebuild_children` — otherwise
        // the cached chrome (body, border, labels, sockets) keeps
        // painting in the old theme's colours.
        agg_gui::current_visuals_epoch().hash(&mut h);
        h.finish()
    }
}
