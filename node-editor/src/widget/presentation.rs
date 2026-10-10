//! Socket and noodle presentation state for [`NodeEditor`]: the noodle
//! style, the socket hover ring with its name label, and the pass that
//! stamps each socket layout with its model-chosen shape, the number of
//! noodles landed on a multi-input and its visibility.
//!
//! The drawing itself is [`crate::socket_style`]; this file decides *what*
//! to draw from the model and the editor's interaction state. MatterCAD's
//! reference is `NoodleRenderer` / `NoodleDragController.HoverAt`.

use agg_gui::DrawCtx;

use crate::draw::{NodeLayoutInfo, NodeRow, SocketLayout, SocketSide};
use crate::model::{NodeGraphModel, NodeId, NoodleView};
use crate::socket_style::{
    draw_socket_ring, landing_offset, NoodleStyle, SocketPaint, NODE_DESIGNER_SOCKET_RADIUS,
    RING_WIDTH, SOCKET_OUTLINE_WIDTH,
};

use super::{CanvasState, NodeEditor};

/// The editor's presentation choices and the socket under the pointer.
pub(crate) struct PresentationState {
    pub noodle_style: NoodleStyle,
    pub socket_hover: bool,
    /// The hovered socket, `(node, side, name)`; tracked only with
    /// `socket_hover` on and the canvas idle.
    pub hovered: Option<(NodeId, SocketSide, String)>,
    /// Sockets are drawn (MatterCAD's `NodeEditor.ShowSockets`).
    pub show_sockets: bool,
    /// The grid backdrop is drawn.
    pub canvas_grid: bool,
    /// The hosted cards' drop shadow (`None`: agg-gui's window shadow).
    pub card_shadow: Option<super::CardShadow>,
}

impl Default for PresentationState {
    fn default() -> Self {
        Self {
            noodle_style: NoodleStyle::default(),
            socket_hover: false,
            hovered: None,
            show_sockets: true,
            canvas_grid: true,
            card_shadow: None,
        }
    }
}

impl NodeEditor {
    /// Draw noodles and sockets in `style` ([`NoodleStyle::Simple`] by
    /// default; MatterCAD uses [`NoodleStyle::NodeDesigner`]).
    pub fn with_noodle_style(mut self, style: NoodleStyle) -> Self {
        self.presentation.noodle_style = style;
        self
    }

    /// Ring the socket under the pointer (in the theme's text colour, just
    /// outside its outline) and name it beside it with
    /// [`NodeGraphModel::socket_hover_text`]. Off by default.
    pub fn with_socket_hover(mut self, enabled: bool) -> Self {
        self.presentation.socket_hover = enabled;
        self
    }

    /// Draw the canvas's grid backdrop, or not (on by default; MatterCAD's
    /// node editor draws a plain canvas).
    pub fn with_canvas_grid(mut self, enabled: bool) -> Self {
        self.presentation.canvas_grid = enabled;
        self
    }

    /// Whether the grid backdrop is drawn (see [`Self::with_canvas_grid`]).
    pub fn canvas_grid(&self) -> bool {
        self.presentation.canvas_grid
    }

    /// Paint hosted cards with `shadow` instead of agg-gui's window shadow
    /// (whose 14 unit blur reaches over the start of the noodles beside a
    /// card; MatterCAD's node card blurs 3.5 units, 1.5 down).
    pub fn with_card_shadow(mut self, shadow: super::CardShadow) -> Self {
        self.presentation.card_shadow = Some(shadow);
        self
    }

    /// Draw sockets, or not (MatterCAD's `NodeEditor.ShowSockets`, on by
    /// default; its tests turn sockets off to tell them from what a card
    /// draws under them). Off, no socket, socket ring or dragged noodle is
    /// drawn; noodles between nodes still are, and sockets still hit-test.
    pub fn set_show_sockets(&mut self, show: bool) {
        if self.presentation.show_sockets != show {
            self.presentation.show_sockets = show;
            self.last_paint_fingerprint = None;
            self.backbuffer.invalidate();
            agg_gui::animation::request_draw();
        }
    }

    /// Whether sockets are drawn (see [`Self::set_show_sockets`]).
    pub fn show_sockets(&self) -> bool {
        self.presentation.show_sockets
    }

