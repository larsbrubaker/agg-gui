//! Noodle drags on [`NodeEditor`]: starting one (from an output, backwards
//! from an input, or by picking a connected input's noodle up), asking the
//! model which socket under the pointer accepts it, the refusal note, and
//! the drop — on a socket, on a card body (the model's auto-pick), or on
//! empty canvas.
//!
//! The rules themselves are the model's ([`NodeGraphModel::can_connect`],
//! [`NodeGraphModel::auto_pick_socket`], [`NodeGraphModel::move_noodle`]);
//! the public helpers are in [`crate::connection`]. MatterCAD's reference
//! is `NoodleDragController`.

use agg_gui::{DrawCtx, EventResult, Point};

use crate::connection::{refusal_note_position, SocketRef};
use crate::draw::{NodeLayoutInfo, SocketLayout, SocketSide};
use crate::model::{NodeGraphModel, NodeId, NoodleView};

use super::{presentation, CanvasState, NodeEditor};

/// Font size and padding of the refusal note, and its gap from the pointer.
const NOTE_FONT: f64 = 12.0;
const NOTE_PAD: f64 = 3.0;
const NOTE_GAP: f64 = 12.0;

/// The [`SocketRef`] of `socket` on `node`.
pub(super) fn socket_ref(node: NodeId, socket: &SocketLayout) -> SocketRef {
    SocketRef {
        node,
        side: socket.side,
        socket: socket.name.clone(),
        socket_type: socket.socket_type,
    }
}

/// Whether a drag from `fixed` may drop on `to`: the picked-up noodle's own
/// place always may (a multi-input would otherwise refuse it as already
/// there); everything else is the model's [`NodeGraphModel::can_connect`].
pub(super) fn accepts(
    model: &dyn NodeGraphModel,
    fixed: &SocketRef,
    picked_up: Option<&NoodleView>,
    to: &SocketRef,
) -> Result<(), String> {
    if let (Some(picked), Some(ends)) = (picked_up, fixed.noodle_to(to)) {
        if same_noodle(picked, &ends) {
            return Ok(());
        }
    }
    model.can_connect(fixed, to)
}

fn same_noodle(a: &NoodleView, b: &NoodleView) -> bool {
    a.from_node == b.from_node
        && a.from_socket == b.from_socket
        && a.to_node == b.to_node
        && a.to_socket == b.to_socket
}

/// The accepting, shown socket nearest `cursor` within the drop-snap radius: the
/// one the in-flight noodle snaps to and rings.
pub(super) fn find_target_near<'a>(
    layouts: &'a [NodeLayoutInfo],
    model: &dyn NodeGraphModel,
    cursor: [f64; 2],
    fixed: &SocketRef,
    picked_up: Option<&NoodleView>,
) -> Option<&'a SocketLayout> {
    let snap_r = crate::draw::SOCKET_HIT_RADIUS * 1.6;
    let mut best: Option<(&SocketLayout, f64)> = None;
    for l in layouts {
        // A socket the host hides takes no drop.
        for s in l.sockets().filter(|s| !s.hidden) {
            let dx = s.center[0] - cursor[0];
            let dy = s.center[1] - cursor[1];
            let d2 = dx * dx + dy * dy;
            if d2 > snap_r * snap_r || best.is_some_and(|(_, b)| d2 >= b) {
                continue;
            }
            let to = socket_ref(l.node_id, s);
            if to != *fixed && accepts(model, fixed, picked_up, &to).is_ok() {
                best = Some((s, d2));
            }
        }
    }
    best.map(|(s, _)| s)
}

impl NodeEditor {
    /// Keep a noodle picked up off its input in the model until the drop,
    /// as MatterCAD's NodeDesigner does, and hand the whole move to
    /// [`NodeGraphModel::move_noodle`] once: a drop on another socket or
    /// card moves it, a drop on empty canvas deletes it, and a drop back
    /// in place or where nothing accepts it changes nothing. Off by
    /// default: the press removes the noodle at once and the drop adds
    /// whatever it lands on.
    pub fn with_deferred_noodle_pickup(mut self, deferred: bool) -> Self {
        self.deferred_pickup = deferred;
        self
    }

    /// Why the socket under the pointer refuses the noodle being dragged,
    /// as the note beside the pointer says; `None` with no drag, over a
    /// socket that accepts, or over a silent refusal.
    pub fn noodle_refusal(&self) -> Option<&str> {
        match &self.interaction {
            CanvasState::DrawingConnection { refusal, .. } => refusal.as_deref(),
            _ => None,
        }
    }

