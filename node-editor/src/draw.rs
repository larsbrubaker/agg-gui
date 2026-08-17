//! Drawing helpers for the node-editor canvas — split from `widget.rs`
//! to respect the 800-line limit.
//!
//! All coordinates are **canvas-space**: positive Y is up (agg-gui
//! convention), and a node's `position` is its top-left corner. The caller
//! has already `save()`d, `translate()`d by `canvas_offset`, and `scale()`d
//! by `canvas_scale` on the `DrawCtx` before invoking these helpers, so we
//! draw straight in canvas units.
//!
//! # Node row composition
//!
//! Each node is laid out as a vertical stack of rows under the title bar.
//! The row order mirrors NodeDesigner:
//!
//! 1. **Output rows** first — one per output socket. The attach dot sits
//!    on the right edge of the row; the label hugs the dot.
//! 2. **Input rows** next — one per input socket. The attach dot sits on
//!    the left edge; the label follows. If the input has a
//!    `bound_input`-tagged property and the socket isn't connected, the
//!    property's inline editor is drawn on the right side of the same
//!    row.
//! 3. **Unbound property rows** last — every property whose
//!    `bound_input` is `None`. These behave like the legacy node-level
//!    property rows.
//!
//! A [`NodeRow`] captures everything one row needs: which side it
//! belongs to, an optional socket, an optional editor, and the row's
//! canvas-space rectangle. The widget hit-tests against `NodeRow`s
//! directly, so the layout is the single source of truth for visuals +
//! interaction.

use agg_gui::{Color, DrawCtx};

use crate::model::{NodeId, NodeView, PropertyValue, PropertyView, SocketTypeId};

// --- Layout constants ------------------------------------------------------

/// Minimum width every node renders at. Nodes whose labels need
/// more space auto-grow past this — see [`compute_node_width`].
pub const NODE_WIDTH: f64 = 200.0;
pub const TITLE_HEIGHT: f64 = 26.0;
pub const ROW_HEIGHT: f64 = 22.0;
pub const NODE_BOTTOM_PAD: f64 = 6.0;
pub const SOCKET_RADIUS: f64 = 5.5;
pub const SOCKET_HIT_RADIUS: f64 = 9.0;
pub const NODE_RADIUS: f64 = 6.0;
/// Right-side reserved width for an inline editor on an input row.
pub const EDITOR_WIDTH: f64 = 90.0;
/// Horizontal padding between the socket dot / row edge and the label.
pub(crate) const ROW_PADDING_X: f64 = 6.0;
/// Padding from the row edge to the label baseline.
pub(crate) const LABEL_OFFSET_Y: f64 = 14.0;

/// Side a socket appears on, in node-local coordinates (canvas Y-up).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketSide {
    Input,
    Output,
}

/// A single socket's hit-test info — its name, type id, computed
/// canvas-space position, and which side of the node it sits on.
#[derive(Clone, Debug)]
pub struct SocketLayout {
    pub side: SocketSide,
    pub name: String,
    pub display_label: String,
    pub socket_type: SocketTypeId,
    /// Canvas-space center of the socket circle.
    pub center: [f64; 2],
}

/// One editable property hit-rect inside a node — either bound to an
/// input row, or standing alone in the unbound-property section. The
/// widget uses these for click-drag editing.
#[derive(Clone, Debug)]
pub struct PropLayout {
    pub name: String,
    /// Optional display label override — `None` falls back to `name`.
    /// Reflects MatterCAD's `[DisplayName("…")]` attribute (the
    /// host-side property panel uses `display_label` whenever the
    /// schema declared one).
    pub display_label: Option<String>,
    /// Numeric range, copied from the model's `PropertyView`. Used to
    /// clamp drag deltas on number drags.
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub current: PropertyValue,
    /// Host's editor hint, copied from the model's `PropertyView`.
    /// Drives richer popups (today: the ColorWheelPicker dialog on
    /// `EditorHint::Color`).
    pub editor: Option<crate::model::EditorHint>,
    /// Full editor description from agg-gui's property-row vocabulary
    /// — drives the per-kind row renderers (`paint_row`). `None`
    /// means the row falls back to default value-pill paint.
    pub editor_kind: Option<agg_gui::widgets::EditorKind>,
    /// Canvas-space top-left (y at the row top edge) + size of the
    /// hit-test rectangle. Y-up: `top_left.y` is the row's top edge,
    /// `top_left.y - size.y` is the bottom edge.
    pub top_left: [f64; 2],
    pub size: [f64; 2],
    /// `true` when the rect above is the **whole** row and the row
    /// renderer paints the label inside it (unbound property rows);
    /// `false` when it is only the editor pill on an input socket's row,
    /// whose label a sibling widget paints. Hit-testing inside the
    /// editor (which segment of an enum strip did the user click?) has
    /// to know which of the two it is — see
    /// [`agg_gui::widgets::enum_variant_at`].
    pub full_row: bool,
}

