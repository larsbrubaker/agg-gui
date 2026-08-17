//! Paint-cache fingerprint helpers — hash one composed row so the
//! canvas's child-widget tree invalidates whenever a row's
//! user-visible state changes.
//!
//! Pulled out of `widget/mod.rs` to keep that file under the
//! project-wide 800-line cap. The canonical caller is
//! [`super::NodeEditor::compute_fingerprint`].

use std::hash::Hash;

use crate::draw::{NodeRow, PropLayout};
use crate::model::PropertyValue;

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
        }
        NodeRow::Input { socket, editor, .. } => {
            socket.name.hash(h);
            socket.display_label.hash(h);
            socket.socket_type.0.hash(h);
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