    /// The socket under `local` (editor-local coordinates at the current
    /// pan and zoom), `(node, side, socket name)`: the same hit area a
    /// press, hover or drop uses. Sockets the host hides take no hit.
    /// Locks the model, so call it without holding the model lock.
    pub fn socket_at(&self, local: agg_gui::Point) -> Option<(NodeId, SocketSide, String)> {
        let layouts = self.snapshot_layouts();
        let canvas = [
            (local.x - self.canvas_offset[0]) / self.canvas_scale,
            (local.y - self.canvas_offset[1]) / self.canvas_scale,
        ];
        self.hit_socket(&layouts, canvas)
            .map(|(node, s)| (node, s.side, s.name))
    }

    /// Half extents, in canvas units, of the socket hit box: MatterCAD's
    /// 6 x 10 design units either side for [`NoodleStyle::NodeDesigner`],
    /// never under 8 device pixels a side however far the graph is zoomed
    /// out (`NoodleDragController.SocketAt`). `None` for
    /// [`NoodleStyle::Simple`], whose hit area is the round
    /// [`crate::draw::SOCKET_HIT_RADIUS`].
    pub(super) fn socket_hit_half(&self) -> Option<[f64; 2]> {
        match self.presentation.noodle_style {
            NoodleStyle::Simple => None,
            NoodleStyle::NodeDesigner => {
                let floor = 8.0 / (agg_gui::device_scale() * self.canvas_scale);
                Some([6f64.max(floor), 10f64.max(floor)])
            }
        }
    }

    /// The noodle style the editor draws with.
    pub fn noodle_style(&self) -> NoodleStyle {
        self.presentation.noodle_style
    }

    /// The hovered socket, `(node, side, socket name)`, when socket hover
    /// is on and the pointer rests on one.
    pub fn hovered_socket(&self) -> Option<(NodeId, SocketSide, &str)> {
        self.presentation
            .hovered
            .as_ref()
            .map(|(n, s, name)| (*n, *s, name.as_str()))
    }

    /// Where `socket` on `node` sits, in editor-local coordinates at the
    /// current pan and zoom: the centre noodles end at and a press must
    /// land on. `None` when the node or socket is not in the model. A
    /// socket hidden by [`NodeGraphModel::socket_visible`] still reports
    /// its laid-out place (wired noodles draw to it). Locks the model, so
    /// call it without holding the model lock.
    pub fn socket_position(
        &self,
        node: NodeId,
        side: SocketSide,
        socket: &str,
    ) -> Option<agg_gui::Point> {
        let layouts = self.snapshot_layouts();
        let s = layouts
            .iter()
            .find(|l| l.node_id == node)?
            .sockets()
            .find(|s| s.side == side && s.name == socket)?;
        Some(agg_gui::Point::new(
            s.center[0] * self.canvas_scale + self.canvas_offset[0],
            s.center[1] * self.canvas_scale + self.canvas_offset[1],
        ))
    }

    /// Track the socket under `canvas_pos` on an idle pointer move.
    /// Repaints only when the hovered socket changes, so plain motion stays
    /// free of repaints.
    pub(super) fn update_socket_hover(&mut self, layouts: &[NodeLayoutInfo], canvas_pos: [f64; 2]) {
        if !self.presentation.socket_hover {
            return;
        }
        let hit = self
            .hit_socket(layouts, canvas_pos)
            .map(|(n, s)| (n, s.side, s.name));
        self.set_hovered_socket(hit);
    }

    pub(super) fn set_hovered_socket(&mut self, hit: Option<(NodeId, SocketSide, String)>) {
        if self.presentation.hovered != hit {
            self.presentation.hovered = hit;
            self.backbuffer.invalidate();
            agg_gui::animation::request_draw();
        }
    }

    /// The socket paint for `s` in the editor's style.
    pub(super) fn socket_paint(&self, s: &SocketLayout, color: agg_gui::Color) -> SocketPaint {
        SocketPaint {
            shape: s.shape,
            color,
            stretch: s.stretch(),
            style: self.presentation.noodle_style,
            border: self.palette.node_border,
        }
    }

    /// Paint the hover ring and label, in canvas space, over the cards.
    /// Called from `finish_paint` while the canvas is idle.
    pub(super) fn paint_socket_hover(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.presentation.socket_hover
            || !self.presentation.show_sockets
            || !matches!(self.interaction, CanvasState::Idle)
        {
            return;
        }
        let Some((node, side, name)) = self.presentation.hovered.clone() else {
            return;
        };
        let layouts = self.snapshot_layouts();
        let Some(socket) = layouts
            .iter()
            .find(|l| l.node_id == node)
            .and_then(|l| l.sockets().find(|s| s.side == side && s.name == name))
            .cloned()
        else {
            return;
        };
        let text = self
            .model
            .lock()
            .unwrap()
            .socket_hover_text(node, side, &name);
        let paint = self.socket_paint(&socket, self.palette.label_text);
        ctx.save();
        ctx.translate(self.canvas_offset[0], self.canvas_offset[1]);
        ctx.scale(self.canvas_scale, self.canvas_scale);
        draw_socket_ring(ctx, socket.center, &paint, self.palette.label_text);
        if let Some(text) = text {
            self.paint_hover_label(ctx, &socket, &text);
        }
        ctx.restore();
    }

