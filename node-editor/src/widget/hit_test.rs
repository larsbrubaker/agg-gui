//! Canvas-level hit tests for [`NodeEditor`]: which card, socket or
//! property row a canvas-space point lands on, and whether a card wears
//! its selection ring.
//!
//! Every test walks the layouts from `snapshot_layouts` backwards. That
//! list is in paint order (`hosted::order_layouts`: simplified cards,
//! selected on top, then the hosted cards bottom → top in `hosted.order`,
//! which a press re-orders while raise-on-click is on), so the first card
//! found is the one the user sees on top. A socket or row of a card that
//! lies under another card's body is hidden there, so the walk stops at
//! the first body containing the point.

use crate::draw::{NodeLayoutInfo, PropLayout, SocketLayout};
use crate::model::NodeId;

use super::{CanvasState, NodeEditor};

impl NodeEditor {
    /// The top-most card whose body contains `canvas_pos`.
    pub(super) fn hit_node(
        &self,
        layouts: &[NodeLayoutInfo],
        canvas_pos: [f64; 2],
    ) -> Option<NodeId> {
        layouts
            .iter()
            .rev()
            .find(|l| l.body_contains(canvas_pos))
            .map(|l| l.node_id)
    }

    /// The visible socket a noodle dropped at `canvas_pos` lands on, of
    /// the top-most card that has one there; `None` where a card above
    /// covers it. The whole hit area, zoom-grown, on either side of the
    /// card's edge ([`crate::draw::socket_hit`]).
    pub(super) fn hit_socket(
        &self,
        layouts: &[NodeLayoutInfo],
        canvas_pos: [f64; 2],
    ) -> Option<(NodeId, SocketLayout)> {
        self.socket_hit_by(layouts, canvas_pos, false)
    }

    /// The visible socket a press (or hover) at `canvas_pos` takes: as
    /// [`Self::hit_socket`], but on the card's own body no further in from
    /// the socket than [`crate::draw::SOCKET_INNER_REACH`], so a body
    /// widget there gets the press. The hosted card's own claim
    /// (`HostedCard::claims_pointer_exclusively`) follows the same rule, so
    /// a press resolves the same whichever path sees it first.
    pub(super) fn press_socket(
        &self,
        layouts: &[NodeLayoutInfo],
        canvas_pos: [f64; 2],
    ) -> Option<(NodeId, SocketLayout)> {
        self.socket_hit_by(layouts, canvas_pos, true)
    }

    fn socket_hit_by(
        &self,
        layouts: &[NodeLayoutInfo],
        canvas_pos: [f64; 2],
        press: bool,
    ) -> Option<(NodeId, SocketLayout)> {
        let half = self.socket_hit_half();
        for l in layouts.iter().rev() {
            let hit = if press {
                l.socket_pressed_at(canvas_pos, half)
            } else {
                l.socket_within(canvas_pos, half)
            };
            if let Some(s) = hit {
                return Some((l.node_id, s.clone()));
            }
            if l.body_contains(canvas_pos) {
                return None;
            }
        }
        None
    }

    /// The property row at `canvas_pos` of the top-most card there.
    pub(super) fn hit_property(
        &self,
        layouts: &[NodeLayoutInfo],
        canvas_pos: [f64; 2],
    ) -> Option<(NodeId, PropLayout)> {
        for l in layouts.iter().rev() {
            if let Some(p) = l.prop_at(canvas_pos) {
                return Some((l.node_id, p.clone()));
            }
            if l.body_contains(canvas_pos) {
                return None;
            }
        }
        None
    }

    /// Whether card `id` draws its selection ring. The host's primary
    /// selection (`ext_sel`) rings a card the editor did not select, except
    /// while a rubber band is being dragged: then the rings preview exactly
    /// the selection the release will make.
    pub(super) fn shows_ring(&self, id: NodeId, ext_sel: Option<NodeId>) -> bool {
        if self.selected.contains(&id) {
            return true;
        }
        let band = matches!(
            self.interaction,
            CanvasState::RubberBand { active: true, .. }
        );
        !band && ext_sel == Some(id)
    }
}

/// Index in the paint-ordered `layouts` of the top-most card whose body
/// contains `p`. As in [`NodeEditor::press_socket`], a press at `p` can reach
/// the sockets of that card and of the cards above it, never those below.
pub(super) fn top_body_at(layouts: &[NodeLayoutInfo], p: [f64; 2]) -> Option<usize> {
    layouts.iter().rposition(|l| l.body_contains(p))
}