impl PropLayout {
    /// The text the renderer should show as the row's label —
    /// `display_label` when present, else `name`.
    pub fn label(&self) -> &str {
        self.display_label.as_deref().unwrap_or(&self.name)
    }

    pub fn contains(&self, canvas_pos: [f64; 2]) -> bool {
        let x0 = self.top_left[0];
        let y1 = self.top_left[1];
        let x1 = x0 + self.size[0];
        let y0 = y1 - self.size[1];
        canvas_pos[0] >= x0 && canvas_pos[0] <= x1 && canvas_pos[1] >= y0 && canvas_pos[1] <= y1
    }
}

/// One composed row inside a node — either an output socket row, an
/// input socket row (optionally with a bound inline editor), or a
/// standalone property row. The visuals + hit-rects are all derived
/// from this struct.
#[derive(Clone, Debug)]
pub enum NodeRow {
    /// Output socket row — dot on the right, label hugging the dot.
    Output(SocketLayout),
    /// Input socket row — dot on the left, label, optional inline
    /// editor on the right.
    Input {
        socket: SocketLayout,
        editor: Option<PropLayout>,
        /// The row's own height (see [`NodeRow::height`]). Stored
        /// rather than assumed: a bound property can be taller than one
        /// [`ROW_HEIGHT`], and the row keeps that even when the editor
        /// is dropped because the socket is connected.
        height: f64,
    },
    /// Standalone property row — no socket attachment, just an inline
    /// editor that spans most of the row.
    Property(PropLayout),
}

impl NodeRow {
    /// Canvas-space row rectangle (top edge, size).
    pub fn row_rect(&self) -> ([f64; 2], [f64; 2]) {
        match self {
            NodeRow::Output(s) | NodeRow::Input { socket: s, .. } => {
                // We don't track the row's own rect explicitly on the socket
                // (its `center` is in the middle of the row), so reconstruct
                // it from the socket center. The dot stays centred on the
                // first ROW_HEIGHT of the row even when the row is taller.
                let top = s.center[1] + ROW_HEIGHT * 0.5;
                ([0.0, top], [NODE_WIDTH, self.height()])
            }
            NodeRow::Property(p) => (p.top_left, p.size),
        }
    }

    /// The row's own height in canvas units — **the** stacking pitch.
    ///
    /// Both the layout pass ([`layout_node_with_state`]) and the widget
    /// builder (`NodeRowWidget::from_row`) advance by exactly this, so
    /// what the user sees and what the canvas hit-tests cannot drift.
    /// They did: rows were painted at a fixed `ROW_HEIGHT` pitch while
    /// the layout advanced by [`row_height_for_property`], and a
    /// read-only string row (two rows tall) pushed everything below it
    /// out of reach of its own clicks.
    pub fn height(&self) -> f64 {
        match self {
            NodeRow::Output(_) => ROW_HEIGHT,
            NodeRow::Input { height, .. } => *height,
            NodeRow::Property(p) => p.size[1],
        }
    }

    pub fn socket(&self) -> Option<&SocketLayout> {
        match self {
            NodeRow::Output(s) | NodeRow::Input { socket: s, .. } => Some(s),
            NodeRow::Property(_) => None,
        }
    }

