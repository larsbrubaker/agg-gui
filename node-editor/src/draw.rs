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

use crate::model::{NodeGraphModel, NodeId, NodeView, PropertyValue, PropertyView, SocketTypeId};

// --- Layout constants ------------------------------------------------------

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
const ROW_PADDING_X: f64 = 6.0;
/// Padding from the row edge to the label baseline.
const LABEL_OFFSET_Y: f64 = 14.0;

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
    },
    /// Standalone property row — no socket attachment, just an inline
    /// editor that spans most of the row.
    Property(PropLayout),
}

impl NodeRow {
    /// Canvas-space row rectangle (top edge y, height `ROW_HEIGHT`).
    pub fn row_rect(&self) -> ([f64; 2], [f64; 2]) {
        match self {
            NodeRow::Output(s) | NodeRow::Input { socket: s, .. } => {
                // We don't track the row's own rect explicitly on the socket
                // (its `center` is in the middle of the row), so reconstruct
                // it from the socket center.
                let top = s.center[1] + ROW_HEIGHT * 0.5;
                ([0.0, top], [NODE_WIDTH, ROW_HEIGHT])
            }
            NodeRow::Property(p) => (p.top_left, p.size),
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
                center: [top_left[0] + NODE_WIDTH, center_y],
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
            });
        }
        return NodeLayoutInfo {
            node_id: node.id,
            top_left,
            size: [NODE_WIDTH, height],
            rows,
            display_name: node.display_name.clone(),
            category: node.category.clone(),
            collapsed: true,
        };
    }

    // Partition properties by bound input. A property whose
    // `bound_input` names an input socket that doesn't exist on this
    // node falls back to being treated as unbound — the property still
    // renders as its own row instead of vanishing. This handles two
    // common cases gracefully:
    //   1. Cached instances minted under an older schema that didn't
    //      yet have the matching sockets.
    //   2. Schema declares a default `bound_input` but the host node
    //      type opted out of adding the socket.
    let input_names: std::collections::HashSet<&str> =
        node.inputs.iter().map(|s| s.name.as_str()).collect();
    let bound_properties: std::collections::HashMap<&str, &PropertyView> = node
        .properties
        .iter()
        .filter_map(|p| {
            p.bound_input
                .as_deref()
                .filter(|name| input_names.contains(name))
                .map(|s| (s, p))
        })
        .collect();
    let unbound_props: Vec<&PropertyView> = node
        .properties
        .iter()
        .filter(|p| match p.bound_input.as_deref() {
            None => true,
            Some(name) => !input_names.contains(name),
        })
        .collect();

    let output_rows = node.outputs.len();
    let input_rows = node.inputs.len();
    let prop_rows = unbound_props.len();
    let total_rows = (output_rows + input_rows + prop_rows) as f64;
    let height = TITLE_HEIGHT + total_rows * ROW_HEIGHT + NODE_BOTTOM_PAD;

    let mut rows: Vec<NodeRow> = Vec::with_capacity(output_rows + input_rows + prop_rows);
    let mut row_index = 0.0;

    // Outputs first.
    for s in &node.outputs {
        let center_y = top_left[1] - TITLE_HEIGHT - (row_index + 0.5) * ROW_HEIGHT;
        rows.push(NodeRow::Output(SocketLayout {
            side: SocketSide::Output,
            name: s.name.clone(),
            display_label: s.label().to_string(),
            socket_type: s.socket_type,
            center: [top_left[0] + NODE_WIDTH, center_y],
        }));
        row_index += 1.0;
    }

    // Inputs next, with optional bound editors.
    for s in &node.inputs {
        let center_y = top_left[1] - TITLE_HEIGHT - (row_index + 0.5) * ROW_HEIGHT;
        let socket = SocketLayout {
            side: SocketSide::Input,
            name: s.name.clone(),
            display_label: s.label().to_string(),
            socket_type: s.socket_type,
            center: [top_left[0], center_y],
        };
        let editor = bound_properties.get(s.name.as_str()).and_then(|p| {
            // Hide the inline editor when the socket is connected — the
            // upstream value wins. Static layout reserves the slot
            // either way; we just drop the hit-rect.
            if is_input_connected(&s.name) {
                None
            } else {
                Some(input_editor_layout(top_left, row_index, p))
            }
        });
        rows.push(NodeRow::Input { socket, editor });
        row_index += 1.0;
    }

    // Unbound properties last.
    for p in unbound_props {
        let row_top_y = top_left[1] - TITLE_HEIGHT - row_index * ROW_HEIGHT;
        rows.push(NodeRow::Property(PropLayout {
            name: p.name.clone(),
            display_label: p.display_label.clone(),
            min: p.min,
            max: p.max,
            current: p.current.clone(),
            editor: p.editor,
            editor_kind: p.editor_kind.clone(),
            top_left: [top_left[0] + 1.0, row_top_y],
            size: [NODE_WIDTH - 2.0, ROW_HEIGHT],
        }));
        row_index += 1.0;
    }

    NodeLayoutInfo {
        node_id: node.id,
        top_left,
        size: [NODE_WIDTH, height],
        rows,
        display_name: node.display_name.clone(),
        category: node.category.clone(),
        collapsed: false,
    }
}

