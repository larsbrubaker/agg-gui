//! The `NodeEditor` interaction state machine: what a press started and
//! what the pointer is doing until the release. Split out of `mod.rs` to
//! keep that file under the 800-line guardrail; the handlers that move
//! between states live in `events` and `connect`.

use crate::draw::SocketSide;
use crate::model::{NodeId, NoodleView, SocketTypeId};

/// Interaction state machine. Only one drag at a time.
#[derive(Clone, Debug)]
pub(super) enum CanvasState {
    Idle,
    PanningCanvas {
        start_offset: [f64; 2],
        start_local: agg_gui::Point,
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
