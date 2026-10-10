//! Connection semantics for noodle drags: the socket ends a drag carries,
//! the default rule for which socket accepts a noodle, NodeDesigner's pick
//! of a socket when a noodle is dropped on a card body, and where the
//! refusal note goes.
//!
//! The model decides (see [`crate::NodeGraphModel::can_connect`],
//! [`crate::NodeGraphModel::auto_pick_socket`] and
//! [`crate::NodeGraphModel::move_noodle`]); the editor's drag code
//! (`widget::connect`) asks it on every pointer move and at the drop.
//! MatterCAD's reference is `NoodleDragController` (`DropEnds`,
//! `AutoPick`, `RefusalPosition`).

use crate::draw::SocketSide;
use crate::model::{NodeGraphModel, NodeId, NoodleView, SocketTypeId};

/// One socket as a noodle drag sees it: the end the drag started from
/// (the output pressed, the empty input pressed when dragging backwards,
/// or the source of a noodle picked up off its input), or a socket the
/// pointer is over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketRef {
    pub node: NodeId,
    pub side: SocketSide,
    pub socket: String,
    pub socket_type: SocketTypeId,
}

impl SocketRef {
    /// The noodle a drop of a drag from `from` onto `to` makes, output
    /// end first, or `None` when both are on the same side.
    pub fn noodle_to(&self, to: &SocketRef) -> Option<NoodleView> {
        let (out, inp) = match (self.side, to.side) {
            (SocketSide::Output, SocketSide::Input) => (self, to),
            (SocketSide::Input, SocketSide::Output) => (to, self),
            _ => return None,
        };
        Some(NoodleView {
            from_node: out.node,
            from_socket: out.socket.clone(),
            to_node: inp.node,
            to_socket: inp.socket.clone(),
        })
    }
}

/// The editor's built-in rule, which [`NodeGraphModel::can_connect`]
/// answers unless a host overrides it: an output to an input, on another
/// node, with [`NodeGraphModel::sockets_compatible`] types. Refuses with
/// an empty reason, so no note is shown.
pub fn default_can_connect<M: NodeGraphModel + ?Sized>(
    model: &M,
    from: &SocketRef,
    to: &SocketRef,
) -> Result<(), String> {
    if from.side == to.side || from.node == to.node {
        return Err(String::new());
    }
    let (out_ty, in_ty) = match from.side {
        SocketSide::Output => (from.socket_type, to.socket_type),
        SocketSide::Input => (to.socket_type, from.socket_type),
    };
    if model.sockets_compatible(out_ty, in_ty) {
        Ok(())
    } else {
        Err(String::new())
    }
}

/// NodeDesigner's pick of a socket for a noodle dropped on a card body
/// (MatterCAD's `AutoPick`), for hosts to answer
/// [`NodeGraphModel::auto_pick_socket`] with. Of `candidates` (the card's
/// shown sockets that accept the noodle, in card order): one of the same
/// type as `from` first, unless `from` takes any type; then one that takes
/// any type; then the first output, or input with no noodle yet in
/// `noodles`.
pub fn node_designer_auto_pick(
    from: &SocketRef,
    candidates: &[SocketRef],
    takes_any_type: impl Fn(SocketTypeId) -> bool,
    noodles: &[NoodleView],
) -> Option<String> {
    let same_type = (!takes_any_type(from.socket_type))
        .then(|| {
            candidates
                .iter()
                .find(|c| c.socket_type == from.socket_type)
        })
        .flatten();
    same_type
        .or_else(|| candidates.iter().find(|c| takes_any_type(c.socket_type)))
        .or_else(|| {
            candidates.iter().find(|c| {
                c.side == SocketSide::Output
                    || !noodles
                        .iter()
                        .any(|n| n.to_node == c.node && n.to_socket == c.socket)
            })
        })
        .map(|c| c.socket.clone())
}

/// Where the refusal note of `size` goes for a pointer at `pointer`, both
/// editor-local (Y-up): up and right of it by `gap`, flipped to the
/// pointer's other side where it would run past the editor's right or top
/// edge, and kept inside the editor (MatterCAD's `RefusalPosition`).
/// Returns the note's bottom-left corner.
pub fn refusal_note_position(
    pointer: [f64; 2],
    size: [f64; 2],
    editor_size: [f64; 2],
    gap: f64,
) -> [f64; 2] {
    let x = if pointer[0] + gap + size[0] <= editor_size[0] {
        pointer[0] + gap
    } else {
        pointer[0] - gap - size[0]
    };
    let y = if pointer[1] + gap + size[1] <= editor_size[1] {
        pointer[1] + gap
    } else {
        pointer[1] - gap - size[1]
    };
    [
        0f64.max(x.min(editor_size[0] - size[0])),
        0f64.max(y.min(editor_size[1] - size[1])),
    ]
}