fn input_editor_layout(top_left: [f64; 2], row_index: f64, p: &PropertyView) -> PropLayout {
    let row_top_y = top_left[1] - TITLE_HEIGHT - row_index * ROW_HEIGHT;
    let editor_x = top_left[0] + NODE_WIDTH - EDITOR_WIDTH - SOCKET_RADIUS;
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
    }
}

// --- Drawing ---------------------------------------------------------------

/// Theme palette used by the canvas. Built from agg-gui's current visuals
/// so light / dark mode toggles flow through automatically. Hosts that
/// want different colours can construct one of these manually and pass
/// it via [`crate::NodeEditor::set_palette`].
pub struct CanvasPalette {
    pub canvas_bg: Color,
    pub canvas_grid: Color,
    pub node_body: Color,
    pub node_body_selected: Color,
    pub node_border: Color,
    /// Border colour for the currently-selected node — pulled from
    /// `Visuals::accent` so the View → Color swatch shows up clearly
    /// in the editor.
    pub node_border_selected: Color,
    pub node_title_fallback: Color,
    pub label_text: Color,
}

impl CanvasPalette {
    /// Build the palette from agg-gui's current visuals — adapts to
    /// light or dark mode automatically.
    pub fn from_visuals(v: &agg_gui::theme::Visuals) -> Self {
        let dark = 0.299 * v.bg_color.r + 0.587 * v.bg_color.g + 0.114 * v.bg_color.b < 0.5;
        let canvas_bg = if dark {
            Color::rgb(0.13, 0.14, 0.16)
        } else {
            Color::rgb(0.96, 0.96, 0.97)
        };
        let grid_alpha = if dark { 0.06 } else { 0.30 };
        let canvas_grid = if dark {
            Color::rgba(1.0, 1.0, 1.0, grid_alpha)
        } else {
            Color::rgba(0.0, 0.0, 0.0, grid_alpha * 0.3)
        };
        let node_body = if dark {
            Color::rgb(0.22, 0.23, 0.27)
        } else {
            Color::rgb(0.99, 0.99, 0.99)
        };
        let node_body_selected = if dark {
            Color::rgb(0.28, 0.32, 0.38)
        } else {
            Color::rgb(0.92, 0.94, 1.0)
        };
        let node_border = if dark {
            Color::rgba(0.0, 0.0, 0.0, 0.5)
        } else {
            Color::rgba(0.0, 0.0, 0.0, 0.18)
        };
        Self {
            canvas_bg,
            canvas_grid,
            node_body,
            node_body_selected,
            node_border,
            node_border_selected: v.accent,
            node_title_fallback: v.accent,
            label_text: v.text_color,
        }
    }

    /// Backwards-compat shim used by simple call sites.
    pub fn dark() -> Self {
        Self::from_visuals(&agg_gui::theme::Visuals::dark())
    }
}

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

/// Render one node into the canvas (caller has already applied pan/zoom).
///
/// `model` is consulted for socket + category colours so the host's
/// palette decisions flow through.
pub fn draw_node<M: NodeGraphModel + ?Sized>(
    ctx: &mut dyn DrawCtx,
    layout: &NodeLayoutInfo,
    selected: bool,
    palette: &CanvasPalette,
    model: &M,
) {
    draw_node_chrome(ctx, layout, selected, palette, model);
    for row in &layout.rows {
        draw_row(ctx, layout, row, palette, model);
    }
}

fn draw_node_chrome<M: NodeGraphModel + ?Sized>(
    ctx: &mut dyn DrawCtx,
    layout: &NodeLayoutInfo,
    selected: bool,
    palette: &CanvasPalette,
    model: &M,
) {
    let x = layout.top_left[0];
    let y_top = layout.top_left[1];
    let w = layout.size[0];
    let h = layout.size[1];
    let y_bot = y_top - h;
    let title_color = model.category_color(&layout.category, palette.node_title_fallback);

    ctx.set_fill_color(if selected {
        palette.node_body_selected
    } else {
        palette.node_body
    });
    ctx.begin_path();
    ctx.rounded_rect(x, y_bot, w, h, NODE_RADIUS);
    ctx.fill();

    ctx.set_fill_color(title_color);
    ctx.begin_path();
    ctx.rounded_rect(x, y_top - TITLE_HEIGHT, w, TITLE_HEIGHT, NODE_RADIUS);
    ctx.fill();
    ctx.set_fill_color(if selected {
        palette.node_body_selected
    } else {
        palette.node_body
    });
    ctx.begin_path();
    ctx.rect(x, y_top - TITLE_HEIGHT, w, NODE_RADIUS);
    ctx.fill();
    ctx.set_fill_color(title_color);
    ctx.begin_path();
    ctx.rect(
        x,
        y_top - TITLE_HEIGHT + NODE_RADIUS,
        w,
        TITLE_HEIGHT - NODE_RADIUS,
    );
    ctx.fill();

    ctx.set_stroke_color(palette.node_border);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(x, y_bot, w, h, NODE_RADIUS);
    ctx.stroke();

    ctx.set_fill_color(palette.label_text);
    ctx.set_font_size(13.0);
    let title_y = y_top - TITLE_HEIGHT * 0.5 - 4.0;
    ctx.fill_text(&layout.display_name, x + 10.0, title_y);
}

