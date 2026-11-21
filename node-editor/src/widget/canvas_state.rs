//! The `NodeEditor` interaction state machine: what a press started and
//! what the pointer is doing until the release. Split out of `mod.rs` to
//! keep that file under the 800-line guardrail; the handlers that move
//! between states live in `events` and `connect`.
//!
//! [`NodeEditor::abandon_interaction`] ends whatever is in flight when no
//! release will come: the pointer capture was lost, the window was
//! deactivated, or the host switched the interaction mode.
//!
//! [`NodeEditor::cancel_drag`] is Escape during a card drag or a value
//! scrub: what the drag wrote live is put back to how it was at the press,
//! beside the noodle (`connect.rs`), rubber-band (`rubber_band.rs`) and
//! card-resize (`hosted_events.rs`) cancels, so every drag in the editor
//! answers Escape the same way. A selection the press made stays, as a
//! file manager keeps the selection when Escape cancels a drag of its
//! icons.
//!
//! A card drag, card resize and value scrub are announced to the host
//! (`NodeGraphModel::on_drag_started`) at their press and answered once,
//! by `on_drag_cancelled` from Escape or `on_drag_ended` from the release,
//! another press ([`NodeEditor::end_drag_for_press`]) or
//! [`NodeEditor::abandon_interaction`].

use std::collections::HashSet;

use crate::draw::SocketSide;
use crate::model::{NodeId, NoodleView, PropertyValue, SocketTypeId};

use super::rubber_band::BandMode;

impl super::NodeEditor {
    /// End the gesture in flight without its release. A rubber band is
    /// cancelled (the selection held at its press comes back); a pan, zoom,
    /// node drag or property scrub keeps what it already applied live, as a
    /// release would, but opens no menu or inline editor; a noodle drag is
    /// cancelled as Escape cancels it, a lifted noodle going back where it
    /// was. Leaves the editor idle, no snap guides or noodle left on screen.
    pub(super) fn abandon_interaction(&mut self) {
        if self.cancel_rubber_band() || self.cancel_connection() {
            return;
        }
        if matches!(self.interaction, CanvasState::Idle) {
            return;
        }
        let state = std::mem::replace(&mut self.interaction, CanvasState::Idle);
        if state.reports_drag() {
            self.model.lock().unwrap().on_drag_ended();
        }
        agg_gui::snap::clear_guides();
        self.invalidate_canvas();
        agg_gui::animation::request_draw();
    }

    /// A press while a card drag, card resize or value scrub is in flight
    /// (another button, say) ends it where it is, keeping what it wrote
    /// live as a release would, so the host hears `on_drag_ended` before
    /// whatever the new press starts.
    pub(super) fn end_drag_for_press(&mut self) {
        self.end_hosted_resize();
        if self.interaction.reports_drag() {
            self.interaction = CanvasState::Idle;
            agg_gui::snap::clear_guides();
            self.invalidate_canvas();
            self.model.lock().unwrap().on_drag_ended();
        }
    }

    /// Escape during a card drag or a numeric row's scrub: every dragged
    /// card that moved goes back to its position at the press (one more
    /// `set_node_position` each), and a scrub that wrote a value writes
    /// the value at the press (a press whose scrub wrote nothing yet writes
    /// nothing, and a NumberDrag one opens no inline editor). Then the
    /// host hears `on_drag_cancelled`. The editor goes idle, so the rest
    /// of the gesture moves and writes nothing. False, leaving the state
    /// alone, when neither is in flight.
    pub(super) fn cancel_drag(&mut self) -> bool {
        match std::mem::replace(&mut self.interaction, CanvasState::Idle) {
            CanvasState::DraggingNode {
                ids,
                start_positions,
                ..
            } => self.put_cards_back(&ids, &start_positions),
            CanvasState::DraggingProperty {
                node_id,
                prop_name,
                start_value,
                scrubbed,
                ..
            } => {
                if scrubbed {
                    self.model.lock().unwrap().set_property(
                        node_id,
                        &prop_name,
                        PropertyValue::Number(start_value),
                    );
                }
            }
            other => {
                self.interaction = other;
                return false;
            }
        }
        self.model.lock().unwrap().on_drag_cancelled();
        agg_gui::snap::clear_guides();
        self.invalidate_canvas();
        agg_gui::animation::request_draw();
        true
    }

    /// Write each of `ids` that is no longer at its `starts` entry back
    /// there, so a card the drag never moved costs the host no call.
    fn put_cards_back(&mut self, ids: &[NodeId], starts: &[[f64; 2]]) {
        let mut model = self.model.lock().unwrap();
        let nodes = model.nodes();
        for (id, start) in ids.iter().zip(starts) {
            let moved = nodes
                .iter()
                .find(|n| n.id == *id)
                .is_some_and(|n| n.position != *start);
            if moved {
                model.set_node_position(*id, *start);
            }
        }
    }
}

