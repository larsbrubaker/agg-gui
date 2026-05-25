//! `NodeGraphModel` trait — the abstraction the [`crate::NodeEditor`] widget
//! reads/writes to drive any host application's node graph.
//!
//! The widget never talks to a concrete graph type; it always goes through
//! this trait. That means a host can plug in any backing store — an
//! AtomArtist [`atomartist_lib::Graph`], a `petgraph` `DiGraph`, a `serde_json`
//! `Value`, anything — by implementing this trait.
//!
//! # Snapshot model
//!
//! `nodes()` and `edges()` return owned `Vec`s every paint. That trades
//! a few allocations per frame for *zero* borrow-vs-mutate complexity in
//! the widget. With typical graph sizes (low hundreds of nodes / edges)
//! this is well under the 10 ms frame budget agg-gui targets — measured
//! on a representative AtomArtist scene the snapshot cost is < 50 µs.
//!
//! # Property editing
//!
//! Inline canvas editing (sliders, toggles) covers `Number` and `Bool`.
//! Anything else round-trips through [`PropertyValue::Other`] and is
//! displayed but not directly editable in the canvas — hosts surface
//! richer editors elsewhere (typically an inspector pane).

use agg_gui::Color;

/// Opaque node identifier — host-owned. The widget never inspects the
/// numeric value, only compares it for equality and uses it to fetch
/// node descriptors from the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u64);

/// Opaque socket-type identifier — host-owned. The widget uses
/// equality to determine connection compatibility (an output of type X
/// can only connect to an input of type X) and routes the id through
/// [`NodeGraphModel::socket_color`] to pick the connection's render
/// colour.  Hosts that want richer compatibility rules (subtyping,
/// implicit conversions) can override [`NodeGraphModel::sockets_compatible`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SocketTypeId(pub u32);

/// Inline-editable property value. Anything richer (Color, Geometry,
/// Path, Matrix) shows in the row but isn't drag-edited in the canvas.
#[derive(Clone, Debug)]
pub enum PropertyValue {
    Number(f64),
    Bool(bool),
    /// A 4-component RGBA color, components in 0..=1. The canvas paints
    /// a swatch; richer pickers live in host-side panels.
    Color([f32; 4]),
    /// Anything not covered by the above variants. The host provides a
    /// short display string so the canvas can render the row's value
    /// area; clicking does nothing (hosts route richer editing
    /// elsewhere — typically an inspector pane).
    Other {
        /// Short display string (≤ 24 chars typical) — what to show in
        /// the property row.
        display: String,
    },
}

impl PropertyValue {
    /// True when the value can be edited directly on the canvas (drag,
    /// toggle, etc.). Color / Matrix / Path / Geometry round-trip
    /// through richer host-side editors.
    pub fn is_editable_inline(&self) -> bool {
        matches!(self, Self::Number(_) | Self::Bool(_))
    }
}

/// Description of one socket (input or output) on a node.
#[derive(Clone, Debug)]
pub struct SocketView {
    /// Stable name — must round-trip through `try_add_noodle` /
    /// `remove_edges_to`.  Typically a `&'static str` interned by the
    /// host's node-type registry.
    pub name: String,
    /// Type id used for connection-compatibility checks + render
    /// colour. Hosts that don't care about typed connections can
    /// always return `SocketTypeId(0)`.
    pub socket_type: SocketTypeId,
    /// Optional display label — falls back to `name` when `None`.
    pub display_label: Option<String>,
}

impl SocketView {
    /// Convenience: the label to show in the row — `display_label` if
    /// set, otherwise `name`.
    pub fn label(&self) -> &str {
        self.display_label.as_deref().unwrap_or(&self.name)
    }
}

