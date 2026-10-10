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
#[derive(Default)]
pub(crate) struct PresentationState {
    pub noodle_style: NoodleStyle,
    pub socket_hover: bool,
    /// The hovered socket, `(node, side, name)`; tracked only with
    /// `socket_hover` on and the canvas idle.
    pub hovered: Option<(NodeId, SocketSide, String)>,
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
        if !self.presentation.socket_hover || !matches!(self.interaction, CanvasState::Idle) {
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
