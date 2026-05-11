//! Drawing helpers for the node-editor canvas — split from `widget.rs`
//! to respect the 800-line limit.
//!
//! All coordinates are **canvas-space**: positive Y is up (agg-gui
//! convention), and a node's `position` is its top-left corner. The caller
//! has already `save()`d, `translate()`d by `canvas_offset`, and `scale()`d
//! by `canvas_scale` on the `DrawCtx` before invoking these helpers, so we
//! draw straight in canvas units.

use agg_gui::{Color, DrawCtx};

use crate::model::{NodeGraphModel, NodeId, PropertyValue, SocketTypeId, SocketView};

// --- Layout constants ------------------------------------------------------

pub const NODE_WIDTH: f64 = 180.0;
pub const TITLE_HEIGHT: f64 = 26.0;
pub const ROW_HEIGHT: f64 = 22.0;
pub const NODE_BOTTOM_PAD: f64 = 6.0;
pub const SOCKET_RADIUS: f64 = 5.5;
pub const SOCKET_HIT_RADIUS: f64 = 9.0;
pub const NODE_RADIUS: f64 = 6.0;

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
    pub socket_type: SocketTypeId,
    /// Canvas-space center of the socket circle.
    pub center: [f64; 2],
}

/// One editable property row inside a node.
#[derive(Clone, Debug)]
pub struct PropLayout {
    pub name: String,
    /// Numeric range, copied from the model's `PropertyView`. Used to
    /// clamp drag deltas on number drags.
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub current: PropertyValue,
    /// Canvas-space top-left + size of the row — used for hit testing.
    pub top_left: [f64; 2],
    pub size: [f64; 2],
}

impl PropLayout {
    pub fn contains(&self, canvas_pos: [f64; 2]) -> bool {
        let x0 = self.top_left[0];
        let y1 = self.top_left[1];
        let x1 = x0 + self.size[0];
        let y0 = y1 - self.size[1];
        canvas_pos[0] >= x0 && canvas_pos[0] <= x1 && canvas_pos[1] >= y0 && canvas_pos[1] <= y1
    }
}

/// Computed canvas-space layout for one node — its size and socket
/// positions. Recomputed on each paint frame; cheap.
#[derive(Clone, Debug)]
pub struct NodeLayoutInfo {
    pub node_id: NodeId,
    pub top_left: [f64; 2],
    pub size: [f64; 2],
    pub sockets: Vec<SocketLayout>,
    pub props: Vec<PropLayout>,
    pub display_name: String,
    pub category: String,
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

    /// Find a socket whose hit radius contains `canvas_pos`.
    pub fn socket_at(&self, canvas_pos: [f64; 2]) -> Option<&SocketLayout> {
        self.sockets.iter().find(|s| {
            let dx = s.center[0] - canvas_pos[0];
            let dy = s.center[1] - canvas_pos[1];
            dx * dx + dy * dy <= SOCKET_HIT_RADIUS * SOCKET_HIT_RADIUS
        })
    }

    /// Find the property row hit by `canvas_pos`.
    pub fn prop_at(&self, canvas_pos: [f64; 2]) -> Option<&PropLayout> {
        self.props.iter().find(|p| p.contains(canvas_pos))
    }
}

/// Compute layout for a single node. The node's `position` is its
/// top-left in canvas-space.  Sockets stack from the top under the
/// title bar; properties stack below the sockets.
pub fn layout_node(node: &crate::model::NodeView) -> NodeLayoutInfo {
    let socket_rows = node.inputs.len().max(node.outputs.len()) as f64;
    let prop_rows = node.properties.len() as f64;

    let height = TITLE_HEIGHT + socket_rows * ROW_HEIGHT + prop_rows * ROW_HEIGHT + NODE_BOTTOM_PAD;
    let top_left = node.position;

    let mut sockets = Vec::with_capacity(node.inputs.len() + node.outputs.len());
    push_sockets(&mut sockets, &node.inputs, SocketSide::Input, top_left);
    push_sockets(&mut sockets, &node.outputs, SocketSide::Output, top_left);

    let prop_section_top = top_left[1] - TITLE_HEIGHT - socket_rows * ROW_HEIGHT;
    let mut props = Vec::with_capacity(node.properties.len());
    for (i, p) in node.properties.iter().enumerate() {
        let row_top_y = prop_section_top - i as f64 * ROW_HEIGHT;
        props.push(PropLayout {
            name: p.name.clone(),
            min: p.min,
            max: p.max,
            current: p.current.clone(),
            top_left: [top_left[0] + 1.0, row_top_y],
            size: [NODE_WIDTH - 2.0, ROW_HEIGHT],
        });
    }
    NodeLayoutInfo {
        node_id: node.id,
        top_left,
        size: [NODE_WIDTH, height],
        sockets,
        props,
        display_name: node.display_name.clone(),
        category: node.category.clone(),
    }
}