/// Editor hint — surfaces the host's choice of rich editor for a
/// property when the canvas's inline editor isn't enough.  Hosts forward
/// their schema-side hint (e.g. AtomArtist's
/// `atomartist_lib::registry::EditorKind`) into this enum so the canvas
/// can decide to open a richer popup on a row click.  Variants are
/// intentionally narrow — only the cases the canvas itself can act on
/// (open a colour-wheel popup, etc.) appear here; everything else uses
/// the canvas's default behaviour for the row's `PropertyValue`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorHint {
    /// Open the agg-gui `ColorWheelPicker` popup on click.  Applies to
    /// `PropertyValue::Color` rows only.
    Color,
}

/// One inline-editable property row on a node.
#[derive(Clone, Debug)]
pub struct PropertyView {
    pub name: String,
    /// Optional display label — falls back to `name` when `None`.
    pub display_label: Option<String>,
    pub current: PropertyValue,
    /// Numeric range — the canvas clamps drag deltas to `[min, max]`
    /// when editing. Ignored for non-numeric properties.
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// When `Some(socket_name)`, the property is rendered inline on
    /// that input socket's row instead of getting its own row, and the
    /// editor disappears once the socket is connected. Mirrors
    /// NodeDesigner's per-input fallback editor.
    pub bound_input: Option<String>,
    /// Optional host hint telling the canvas to use a richer popup
    /// editor (e.g. a colour wheel) instead of the default inline
    /// behaviour.  Hosts forward their schema's editor metadata here.
    pub editor: Option<EditorHint>,
    /// Full schema-side editor description — used by the per-kind row
    /// renderers (`paint_row` in agg-gui). When `None` the canvas
    /// falls back to its default text/value pill paint. Hosts
    /// declaring a `EditorKind::Slider` here get a slider row, etc.
    pub editor_kind: Option<agg_gui::widgets::EditorKind>,
}

impl PropertyView {
    /// Convenience: the label to show in the row — `display_label` if
    /// set, otherwise `name`.
    pub fn label(&self) -> &str {
        self.display_label.as_deref().unwrap_or(&self.name)
    }
}

/// Snapshot of a node — what the widget needs for one paint frame.
#[derive(Clone, Debug)]
pub struct NodeView {
    pub id: NodeId,
    /// Stable type identifier — used as the action id when the user
    /// picks "Add this node" from the right-click popup.
    pub type_id: String,
    /// Display name shown in the node header.
    pub display_name: String,
    /// Category — drives the title-bar colour via
    /// [`NodeGraphModel::category_color`].
    pub category: String,
    /// Top-left corner in canvas-space (Y-up — the canvas is Y-up like
    /// the rest of agg-gui).
    pub position: [f64; 2],
    pub inputs: Vec<SocketView>,
    pub outputs: Vec<SocketView>,
    pub properties: Vec<PropertyView>,
}

/// Snapshot of one edge.
#[derive(Clone, Debug)]
pub struct NoodleView {
    pub from_node: NodeId,
    pub from_socket: String,
    pub to_node: NodeId,
    pub to_socket: String,
}

/// Description of one node type — feeds the right-click "Add Node"
/// popup. Grouped by category; the canvas builds a submenu per
/// category and one item per type.
#[derive(Clone, Debug)]
pub struct NodeTypeView {
    pub type_id: String,
    pub display_name: String,
    pub category: String,
}

/// Outcome of an attempted noodle connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoodleResult {
    /// Noodle was added cleanly.
    Connected,
    /// The target input was already connected; the host removed the
    /// existing noodle and added the new one (hosts that don't allow
    /// multi-input or want different replacement semantics return
    /// `Rejected` and let the user disconnect manually).
    Replaced,
    /// Noodle was rejected — incompatible socket types, would create a
    /// cycle, or any other host-defined reason.
    Rejected,
}

/// The trait the [`crate::NodeEditor`] widget drives. Hosts implement
/// this against their own graph store.
///
/// All `&self` methods may be called multiple times per frame (paint
/// snapshot + hit-tests). Keep them cheap; if the underlying store is
/// expensive to read, cache snapshots with a "dirty" flag the host
/// flips on mutation.
pub trait NodeGraphModel {
    // ── Read snapshots ──────────────────────────────────────────────────