fn draw_row<M: NodeGraphModel + ?Sized>(
    ctx: &mut dyn DrawCtx,
    layout: &NodeLayoutInfo,
    row: &NodeRow,
    palette: &CanvasPalette,
    model: &M,
) {
    let x = layout.top_left[0];
    let w = layout.size[0];
    match row {
        NodeRow::Output(socket) => {
            draw_socket(ctx, socket, palette, model);
            // Right-align label so it hugs the dot.
            let label_y = socket.center[1] - 4.0;
            ctx.set_fill_color(palette.label_text);
            ctx.set_font_size(11.0);
            let est_width = (socket.display_label.len() as f64) * 6.5;
            ctx.fill_text(
                &socket.display_label,
                x + w - est_width - SOCKET_RADIUS * 2.0 - ROW_PADDING_X,
                label_y,
            );
        }
        NodeRow::Input { socket, editor } => {
            draw_socket(ctx, socket, palette, model);
            let label_y = socket.center[1] - 4.0;
            ctx.set_fill_color(palette.label_text);
            ctx.set_font_size(11.0);
            ctx.fill_text(
                &socket.display_label,
                x + SOCKET_RADIUS * 2.0 + ROW_PADDING_X,
                label_y,
            );
            if let Some(ed) = editor {
                draw_value_editor(ctx, ed, palette);
            }
        }
        NodeRow::Property(prop) => {
            draw_value_editor(ctx, prop, palette);
            // Name on the left of the editor's row.
            ctx.set_fill_color(palette.label_text);
            ctx.set_font_size(11.0);
            ctx.fill_text(
                &prop.name,
                prop.top_left[0] + ROW_PADDING_X,
                prop.top_left[1] - LABEL_OFFSET_Y,
            );
        }
    }
}

fn draw_socket<M: NodeGraphModel + ?Sized>(
    ctx: &mut dyn DrawCtx,
    socket: &SocketLayout,
    palette: &CanvasPalette,
    model: &M,
) {
    let c = model.socket_color(socket.socket_type);
    ctx.set_fill_color(c);
    ctx.begin_path();
    ctx.circle(socket.center[0], socket.center[1], SOCKET_RADIUS);
    ctx.fill();
    ctx.set_stroke_color(palette.node_border);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.circle(socket.center[0], socket.center[1], SOCKET_RADIUS);
    ctx.stroke();
}

fn draw_value_editor(ctx: &mut dyn DrawCtx, prop: &PropLayout, palette: &CanvasPalette) {
    let body_lum =
        0.299 * palette.node_body.r + 0.587 * palette.node_body.g + 0.114 * palette.node_body.b;
    let pill_bg = if body_lum < 0.5 {
        Color::rgba(0.15, 0.16, 0.20, 0.9)
    } else {
        Color::rgba(0.93, 0.93, 0.94, 0.9)
    };
    let pill_x = prop.top_left[0];
    let pill_y_top = prop.top_left[1];
    let pill_w = prop.size[0];
    let pill_h = prop.size[1] - 2.0;
    let pill_y_bot = pill_y_top - pill_h;

    ctx.set_fill_color(pill_bg);
    ctx.begin_path();
    ctx.rounded_rect(pill_x, pill_y_bot, pill_w, pill_h, 3.0);
    ctx.fill();

    // For Color, paint a swatch occupying the right half of the pill.
    if let PropertyValue::Color(c) = &prop.current {
        let swatch_inset = 3.0;
        ctx.set_fill_color(Color::rgba(c[0], c[1], c[2], c[3]));
        ctx.begin_path();
        ctx.rounded_rect(
            pill_x + swatch_inset,
            pill_y_bot + swatch_inset,
            pill_w - 2.0 * swatch_inset,
            pill_h - 2.0 * swatch_inset,
            2.0,
        );
        ctx.fill();
        return;
    }

    let value_str = format_value(&prop.current);
    ctx.set_fill_color(palette.label_text);
    ctx.set_font_size(11.0);
    let est = (value_str.len() as f64) * 6.0;
    let value_x = pill_x + pill_w - est - 6.0;
    ctx.fill_text(&value_str, value_x, pill_y_top - LABEL_OFFSET_Y);
}

fn format_value(v: &PropertyValue) -> String {
    match v {
        PropertyValue::Number(n) => {
            if (n.fract()).abs() < 1e-6 {
                format!("{}", *n as i64)
            } else {
                format!("{:.3}", n)
            }
        }
        PropertyValue::Bool(b) => {
            if *b {
                "true".into()
            } else {
                "false".into()
            }
        }
        PropertyValue::Color(_) => String::new(),
        PropertyValue::Other { display } => display.clone(),
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