    /// The noodle being moved by a deferred pick-up, which paint leaves
    /// out of its place.
    pub(super) fn picked_up_noodle(&self) -> Option<&NoodleView> {
        match &self.interaction {
            CanvasState::DrawingConnection { picked_up, .. } => picked_up.as_ref(),
            _ => None,
        }
    }

    /// A press on `socket` of `node`: start a noodle drag. A connected
    /// input hands over its noodle (a multi-input the one landing nearest
    /// the press), dragged from its source; an empty input drags backwards.
    pub(super) fn begin_connection(
        &mut self,
        layouts: &[NodeLayoutInfo],
        node: NodeId,
        socket: &SocketLayout,
        canvas_pos: [f64; 2],
        local: Point,
    ) -> EventResult {
        let mut fixed = (node, socket.clone());
        let mut picked_up = None;
        if socket.side == SocketSide::Input {
            // Bound first: a guard in the `if let` would be held through
            // the body, which locks the model again.
            let nearest =
                nearest_noodle_into(&*self.model.lock().unwrap(), node, socket, canvas_pos);
            if let Some(noodle) = nearest {
                let source = layouts
                    .iter()
                    .find(|l| l.node_id == noodle.from_node)
                    .and_then(|l| {
                        l.sockets()
                            .find(|s| s.side == SocketSide::Output && s.name == noodle.from_socket)
                    })
                    .cloned();
                if !self.deferred_pickup {
                    self.model.lock().unwrap().remove_noodle(
                        noodle.from_node,
                        &noodle.from_socket,
                        noodle.to_node,
                        &noodle.to_socket,
                    );
                }
                if let Some(src) = source {
                    fixed = (noodle.from_node, src);
                    picked_up = self.deferred_pickup.then_some(noodle);
                    self.backbuffer.invalidate();
                    agg_gui::animation::request_draw();
                }
            }
        }
        let (from_node, from) = fixed;
        self.interaction = CanvasState::DrawingConnection {
            from_node,
            from_socket: from.name.clone(),
            from_canvas: from.center,
            cursor_canvas: canvas_pos,
            from_socket_type: from.socket_type,
            from_side: from.side,
            picked_up,
            refusal: None,
            cursor_local: local,
        };
        EventResult::Consumed
    }

    /// A pointer move during a noodle drag: follow the pointer and say why
    /// the socket under it refuses, if it does.
    pub(super) fn update_connection_drag(
        &mut self,
        layouts: &[NodeLayoutInfo],
        local: Point,
        canvas_pos: [f64; 2],
    ) {
        let hit = self.hit_socket(layouts, canvas_pos);
        let model = self.model.lock().unwrap();
        let CanvasState::DrawingConnection {
            from_node,
            from_socket,
            from_socket_type,
            from_side,
            picked_up,
            refusal,
            cursor_canvas,
            cursor_local,
            ..
        } = &mut self.interaction
        else {
            return;
        };
        *cursor_canvas = canvas_pos;
        *cursor_local = local;
        let fixed = SocketRef {
            node: *from_node,
            side: *from_side,
            socket: from_socket.clone(),
            socket_type: *from_socket_type,
        };
        let now = hit.and_then(|(node, s)| {
            let to = socket_ref(node, &s);
            if to == fixed {
                return None;
            }
            accepts(&*model, &fixed, picked_up.as_ref(), &to)
                .err()
                .filter(|r| !r.is_empty())
        });
        if *refusal != now {
            *refusal = now;
            self.backbuffer.invalidate();
        }
    }

    /// The drop of a noodle drag at `canvas_pos`. `state` is the
    /// `DrawingConnection` the drag ended in.
    pub(super) fn finish_connection(&mut self, canvas_pos: [f64; 2], state: CanvasState) {
        let CanvasState::DrawingConnection {
            from_node,
            from_socket,
            from_socket_type,
            from_side,
            picked_up,
            ..
        } = state
        else {
            return;
        };
        let fixed = SocketRef {
            node: from_node,
            side: from_side,
            socket: from_socket,
            socket_type: from_socket_type,
        };
        let layouts = self.snapshot_layouts();
        let ends = match self.drop_ends(&layouts, canvas_pos, &fixed, picked_up.as_ref()) {
            DropEnds::Refused => return self.connection_ended(),
            DropEnds::Empty => None,
            DropEnds::Connect(ends) => Some(ends),
        };
        let mut model = self.model.lock().unwrap();
        match (picked_up, ends) {
            (Some(picked), Some(ends)) if same_noodle(&picked, &ends) => {}
            (Some(picked), ends) => model.move_noodle(&picked, ends.as_ref()),
            (None, Some(n)) => {
                let _ = model.try_add_noodle(n.from_node, &n.from_socket, n.to_node, &n.to_socket);
            }
            (None, None) => {}
        }
        drop(model);
        self.connection_ended();
    }