    pub fn editor(&self) -> Option<&PropLayout> {
        match self {
            NodeRow::Input { editor, .. } => editor.as_ref(),
            NodeRow::Property(p) => Some(p),
            NodeRow::Output(_) => None,
        }
    }
}

/// Computed canvas-space layout for one node — its size and the
/// ordered row list. Recomputed on each paint frame; cheap.
#[derive(Clone, Debug)]
pub struct NodeLayoutInfo {
    pub node_id: NodeId,
    pub top_left: [f64; 2],
    pub size: [f64; 2],
    pub rows: Vec<NodeRow>,
    pub display_name: String,
    pub category: String,
    /// True when this layout was produced for a collapsed node — the
    /// body is title-bar-only and rows hold sockets only (anchored at
    /// the title-bar side-center for noodle endpoints).
    pub collapsed: bool,
    /// Carried through from [`NodeView::error`] so the paint pass can
    /// badge a node whose evaluation refused. Layout is unaffected: the
    /// badge lives inside the existing title bar.
    pub error: Option<String>,
}

impl NodeLayoutInfo {
    /// Hit-test the node body. Returns true if the canvas-space point
    /// lies inside the rounded body.
    pub fn body_contains(&self, canvas_pos: [f64; 2]) -> bool {
        let x0 = self.top_left[0];
        let y1 = self.top_left[1];
        let y0 = y1 - self.size[1];
        let x1 = x0 + self.size[0];
        canvas_pos[0] >= x0 && canvas_pos[0] <= x1 && canvas_pos[1] >= y0 && canvas_pos[1] <= y1
    }

    /// True if the canvas-space point lies inside the title bar (top
    /// `TITLE_HEIGHT` strip).
    pub fn header_contains(&self, canvas_pos: [f64; 2]) -> bool {
        let x0 = self.top_left[0];
        let y1 = self.top_left[1];
        let y0 = y1 - TITLE_HEIGHT;
        let x1 = x0 + self.size[0];
        canvas_pos[0] >= x0 && canvas_pos[0] <= x1 && canvas_pos[1] >= y0 && canvas_pos[1] <= y1
    }

    /// All socket layouts on this node (flattened across rows). Returned
    /// as an iterator so the caller can chain other queries cheaply.
    pub fn sockets(&self) -> impl Iterator<Item = &SocketLayout> {
        self.rows.iter().filter_map(NodeRow::socket)
    }

    /// All property hit-rects on this node (flattened across rows).
    pub fn props(&self) -> impl Iterator<Item = &PropLayout> {
        self.rows.iter().filter_map(NodeRow::editor)
    }

    /// Find a socket whose hit radius contains `canvas_pos`.
    pub fn socket_at(&self, canvas_pos: [f64; 2]) -> Option<&SocketLayout> {
        self.sockets().find(|s| {
            let dx = s.center[0] - canvas_pos[0];
            let dy = s.center[1] - canvas_pos[1];
            dx * dx + dy * dy <= SOCKET_HIT_RADIUS * SOCKET_HIT_RADIUS
        })
    }

    /// Find the property row hit by `canvas_pos`.
    pub fn prop_at(&self, canvas_pos: [f64; 2]) -> Option<&PropLayout> {
        self.props().find(|p| p.contains(canvas_pos))
    }
}

/// Estimated rendered width of one ASCII glyph at the body font
/// size. Matches the constant used throughout the renderers; kept in
/// one place so width computation stays consistent.
const GLYPH_WIDTH: f64 = 6.5;

/// Compute the auto-sized width for `node`. The width is the bigger of
/// [`NODE_WIDTH`] and "longest label + reserved editor area + edge
/// padding" so a node like Extrude (with a `Bottom Segments` row)
/// never clips its label into the editor pill.
fn compute_node_width(node: &NodeView) -> f64 {
    let mut max_label = node.display_name.chars().count();
    for s in &node.outputs {
        max_label = max_label.max(s.label().chars().count());
    }
    for s in &node.inputs {
        max_label = max_label.max(s.label().chars().count());
    }
    for p in &node.properties {
        max_label = max_label.max(p.label().chars().count());
    }
    // Padding budget:
    //   - 12 px socket dot + ROW_PADDING_X gap on the left
    //   - EDITOR_WIDTH for the inline pill on the right
    //   - 16 px of right-edge breathing room so the pill isn't flush
    //     against the node border
    let label_w = max_label as f64 * GLYPH_WIDTH + 24.0;
    let computed = label_w + EDITOR_WIDTH + 16.0;
    computed.max(NODE_WIDTH)
}