fn push_sockets(
    out: &mut Vec<SocketLayout>,
    sockets: &[SocketView],
    side: SocketSide,
    top_left: [f64; 2],
) {
    let x = match side {
        SocketSide::Input => top_left[0],
        SocketSide::Output => top_left[0] + NODE_WIDTH,
    };
    for (i, s) in sockets.iter().enumerate() {
        let y = top_left[1] - TITLE_HEIGHT - (i as f64 + 0.5) * ROW_HEIGHT;
        out.push(SocketLayout {
            side,
            name: s.name.clone(),
            socket_type: s.socket_type,
            center: [x, y],
        });
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
    let x = layout.top_left[0];
    let y_top = layout.top_left[1];
    let w = layout.size[0];
    let h = layout.size[1];
    let y_bot = y_top - h;
    let title_color = model.category_color(&layout.category, palette.node_title_fallback);

    // Body (rounded rect) — agg-gui rect uses bottom-left origin in Y-up,
    // so we pass (x, y_bot, w, h).
    ctx.set_fill_color(if selected {
        palette.node_body_selected
    } else {
        palette.node_body
    });
    ctx.begin_path();
    ctx.rounded_rect(x, y_bot, w, h, NODE_RADIUS);
    ctx.fill();

    // Title bar (filled rectangle at the top, taking up TITLE_HEIGHT).
    ctx.set_fill_color(title_color);
    ctx.begin_path();
    ctx.rounded_rect(x, y_top - TITLE_HEIGHT, w, TITLE_HEIGHT, NODE_RADIUS);
    ctx.fill();
    // Cover the bottom corners of the title bar so it only rounds at the top.
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

    // Border around the whole node.
    ctx.set_stroke_color(palette.node_border);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(x, y_bot, w, h, NODE_RADIUS);
    ctx.stroke();

    // Title text — centered vertically in the title bar.
    ctx.set_fill_color(palette.label_text);
    ctx.set_font_size(13.0);
    let title_y = y_top - TITLE_HEIGHT * 0.5 - 4.0;
    ctx.fill_text(&layout.display_name, x + 10.0, title_y);

    // Property rows — drawn before sockets so socket labels paint on top.
    let body_lum =
        0.299 * palette.node_body.r + 0.587 * palette.node_body.g + 0.114 * palette.node_body.b;
    let prop_bg = if body_lum < 0.5 {
        Color::rgba(0.15, 0.16, 0.20, 0.9)
    } else {
        Color::rgba(0.93, 0.93, 0.94, 0.9)
    };
    for p in &layout.props {
        ctx.set_fill_color(prop_bg);
        ctx.begin_path();
        ctx.rect(
            p.top_left[0],
            p.top_left[1] - p.size[1],
            p.size[0],
            p.size[1] - 2.0,
        );
        ctx.fill();

        // Name on the left.
        ctx.set_fill_color(palette.label_text);
        ctx.set_font_size(11.0);
        ctx.fill_text(&p.name, p.top_left[0] + 6.0, p.top_left[1] - 14.0);

        // Value on the right (rough right-align by string length estimate).
        let value_str = format_value(&p.current);
        let est = (value_str.len() as f64) * 6.0;
        ctx.fill_text(
            &value_str,
            p.top_left[0] + p.size[0] - est - 6.0,
            p.top_left[1] - 14.0,
        );
    }

    // Socket circles + labels.
    for s in &layout.sockets {
        let c = model.socket_color(s.socket_type);
        ctx.set_fill_color(c);
        ctx.begin_path();
        ctx.circle(s.center[0], s.center[1], SOCKET_RADIUS);
        ctx.fill();
        ctx.set_stroke_color(palette.node_border);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.circle(s.center[0], s.center[1], SOCKET_RADIUS);
        ctx.stroke();

        ctx.set_fill_color(palette.label_text);
        ctx.set_font_size(11.0);
        let label_y = s.center[1] - 4.0;
        match s.side {
            SocketSide::Input => {
                ctx.fill_text(&s.name, x + SOCKET_RADIUS * 2.0 + 4.0, label_y);
            }
            SocketSide::Output => {
                let est_width = (s.name.len() as f64) * 6.5;
                ctx.fill_text(
                    &s.name,
                    x + w - est_width - SOCKET_RADIUS * 2.0 - 4.0,
                    label_y,
                );
            }
        }
    }
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
        PropertyValue::Other { display } => display.clone(),
    }
}