    /// The name label: a small box outside the node beside the socket.
    fn paint_hover_label(&self, ctx: &mut dyn DrawCtx, socket: &SocketLayout, text: &str) {
        const FONT: f64 = 11.0;
        const PAD: f64 = 4.0;
        ctx.set_font_size(FONT);
        let text_w = ctx
            .measure_text(text)
            .map(|m| m.width)
            .unwrap_or(text.chars().count() as f64 * 6.5);
        let reach = NODE_DESIGNER_SOCKET_RADIUS + SOCKET_OUTLINE_WIDTH + RING_WIDTH + 4.0;
        let (w, h) = (text_w + 2.0 * PAD, FONT + 2.0 * PAD);
        let x = match socket.side {
            SocketSide::Output => socket.center[0] + reach,
            SocketSide::Input => socket.center[0] - reach - w,
        };
        let y = socket.center[1] - h / 2.0;
        ctx.set_fill_color(self.palette.node_body);
        ctx.begin_path();
        ctx.rounded_rect(x, y, w, h, 3.0);
        ctx.fill();
        ctx.set_stroke_color(self.palette.node_border);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(x, y, w, h, 3.0);
        ctx.stroke();
        ctx.set_fill_color(self.palette.label_text);
        ctx.fill_text(text, x + PAD, y + PAD + 2.0);
    }
}

/// Noodles drawn into input `socket` on `node` when it is a multi-input,
/// zero otherwise.
pub(crate) fn landed_on(
    model: &dyn NodeGraphModel,
    noodles: &[NoodleView],
    node: NodeId,
    socket: &str,
) -> usize {
    if !model.socket_multi_input(node, socket) {
        return 0;
    }
    noodles
        .iter()
        .filter(|n| n.to_node == node && n.to_socket == socket)
        .count()
}

/// A copy of `layouts` with every socket marked hidden, for drawing the
/// node widgets while sockets are off.
pub(crate) fn undrawn_sockets(layouts: &[NodeLayoutInfo]) -> Vec<NodeLayoutInfo> {
    let mut copy = layouts.to_vec();
    for l in &mut copy {
        for row in &mut l.rows {
            if let NodeRow::Output(s) | NodeRow::Input { socket: s, .. } = row {
                s.hidden = true;
            }
        }
    }
    copy
}

/// Stamp every socket layout with its shape, landed-noodle count and
/// whether the host hides it.
pub(crate) fn apply_socket_styles(
    model: &dyn NodeGraphModel,
    noodles: &[NoodleView],
    layouts: &mut [NodeLayoutInfo],
) {
    for l in layouts {
        let node = l.node_id;
        for row in &mut l.rows {
            let s = match row {
                NodeRow::Output(s) | NodeRow::Input { socket: s, .. } => s,
                NodeRow::Property(_) => continue,
            };
            s.shape = model.socket_shape(node, s.side, &s.name, s.socket_type);
            s.hidden = !model.socket_visible(node, s.side, &s.name);
            s.landed = match s.side {
                SocketSide::Input => landed_on(model, noodles, node, &s.name),
                SocketSide::Output => 0,
            };
        }
    }
}

/// Where `noodle` ends on its target socket `to`: the socket's centre, or
/// for a multi-input with several noodles, its own place along the pill
/// in `noodles` order (the first highest).
pub(crate) fn landing_point(
    noodles: &[NoodleView],
    noodle: &NoodleView,
    to: &SocketLayout,
) -> [f64; 2] {
    if to.landed < 2 {
        return to.center;
    }
    let index = noodles
        .iter()
        .filter(|n| n.to_node == noodle.to_node && n.to_socket == noodle.to_socket)
        .position(|n| n.from_node == noodle.from_node && n.from_socket == noodle.from_socket)
        .unwrap_or(0);
    [
        to.center[0],
        to.center[1] + landing_offset(index, to.landed),
    ]
}

/// How far past its centre a socket's drawing can reach horizontally
/// (a field diamond's outlined points), for widening child clips.
pub(crate) fn socket_reach(scale: f64) -> f64 {
    (crate::socket_style::DIAMOND_HALF_SIZE + SOCKET_OUTLINE_WIDTH * 2f64.sqrt()) * scale
}