/// Compute layout for a single node. The node's `position` is its
/// top-left in canvas-space. Rows stack from the top under the title
/// bar in this order: output sockets, input sockets, then unbound
/// properties.
pub fn layout_node(node: &NodeView) -> NodeLayoutInfo {
    layout_node_with_connections(node, |_| false)
}

/// Same as [`layout_node`] but the caller tells us which input sockets
/// are currently connected. Bound inline editors on connected inputs
/// are suppressed so the row reads as "data is flowing in here" without
/// the extra editor noise.
pub fn layout_node_with_connections<F>(node: &NodeView, is_input_connected: F) -> NodeLayoutInfo
where
    F: FnMut(&str) -> bool,
{
    layout_node_with_state(node, is_input_connected, /* collapsed */ false)
}

/// Full layout with both connection-aware editor suppression and a
/// per-node collapsed flag. A collapsed node draws as a title-bar-only
/// strip; its sockets all anchor at the title-bar's side-center so
/// existing noodles still have endpoints to resolve against. Property
/// rows and inline editors are dropped entirely while collapsed.
pub fn layout_node_with_state<F>(
    node: &NodeView,
    mut is_input_connected: F,
    collapsed: bool,
) -> NodeLayoutInfo
where
    F: FnMut(&str) -> bool,
{
    let top_left = node.position;
    let node_width = compute_node_width(node);

    if collapsed {
        // Single-row layout: just the title bar. Sockets collapse to
        // the bar's side-center (one point per side) so the noodle
        // bezier endpoints land on the chrome.
        let height = TITLE_HEIGHT;
        let center_y = top_left[1] - TITLE_HEIGHT * 0.5;
        let mut rows: Vec<NodeRow> = Vec::with_capacity(node.outputs.len() + node.inputs.len());
        for s in &node.outputs {
            rows.push(NodeRow::Output(SocketLayout {
                side: SocketSide::Output,
                name: s.name.clone(),
                display_label: s.label().to_string(),
                socket_type: s.socket_type,
                center: [top_left[0] + node_width, center_y],
            }));
        }
        for s in &node.inputs {
            // Suppress the editor regardless of connection state — there's
            // no row to host it on.
            let _ = is_input_connected(&s.name);
            rows.push(NodeRow::Input {
                socket: SocketLayout {
                    side: SocketSide::Input,
                    name: s.name.clone(),
                    display_label: s.label().to_string(),
                    socket_type: s.socket_type,
                    center: [top_left[0], center_y],
                },
                editor: None,
                height: ROW_HEIGHT,
            });
        }
        return NodeLayoutInfo {
            node_id: node.id,
            top_left,
            size: [node_width, height],
            rows,
            display_name: node.display_name.clone(),
            category: node.category.clone(),
            collapsed: true,
            error: node.error.clone(),
        };
    }

    // Row ordering: the node's `properties()` declaration order is the
    // source of truth for body row order. Inputs sockets without a
    // matching property (e.g. Extrude's `Paths` geometry input) stack
    // at the top of the body — the conventional spot for inputs that
    // don't carry an inline fallback. Everything else flows in the
    // order the schema declared it, so a node like Cylinder can place
    // its `advanced` toggle right above the rows that toggle gates.
    let input_socket_by_name: std::collections::HashMap<&str, &crate::model::SocketView> =
        node.inputs.iter().map(|s| (s.name.as_str(), s)).collect();
    let bound_input_names: std::collections::HashSet<&str> = node
        .properties
        .iter()
        .filter_map(|p| {
            p.bound_input
                .as_deref()
                .filter(|name| input_socket_by_name.contains_key(name))
        })
        .collect();
    let bare_inputs: Vec<&crate::model::SocketView> = node
        .inputs
        .iter()
        .filter(|s| !bound_input_names.contains(s.name.as_str()))
        .collect();

    let output_rows = node.outputs.len();
    let bare_input_rows = bare_inputs.len();
    let prop_rows = node.properties.len();

    let mut rows: Vec<NodeRow> = Vec::with_capacity(output_rows + bare_input_rows + prop_rows);
    // Cumulative offset from the body's top edge in screen-space-equivalent
    // canvas units. Rows can claim more than one `ROW_HEIGHT` (e.g. a
    // read-only hint message wraps to multiple lines) so the layout
    // tracks an explicit Y instead of multiplying a row index.
    let mut y_offset = 0.0_f64;

    // Outputs first — fixed row height.
    for s in &node.outputs {
        let center_y = top_left[1] - TITLE_HEIGHT - y_offset - ROW_HEIGHT * 0.5;
        rows.push(NodeRow::Output(SocketLayout {
            side: SocketSide::Output,
            name: s.name.clone(),
            display_label: s.label().to_string(),
            socket_type: s.socket_type,
            center: [top_left[0] + node_width, center_y],
        }));
        y_offset += rows[rows.len() - 1].height();
    }

    // Bare input sockets (no matching property) — render socket + label,
    // no inline editor. Position retained from the input list order.
    for s in bare_inputs {
        let center_y = top_left[1] - TITLE_HEIGHT - y_offset - ROW_HEIGHT * 0.5;
        rows.push(NodeRow::Input {
            socket: SocketLayout {
                side: SocketSide::Input,
                name: s.name.clone(),
                display_label: s.label().to_string(),
                socket_type: s.socket_type,
                center: [top_left[0], center_y],
            },
            editor: None,
            height: ROW_HEIGHT,
        });
        y_offset += rows[rows.len() - 1].height();
    }

    // Properties in declaration order. Each one becomes either:
    //   - an Input row (with bound editor) when its `bound_input`
    //     matches a real input socket — gives the row the colored
    //     socket dot + socket-side label.
    //   - a Property row when no socket matches — full-width row with
    //     label + editor owned by the renderer.
    //
    // Per-row height comes from `row_height_for_property` so multi-line
    // read-only strings get a taller row instead of clipping into the
    // next one.
    for p in &node.properties {
        let row_h = row_height_for_property(p);
        let bound_socket = p
            .bound_input
            .as_deref()
            .and_then(|name| input_socket_by_name.get(name).copied());
        if let Some(s) = bound_socket {
            let center_y = top_left[1] - TITLE_HEIGHT - y_offset - ROW_HEIGHT * 0.5;
            let socket = SocketLayout {
                side: SocketSide::Input,
                name: s.name.clone(),
                display_label: s.label().to_string(),
                socket_type: s.socket_type,
                center: [top_left[0], center_y],
            };
            // Hide the inline editor when the socket is connected — the
            // upstream value wins. Static layout reserves the slot
            // either way; we just drop the hit-rect.
            let editor = if is_input_connected(&s.name) {
                None
            } else {
                Some(input_editor_layout_y(top_left, y_offset, p, node_width))
            };
            rows.push(NodeRow::Input {
                socket,
                editor,
                height: row_h,
            });
        } else {
            let row_top_y = top_left[1] - TITLE_HEIGHT - y_offset;
            rows.push(NodeRow::Property(PropLayout {
                name: p.name.clone(),
                display_label: p.display_label.clone(),
                min: p.min,
                max: p.max,
                current: p.current.clone(),
                editor: p.editor,
                editor_kind: p.editor_kind.clone(),
                top_left: [top_left[0] + 1.0, row_top_y],
                size: [node_width - 2.0, row_h],
                full_row: true,
            }));
        }
        // Advance by the row's own height, read back off the row that
        // was just pushed — the same accessor the widget builder uses,
        // so the two cannot describe different stacks.
        y_offset += rows[rows.len() - 1].height();
    }

    let height = TITLE_HEIGHT + y_offset + NODE_BOTTOM_PAD;

    NodeLayoutInfo {
        node_id: node.id,
        top_left,
        size: [node_width, height],
        rows,
        display_name: node.display_name.clone(),
        category: node.category.clone(),
        collapsed: false,
        error: node.error.clone(),
    }
}