    /// Snapshot every node visible in the canvas — used for layout,
    /// hit-testing, and paint.
    fn nodes(&self) -> Vec<NodeView>;

    /// Snapshot every edge between visible nodes — used for paint.
    fn noodles(&self) -> Vec<NoodleView>;

    /// Right-click "Add Node" menu source — categories with the types
    /// that belong to each. Order is preserved in the menu.
    fn node_types_by_category(&self) -> Vec<(String, Vec<NodeTypeView>)>;

    /// "Primary selected" node id sourced *from outside the widget* —
    /// e.g. the host has another widget (a 3-D viewport, an inspector
    /// pane) that writes a selection that the canvas should also
    /// highlight.  The canvas reads this on every paint and OR's it
    /// with its own multi-select set.  Default returns `None`.
    fn primary_selection(&self) -> Option<NodeId> {
        None
    }

    // ── Visual mapping ──────────────────────────────────────────────────

    /// Connection / socket colour for a given socket type id. Default
    /// returns a neutral grey so hosts that don't care about typed
    /// sockets get a sensible fallback.
    fn socket_color(&self, _ty: SocketTypeId) -> Color {
        Color::rgba(0.55, 0.58, 0.66, 1.0)
    }

    /// Title-bar colour for nodes in `category`. Default returns the
    /// theme accent so an undecorated category still reads as a node.
    fn category_color(&self, _category: &str, fallback: Color) -> Color {
        fallback
    }

    /// Whether an output of `out_ty` can connect to an input of
    /// `in_ty`. Default: equality. Override for subtyping / coercion.
    fn sockets_compatible(&self, out_ty: SocketTypeId, in_ty: SocketTypeId) -> bool {
        out_ty == in_ty
    }

    // ── Mutation ────────────────────────────────────────────────────────

    /// Move a node. `pos` is canvas-space top-left.
    fn set_node_position(&mut self, id: NodeId, pos: [f64; 2]);

    /// Insert a node of `type_id` at canvas-space `pos`.  Returns the
    /// new node's id, or `None` if the host rejected the insertion.
    fn add_node(&mut self, type_id: &str, pos: [f64; 2]) -> Option<NodeId>;

    /// Remove a node and any edges incident to it.
    fn remove_node(&mut self, id: NodeId);

    /// Try to add an edge. The widget calls this on connection-drag
    /// release; it's responsible for socket-direction inference (the
    /// caller passes producer-first / consumer-second).
    fn try_add_noodle(
        &mut self,
        from_node: NodeId,
        from_socket: &str,
        to_node: NodeId,
        to_socket: &str,
    ) -> NoodleResult;

    /// Remove the noodle whose endpoints match exactly. Used by the
    /// disconnect-by-drag flow: the user clicks a connected input
    /// socket and drags away — the widget pops the existing noodle
    /// off and starts a re-attach drag from the source side.
    /// Returns `true` if a matching noodle was found + removed.
    fn remove_noodle(
        &mut self,
        from_node: NodeId,
        from_socket: &str,
        to_node: NodeId,
        to_socket: &str,
    ) -> bool;

    /// Update a property value (only invoked for `Number` / `Bool`).
    fn set_property(&mut self, id: NodeId, name: &str, value: PropertyValue);

    // ── Notification hooks ──────────────────────────────────────────────

    /// Called once per scroll-zoom event with the new zoom level.  Hosts
    /// that surface zoom in a status bar (typical) read it from here;
    /// hosts that don't care default to a no-op.
    fn on_canvas_zoom_changed(&mut self, _zoom: f64) {}

    /// Called when the user changes the primary selected node — the
    /// last-clicked node, or `None` after a click on empty canvas.
    /// Hosts use this to drive companion UI (3-D viewport outline,
    /// inspector pane) without polling.
    fn on_primary_selection_changed(&mut self, _id: Option<NodeId>) {}
}
