//! Tests for the connection helpers in [`crate::connection`]: the default
//! accept rule, a drag's noodle orientation, NodeDesigner's auto-pick
//! order and the refusal note's placement (MatterCAD's
//! `NoodleDragController.RefusalPosition`).

use crate::connection::{
    default_can_connect, node_designer_auto_pick, refusal_note_position, SocketRef,
};
use crate::draw::SocketSide;
use crate::model::{
    NodeGraphModel, NodeId, NodeTypeView, NodeView, NoodleResult, NoodleView, PropertyValue,
    SocketTypeId,
};

fn sock(node: u64, side: SocketSide, name: &str, ty: u32) -> SocketRef {
    SocketRef {
        node: NodeId(node),
        side,
        socket: name.into(),
        socket_type: SocketTypeId(ty),
    }
}

/// A model with nothing in it: only the default rules are under test.
struct Empty;

impl NodeGraphModel for Empty {
    fn nodes(&self) -> Vec<NodeView> {
        vec![]
    }
    fn noodles(&self) -> Vec<NoodleView> {
        vec![]
    }
    fn node_types_by_category(&self) -> Vec<(String, Vec<NodeTypeView>)> {
        vec![]
    }
    fn set_node_position(&mut self, _: NodeId, _: [f64; 2]) {}
    fn add_node(&mut self, _: &str, _: [f64; 2]) -> Option<NodeId> {
        None
    }
    fn remove_node(&mut self, _: NodeId) {}
    fn try_add_noodle(&mut self, _: NodeId, _: &str, _: NodeId, _: &str) -> NoodleResult {
        NoodleResult::Rejected
    }
    fn remove_noodle(&mut self, _: NodeId, _: &str, _: NodeId, _: &str) -> bool {
        false
    }
    fn set_property(&mut self, _: NodeId, _: &str, _: PropertyValue) {}
}

#[test]
fn default_can_connect_takes_an_output_to_a_compatible_input_on_another_node() {
    let out = sock(1, SocketSide::Output, "out", 3);
    assert_eq!(
        default_can_connect(&Empty, &out, &sock(2, SocketSide::Input, "in", 3)),
        Ok(())
    );
    // Dragged backwards from the input, the same pair is fine.
    assert_eq!(
        default_can_connect(&Empty, &sock(2, SocketSide::Input, "in", 3), &out),
        Ok(())
    );
    // Silent refusals: another type, the same side, the same node.
    let refused = Err(String::new());
    assert_eq!(
        default_can_connect(&Empty, &out, &sock(2, SocketSide::Input, "in", 4)),
        refused
    );
    assert_eq!(
        default_can_connect(&Empty, &out, &sock(2, SocketSide::Output, "o", 3)),
        refused
    );
    assert_eq!(
        default_can_connect(&Empty, &out, &sock(1, SocketSide::Input, "in", 3)),
        refused
    );
    // The trait's default is the same rule.
    assert_eq!(
        Empty.can_connect(&out, &sock(2, SocketSide::Input, "in", 4)),
        refused
    );
}

#[test]
fn noodle_to_puts_the_output_first_either_way_round() {
    let out = sock(1, SocketSide::Output, "out", 0);
    let inp = sock(2, SocketSide::Input, "in", 0);
    for n in [out.noodle_to(&inp).unwrap(), inp.noodle_to(&out).unwrap()] {
        assert_eq!((n.from_node, n.from_socket.as_str()), (NodeId(1), "out"));
        assert_eq!((n.to_node, n.to_socket.as_str()), (NodeId(2), "in"));
    }
    assert!(out.noodle_to(&out).is_none());
}

#[test]
fn auto_pick_prefers_the_same_type_then_any_type_then_a_free_input() {
    const ANY: u32 = 0;
    let any = |t: SocketTypeId| t.0 == ANY;
    let from = sock(1, SocketSide::Output, "out", 5);
    let taken = NoodleView {
        from_node: NodeId(9),
        from_socket: "x".into(),
        to_node: NodeId(2),
        to_socket: "first".into(),
    };
    let first = sock(2, SocketSide::Input, "first", 7);
    let free = sock(2, SocketSide::Input, "free", 7);
    let anything = sock(2, SocketSide::Input, "anything", ANY);
    let same = sock(2, SocketSide::Input, "same", 5);
    let all = [first, free, anything, same];
    let pick =
        |c: &[SocketRef]| node_designer_auto_pick(&from, c, any, std::slice::from_ref(&taken));
    assert_eq!(pick(&all).as_deref(), Some("same"));
    assert_eq!(pick(&all[..3]).as_deref(), Some("anything"));
    assert_eq!(pick(&all[..2]).as_deref(), Some("free"));
    assert_eq!(pick(&all[..1]), None, "the only input already has a noodle");
    assert_eq!(pick(&[]), None);
    // Dragging backwards, an output is always free to take another noodle.
    let from_input = sock(3, SocketSide::Input, "in", 6);
    let outputs = [sock(2, SocketSide::Output, "o", 8)];
    assert_eq!(
        node_designer_auto_pick(&from_input, &outputs, any, &[]).as_deref(),
        Some("o")
    );
}

#[test]
fn refusal_note_sits_up_and_right_of_the_pointer() {
    let p = refusal_note_position([100.0, 100.0], [80.0, 20.0], [400.0, 300.0], 12.0);
    assert_eq!(p, [112.0, 112.0]);
}

#[test]
fn refusal_note_flips_at_the_right_and_top_edges() {
    let p = refusal_note_position([350.0, 290.0], [80.0, 20.0], [400.0, 300.0], 12.0);
    assert_eq!(p, [350.0 - 12.0 - 80.0, 290.0 - 12.0 - 20.0]);
}

#[test]
fn refusal_note_stays_inside_an_editor_too_small_for_either_side() {
    // Too wide to fit right or left of the pointer: pinned to the left edge.
    let p = refusal_note_position([50.0, 10.0], [90.0, 20.0], [100.0, 100.0], 12.0);
    assert_eq!(p, [0.0, 22.0]);
}