/// Pick the row height a given property wants — multi-line read-only
/// strings (MatterCAD's `[ReadOnly] string` rows) get two rows of
/// vertical real estate so a wrapped hint message fits without
/// clipping the second line.
fn row_height_for_property(p: &PropertyView) -> f64 {
    use agg_gui::widgets::EditorKind;
    let is_read_only = matches!(&p.editor_kind, Some(EditorKind::StringReadOnly));
    if is_read_only {
        // Two rows worth — enough headroom for the typical
        // ~60-character hint message at the auto-sized node width.
        // The renderer's word-wrap fills the available height.
        ROW_HEIGHT * 2.0
    } else {
        ROW_HEIGHT
    }
}

/// Position the inline editor pill on a bound-input row at the given
/// cumulative Y offset from the body top. Replaces the older
/// `input_editor_layout` which assumed every row was `ROW_HEIGHT`.
fn input_editor_layout_y(
    top_left: [f64; 2],
    y_offset: f64,
    p: &PropertyView,
    node_width: f64,
) -> PropLayout {
    let row_top_y = top_left[1] - TITLE_HEIGHT - y_offset;
    let editor_x = top_left[0] + node_width - EDITOR_WIDTH - SOCKET_RADIUS;
    PropLayout {
        name: p.name.clone(),
        display_label: p.display_label.clone(),
        min: p.min,
        max: p.max,
        current: p.current.clone(),
        editor: p.editor,
        editor_kind: p.editor_kind.clone(),
        top_left: [editor_x, row_top_y],
        size: [EDITOR_WIDTH, ROW_HEIGHT],
        full_row: false,
    }
}

