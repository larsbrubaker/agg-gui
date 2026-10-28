//! The `NodeEditor` interaction state machine: what a press started and
//! what the pointer is doing until the release. Split out of `mod.rs` to
//! keep that file under the 800-line guardrail; the handlers that move
//! between states live in `events` and `connect`.
//!
//! [`NodeEditor::abandon_interaction`] ends whatever is in flight when no
//! release will come: the pointer capture was lost, the window was
//! deactivated, or the host switched the interaction mode.

use std::collections::HashSet;

use crate::draw::SocketSide;
use crate::model::{NodeId, NoodleView, SocketTypeId};

use super::rubber_band::BandMode;

impl super::NodeEditor {
    /// End the gesture in flight without its release. A rubber band is
    /// cancelled (the selection held at its press comes back); a pan, zoom,
    /// node drag or property scrub keeps what it already applied live, as a
    /// release would, but opens no menu or inline editor; a noodle drag is
    /// dropped without connecting. Leaves the editor idle, no snap guides
    /// or noodle left on screen.
    pub(super) fn abandon_interaction(&mut self) {
        if self.cancel_rubber_band() {
            return;
        }
        if matches!(self.interaction, CanvasState::Idle) {
            return;
        }
        self.interaction = CanvasState::Idle;
        agg_gui::snap::clear_guides();
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
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
        /// The drag picked an existing noodle up off the input pressed
        /// (at once or deferred): a drop on empty canvas disconnects it
        /// instead of asking the host's add menu for a node to wire to.
        lifted: bool,
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
