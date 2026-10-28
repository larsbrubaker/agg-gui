//! Tests for `NodeEditor::arrange_plan` (`arrange_plan.rs`): the plan is
//! built from hosted cards' real sizes and socket heights and the model's
//! roles, so after applying it the noodles between laid-out cards run
//! straight, the cards keep apart, and a second plan is empty.

use std::sync::{Arc, Mutex};

use agg_gui::{DrawCtx, Event, EventResult, Rect, Size, Widget};

use super::super::hosted::{HostedNodeBody, SocketAnchor};
use super::super::tests_common::{mk_node, Memory};
use super::super::{NodeEditor, SharedModel};
use crate::arrange::{ArrangeOptions, ArrangeRole};
use crate::draw::SocketSide;
use crate::model::{
    NodeGraphModel, NodeId, NodeTypeView, NodeView, NoodleResult, NoodleView, PropertyValue,
    SocketTypeId, SocketView,
};

/// `Memory` plus arrange roles: node 1 is the input, node 3 the output.
#[derive(Default)]
struct RoleModel {
    inner: Memory,
}

impl NodeGraphModel for RoleModel {
    fn nodes(&self) -> Vec<NodeView> {
        self.inner.nodes()
    }
    fn noodles(&self) -> Vec<NoodleView> {
        self.inner.noodles()
    }
    fn node_types_by_category(&self) -> Vec<(String, Vec<NodeTypeView>)> {
        vec![]
    }
    fn set_node_position(&mut self, id: NodeId, pos: [f64; 2]) {
        self.inner.set_node_position(id, pos)
    }
    fn add_node(&mut self, _: &str, _: [f64; 2]) -> Option<NodeId> {
        None
    }
    fn remove_node(&mut self, id: NodeId) {
        self.inner.remove_node(id)
    }
    fn try_add_noodle(&mut self, a: NodeId, b: &str, c: NodeId, d: &str) -> NoodleResult {
        self.inner.try_add_noodle(a, b, c, d)
    }
    fn remove_noodle(&mut self, a: NodeId, b: &str, c: NodeId, d: &str) -> bool {
        self.inner.remove_noodle(a, b, c, d)
    }
    fn set_property(&mut self, id: NodeId, name: &str, value: PropertyValue) {
        self.inner.set_property(id, name, value)
    }
    fn arrange_role(&self, id: NodeId) -> ArrangeRole {
        match id.0 {
            1 => ArrangeRole::Input,
            3 => ArrangeRole::Output,
            _ => ArrangeRole::Normal,
        }
    }
}

/// A hosted body of a fixed height.
struct FixedBody {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    height: f64,
}

impl Widget for FixedBody {
    fn type_name(&self) -> &'static str {
        "FixedBody"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn measure_min_height(&self, _w: f64) -> f64 {
        self.height
    }
    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, self.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn socket(name: &str) -> SocketView {
    SocketView {
        name: name.into(),
        socket_type: SocketTypeId(0),
        display_label: None,
    }
}

/// Body height and socket anchors (below the body's top) per node.
fn body_of(id: u64) -> (f64, Vec<(&'static str, SocketSide, f64)>) {
    match id {
        1 => (40.0, vec![("Out", SocketSide::Output, 10.0)]),
        2 => (
            90.0,
            vec![
                ("In", SocketSide::Input, 20.0),
                ("Out", SocketSide::Output, 70.0),
            ],
        ),
        _ => (60.0, vec![("In", SocketSide::Input, 50.0)]),
    }
}

/// Input 1 → 2 → Output 3, listed and placed out of order, as hosted cards.
fn editor() -> (NodeEditor, Arc<Mutex<RoleModel>>) {
    let model = Arc::new(Mutex::new(RoleModel::default()));
    {
        let mut m = model.lock().unwrap();
        let mut output = mk_node(3, "Output", [0.0, 0.0]);
        output.inputs.push(socket("In"));
        let mut mid = mk_node(2, "Mid", [400.0, 300.0]);
        mid.inputs.push(socket("In"));
        mid.outputs.push(socket("Out"));
        let mut input = mk_node(1, "Input", [250.0, -150.0]);
        input.outputs.push(socket("Out"));
        m.inner.nodes = vec![output, mid, input];
        for (a, b) in [(1, 2), (2, 3)] {
            m.inner.noodles.push(NoodleView {
                from_node: NodeId(a),
                from_socket: "Out".into(),
                to_node: NodeId(b),
                to_socket: "In".into(),
            });
        }
    }
    let shared: SharedModel = model.clone();
    let mut editor = NodeEditor::new(shared).with_body_factory(|n: &NodeView| {
        let (height, anchors) = body_of(n.id.0);
        let body = FixedBody {
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            children: Vec::new(),
            height,
        };
        Some(HostedNodeBody::new(Box::new(body)).with_socket_anchor(
            move |_w: &dyn Widget, name: &str| -> Option<SocketAnchor> {
                anchors
                    .iter()
                    .find(|(n, _, _)| *n == name)
                    .map(|(_, side, off)| (*side, *off))
            },
        ))
    });
    editor.layout(Size::new(800.0, 600.0));
    (editor, model)
}

#[test]
fn the_plan_uses_real_card_geometry_and_settles() {
    let (mut editor, model) = editor();
    let plan = editor.arrange_plan(&ArrangeOptions::default());
    assert_eq!(plan.len(), 3, "every card moves: {plan:?}");
    // The plan follows the model's node order and changes nothing yet.
    let order: Vec<u64> = plan.iter().map(|(id, _)| id.0).collect();
    assert_eq!(order, vec![3, 2, 1]);
    assert_eq!(model.lock().unwrap().inner.nodes[0].position, [0.0, 0.0]);

    for (id, pos) in &plan {
        model.lock().unwrap().set_node_position(*id, *pos);
    }
    editor.layout(Size::new(800.0, 600.0));
    let layouts = editor.snapshot_layouts();
    let find = |id: u64| layouts.iter().find(|l| l.node_id.0 == id).unwrap();
    let centre = |id: u64, side: SocketSide| {
        find(id)
            .sockets()
            .find(|s| s.side == side)
            .map(|s| s.center)
            .unwrap()
    };
    // Input left of the middle card, left of the output.
    assert!(find(1).top_left[0] < find(2).top_left[0]);
    assert!(find(2).top_left[0] < find(3).top_left[0]);
    // Both noodles are level between the laid-out sockets.
    for (a, b) in [(1, 2), (2, 3)] {
        let from = centre(a, SocketSide::Output);
        let to = centre(b, SocketSide::Input);
        assert!(
            (from[1] - to[1]).abs() < 1e-9,
            "noodle {a}->{b}: {from:?} {to:?}"
        );
        // Nothing overlaps: the next card starts past this one's right edge.
        assert!(find(a).top_left[0] + find(a).size[0] < find(b).top_left[0]);
    }
    // The graph's top-left corner did not move: the old left edge was the
    // output's x (0), the old top the middle card's top (300).
    let left = layouts
        .iter()
        .map(|l| l.top_left[0])
        .fold(f64::INFINITY, f64::min);
    let top = layouts
        .iter()
        .map(|l| l.top_left[1])
        .fold(f64::NEG_INFINITY, f64::max);
    assert_eq!([left, top], [0.0, 300.0]);
    assert!(editor.arrange_plan(&ArrangeOptions::default()).is_empty());
}