impl CanvasState {
    /// The states announced to the host by `on_drag_started`.
    pub(super) fn reports_drag(&self) -> bool {
        matches!(
            self,
            CanvasState::DraggingNode { .. } | CanvasState::DraggingProperty { .. }
        )
    }
}

/// Interaction state machine. Only one drag at a time.
#[derive(Clone, Debug)]
pub(super) enum CanvasState {
    Idle,
    PanningCanvas {
        start_offset: [f64; 2],
        start_local: agg_gui::Point,
        /// A right-press on empty canvas (`NodeEditor::with_right_drag_pan`):
        /// released within [`super::add_menu::CLICK_SLOP`] of the press it is
        /// a click and opens the add menu there; moved further it was a pan.
        menu_on_click: bool,
    },
    DraggingNode {
        ids: Vec<NodeId>,
        /// Per-node start position, captured at mousedown.
        start_positions: Vec<[f64; 2]>,
        start_canvas: [f64; 2],
    },
    /// Left-drag zoom (only reachable in [`InteractionMode::Zoom`]).
    /// Anchored on the press point: the canvas position under the
    /// pointer at mousedown stays under it for the whole drag.
    ZoomingCanvas {
        start_scale: f64,
        start_local: agg_gui::Point,
        /// Canvas-space position under the press, captured once so
        /// rounding in the running scale can't drift the anchor.
        anchor_canvas: [f64; 2],
    },
    /// A left press on empty canvas in Select mode (`rubber_band.rs`):
    /// a click until the pointer passes the click slop, then a selection
    /// rectangle from the press to the pointer, both corners in canvas
    /// space so the rectangle stays on the same cards if the view moves.
    RubberBand {
        start_local: agg_gui::Point,
        start_canvas: [f64; 2],
        cursor_canvas: [f64; 2],
        mode: BandMode,
        /// The selection held at the press, which Shift adds to, Ctrl/Cmd
        /// toggles against, and Escape restores.
        base: HashSet<NodeId>,
        /// True once the pointer has passed the click slop.
        active: bool,
    },
    DrawingConnection {
        from_node: NodeId,
        from_socket: String,
        from_canvas: [f64; 2],
        cursor_canvas: [f64; 2],
        from_socket_type: SocketTypeId,
        from_side: SocketSide,
        /// The noodle lifted off the input pressed, while the editor
        /// keeps it in the model until the drop
        /// (`NodeEditor::with_deferred_noodle_pickup`); not drawn in place.
        picked_up: Option<NoodleView>,
        /// The noodle the drag picked up off the input pressed (at once or
        /// deferred), as it was: a drop on empty canvas disconnects it
        /// instead of asking the host's add menu for a node to wire to, and
        /// a cancel puts it back (`NodeEditor::cancel_connection`).
        lifted: Option<NoodleView>,
        /// Why the socket under the pointer refuses the noodle, from
        /// [`crate::NodeGraphModel::can_connect`]; shown beside the pointer.
        refusal: Option<String>,
        /// The pointer in editor-local coordinates, for the refusal note.
        cursor_local: agg_gui::Point,
    },
    /// Click-and-horizontal-drag editing of a numeric property.
    ///
    /// Two contracts share this state:
    ///
    ///   - **NumberDrag** rows (`click_to_edit == true`) mirror the
    ///     standalone [`agg_gui::widgets::DragValue`]: a press does not
    ///     scrub until the pointer moves past a 3px threshold; a plain
    ///     click (release before the threshold) opens an inline keyboard
    ///     editor instead. Drag deltas honour `step` snapping.
    ///   - **Slider** rows (`click_to_edit == false`) keep NodeDesigner
    ///     parity: the press scrubs immediately (`dragging` starts
    ///     `true`) with no step snapping and no click-to-edit.
    DraggingProperty {
        node_id: NodeId,
        prop_name: String,
        start_value: f64,
        start_local_x: f64,
        min: Option<f64>,
        max: Option<f64>,
        /// Snap interval for drag deltas (`None`/`0.0` = no snap). Only
        /// applied on the NumberDrag path.
        step: Option<f64>,
        /// Decimal places to seed the inline keyboard editor with when a
        /// NumberDrag row is clicked without dragging.
        decimals: usize,
        /// True once the 3px drag threshold has been crossed. Slider rows
        /// start `true`; NumberDrag rows start `false`.
        dragging: bool,
        /// True once a move has written a value through `set_property`,
        /// so Escape before that writes nothing back (a Slider row is
        /// `dragging` from its press but writes only when moved).
        scrubbed: bool,
        /// True for NumberDrag rows — a threshold-less release opens the
        /// inline keyboard editor. False for Slider rows.
        click_to_edit: bool,
        /// The clicked row's editor-pill rect in **canvas space**:
        /// `[top_left_x, top_left_y, width, height]` with `top_left_y` the
        /// row's TOP edge (Y-up). Captured at press so a click-to-edit
        /// release can drop the inline editor exactly over the pill.
        pill_rect: [f64; 4],
    },
}