/// Draw a cubic-bezier connection between two canvas-space socket centers.
/// The control points offset horizontally so the curve always exits to the
/// right of an output and enters from the left of an input.
pub fn draw_bezier_connection(
    ctx: &mut dyn DrawCtx,
    from: [f64; 2],
    to: [f64; 2],
    color: Color,
    line_width: f64,
) {
    let dx = (to[0] - from[0]).abs().max(60.0).min(220.0);
    let cp1 = [from[0] + dx, from[1]];
    let cp2 = [to[0] - dx, to[1]];
    ctx.set_stroke_color(color);
    ctx.set_line_width(line_width);
    ctx.begin_path();
    ctx.move_to(from[0], from[1]);
    ctx.cubic_to(cp1[0], cp1[1], cp2[0], cp2[1], to[0], to[1]);
    ctx.stroke();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NodeView, PropertyView, SocketView};

    fn make_node() -> NodeView {
        NodeView {
            id: NodeId(1),
            type_id: "Test".into(),
            display_name: "Test".into(),
            category: "Test".into(),
            position: [100.0, 200.0],
            inputs: vec![SocketView {
                name: "a".into(),
                socket_type: SocketTypeId(7),
                display_label: None,
            }],
            outputs: vec![SocketView {
                name: "out".into(),
                socket_type: SocketTypeId(7),
                display_label: None,
            }],
            properties: vec![PropertyView {
                name: "v".into(),
                current: PropertyValue::Number(1.0),
                min: Some(0.0),
                max: Some(10.0),
            }],
        }
    }

    #[test]
    fn layout_places_input_left_output_right() {
        let info = layout_node(&make_node());
        assert_eq!(info.top_left, [100.0, 200.0]);
        assert_eq!(info.sockets.len(), 2);
        let input = info
            .sockets
            .iter()
            .find(|s| s.side == SocketSide::Input)
            .unwrap();
        let output = info
            .sockets
            .iter()
            .find(|s| s.side == SocketSide::Output)
            .unwrap();
        assert!((input.center[0] - 100.0).abs() < 1e-9);
        assert!((output.center[0] - (100.0 + NODE_WIDTH)).abs() < 1e-9);
        let expected_y = 200.0 - TITLE_HEIGHT - 0.5 * ROW_HEIGHT;
        assert!((input.center[1] - expected_y).abs() < 1e-9);
    }

    #[test]
    fn body_and_header_contains() {
        let mut n = make_node();
        n.position = [0.0, 0.0];
        let info = layout_node(&n);
        assert!(info.body_contains([10.0, -10.0]));
        assert!(!info.body_contains([10.0, 10.0]));
        assert!(info.header_contains([10.0, -5.0]));
        assert!(!info.header_contains([10.0, -TITLE_HEIGHT - 5.0]));
    }

    #[test]
    fn socket_hit_test() {
        let mut n = make_node();
        n.position = [0.0, 0.0];
        let info = layout_node(&n);
        let in_center = info.sockets[0].center;
        assert!(info.socket_at(in_center).is_some());
        assert!(info
            .socket_at([in_center[0] + 5.0, in_center[1] + 5.0])
            .is_some());
        assert!(info
            .socket_at([in_center[0] + 50.0, in_center[1]])
            .is_none());
    }
}