    /// What a drop at `canvas_pos` makes: a socket that accepts it, the
    /// auto-picked socket of a card body, nothing on empty canvas — or a
    /// refusal (a socket that refuses, a body with no pick) that changes
    /// nothing at all.
    fn drop_ends(
        &self,
        layouts: &[NodeLayoutInfo],
        canvas_pos: [f64; 2],
        fixed: &SocketRef,
        picked_up: Option<&NoodleView>,
    ) -> DropEnds {
        let model = self.model.lock().unwrap();
        if let Some((node, s)) = self.hit_socket(layouts, canvas_pos) {
            let to = socket_ref(node, &s);
            return match (to != *fixed)
                .then(|| accepts(&*model, fixed, picked_up, &to).ok())
                .flatten()
                .and_then(|()| fixed.noodle_to(&to))
            {
                Some(ends) => DropEnds::Connect(ends),
                None => DropEnds::Refused,
            };
        }
        let Some(body) = self.hit_node(layouts, canvas_pos) else {
            return DropEnds::Empty;
        };
        let candidates: Vec<SocketRef> = layouts
            .iter()
            .filter(|l| l.node_id == body)
            .flat_map(|l| {
                // Only shown sockets, as MatterCAD's `AutoPick`.
                l.sockets()
                    .filter(|s| !s.hidden)
                    .map(move |s| socket_ref(l.node_id, s))
            })
            .filter(|to| to != fixed && accepts(&*model, fixed, picked_up, to).is_ok())
            .collect();
        model
            .auto_pick_socket(body, fixed, &candidates)
            .and_then(|name| candidates.iter().find(|c| c.socket == name))
            .and_then(|to| fixed.noodle_to(to))
            .map_or(DropEnds::Refused, DropEnds::Connect)
    }

    /// Whether the drop landed or not, the in-flight noodle has to go.
    fn connection_ended(&mut self) {
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
    }

    /// The refusal note beside the pointer, in editor-local space, kept
    /// inside the editor. Called from `finish_paint` over everything else.
    pub(super) fn paint_refusal_note(&self, ctx: &mut dyn DrawCtx) {
        let CanvasState::DrawingConnection {
            refusal: Some(text),
            cursor_local,
            ..
        } = &self.interaction
        else {
            return;
        };
        ctx.set_font_size(NOTE_FONT);
        let text_w = ctx
            .measure_text(text)
            .map(|m| m.width)
            .unwrap_or(text.chars().count() as f64 * 7.0);
        let size = [text_w + 2.0 * NOTE_PAD, NOTE_FONT + 2.0 * NOTE_PAD];
        let [x, y] = refusal_note_position(
            [cursor_local.x, cursor_local.y],
            size,
            [self.bounds.width, self.bounds.height],
            NOTE_GAP,
        );
        // Styled as MatterCAD's tooltips: black on white, a 1 px border.
        ctx.set_fill_color(agg_gui::Color::white());
        ctx.begin_path();
        ctx.rect(x, y, size[0], size[1]);
        ctx.fill();
        ctx.set_stroke_color(agg_gui::Color::black());
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rect(x, y, size[0], size[1]);
        ctx.stroke();
        ctx.set_fill_color(agg_gui::Color::black());
        ctx.fill_text(text, x + NOTE_PAD, y + NOTE_PAD + 2.0);
    }
}

/// What a drop makes; see [`NodeEditor::drop_ends`].
enum DropEnds {
    Connect(NoodleView),
    Empty,
    Refused,
}

/// The noodle into input `socket` of `node` a press at `press` picks up:
/// the only one, or on a multi-input the one landing nearest the press.
fn nearest_noodle_into(
    model: &dyn NodeGraphModel,
    node: NodeId,
    socket: &SocketLayout,
    press: [f64; 2],
) -> Option<NoodleView> {
    let noodles = model.noodles();
    let distance = |n: &NoodleView| {
        let p = presentation::landing_point(&noodles, n, socket);
        (p[0] - press[0]).powi(2) + (p[1] - press[1]).powi(2)
    };
    noodles
        .iter()
        .filter(|n| n.to_node == node && n.to_socket == socket.name)
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .cloned()
}