// --- Drawing ---------------------------------------------------------------

pub use crate::palette::CanvasPalette;

/// Draw an infinite grid backdrop. `cell_size` is in canvas units.
pub fn draw_canvas_grid(
    ctx: &mut dyn DrawCtx,
    visible: ([f64; 2], [f64; 2]),
    cell_size: f64,
    color: Color,
) {
    let (mn, mx) = visible;
    if mn[0] >= mx[0] || mn[1] >= mx[1] || cell_size <= 0.0 {
        return;
    }
    ctx.set_stroke_color(color);
    ctx.set_line_width(1.0);
    let x0 = (mn[0] / cell_size).floor() * cell_size;
    let mut x = x0;
    while x <= mx[0] {
        ctx.begin_path();
        ctx.move_to(x, mn[1]);
        ctx.line_to(x, mx[1]);
        ctx.stroke();
        x += cell_size;
    }
    let y0 = (mn[1] / cell_size).floor() * cell_size;
    let mut y = y0;
    while y <= mx[1] {
        ctx.begin_path();
        ctx.move_to(mn[0], y);
        ctx.line_to(mx[0], y);
        ctx.stroke();
        y += cell_size;
    }
}

/// Draw a cubic-bezier connection between two canvas-space socket centers.
/// Control-point offsets follow NodeDesigner's `render-noodle.js`
/// SPLINE_NOODLE formula: each tangent is horizontal (outputs exit
/// right, inputs enter left) with length = 25% of the Euclidean
/// distance between the two endpoints. The distance-proportional
/// offset keeps short noodles tight and long noodles gracefully
/// curved, matching the look users are familiar with.
pub fn draw_bezier_connection(
    ctx: &mut dyn DrawCtx,
    from: [f64; 2],
    to: [f64; 2],
    color: Color,
    line_width: f64,
) {
    let dx_raw = to[0] - from[0];
    let dy_raw = to[1] - from[1];
    let dist = (dx_raw * dx_raw + dy_raw * dy_raw).sqrt();
    let off = dist * 0.25;
    let cp1 = [from[0] + off, from[1]];
    let cp2 = [to[0] - off, to[1]];
    ctx.set_stroke_color(color);
    ctx.set_line_width(line_width);
    ctx.begin_path();
    ctx.move_to(from[0], from[1]);
    ctx.cubic_to(cp1[0], cp1[1], cp2[0], cp2[1], to[0], to[1]);
    ctx.stroke();
}
