//! Tests for the host-facing socket queries and hooks: downcasting the
//! editor through `Widget::as_any` (`mod.rs`), `NodeEditor::socket_position`
//! (`presentation.rs`), `NodeGraphModel::socket_visible` — a hidden socket
//! is neither drawn (simplified socket dot, hosted card) nor hit-tested
//! (hover, press, drop snap) while a noodle wired to it still draws — and
//! the butt caps of a dashed noodle painted by the editor (`paint.rs`).

use agg_gui::{Event, Modifiers, MouseButton, Point, Size};

use super::hosted::HostedNodeBody;
use super::tests_common::{mk_node, Memory};
use super::*;
use crate::connection::SocketRef;
use crate::model::{NodeTypeView, NodeView, NoodleResult, NoodleView, PropertyValue, SocketView};
use crate::test_recorder::{Op, Recorder};

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};

/// `Memory` plus `socket_visible` and `noodle_dashed`.
#[derive(Default)]
struct Hiding {
    inner: Memory,
    hidden: Vec<(NodeId, SocketSide, String)>,
    dashed: bool,
    /// Candidate socket names of every `auto_pick_socket` call.
    offered: std::cell::RefCell<Vec<Vec<String>>>,
}

impl NodeGraphModel for Hiding {
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
    fn socket_visible(&self, node: NodeId, side: SocketSide, socket: &str) -> bool {
        !self
            .hidden
            .iter()
            .any(|(n, s, name)| *n == node && *s == side && name == socket)
    }
    fn noodle_dashed(&self, _: &NoodleView) -> bool {
        self.dashed
    }
    fn auto_pick_socket(
        &self,
        _node: NodeId,
        _from: &SocketRef,
        candidates: &[SocketRef],
    ) -> Option<String> {
        self.offered
            .borrow_mut()
            .push(candidates.iter().map(|c| c.socket.clone()).collect());
        candidates.first().map(|c| c.socket.clone())
    }
}

fn socket(name: &str) -> SocketView {
    SocketView {
        name: name.into(),
        socket_type: SocketTypeId(0),
        display_label: None,
    }
}

/// Node 1 (`Out`) wired into node 3's `In`; node 2 has an unwired `Free`
/// input and node 3 an unwired `Spare` input.
fn graph(model: &mut Hiding) {
    let mut a = mk_node(1, "A", [0.0, 500.0]);
    a.outputs.push(socket("Out"));
    let mut b = mk_node(2, "B", [400.0, 200.0]);
    b.inputs.push(socket("Free"));
    let mut c = mk_node(3, "C", [400.0, 400.0]);
    c.inputs.push(socket("In"));
    c.inputs.push(socket("Spare"));
    model.inner.nodes = vec![a, b, c];
    model.inner.noodles = vec![NoodleView {
        from_node: NodeId(1),
        from_socket: "Out".into(),
        to_node: NodeId(3),
        to_socket: "In".into(),
    }];
}

fn editor_over(model: Hiding) -> (NodeEditor, Arc<Mutex<Hiding>>) {
    let model = Arc::new(Mutex::new(model));
    let shared: SharedModel = model.clone();
    let mut editor = NodeEditor::new(shared);
    editor.set_bounds(Rect::new(0.0, 0.0, VIEW.width, VIEW.height));
    editor.layout(VIEW);
    (editor, model)
}

fn input(node: u64, name: &str) -> (NodeId, SocketSide, String) {
    (NodeId(node), SocketSide::Input, name.into())
}

fn press(editor: &mut NodeEditor, p: Point) {
    editor.on_event(&Event::MouseDown {
        pos: p,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

#[test]
fn a_host_downcasts_the_editor_through_as_any() {
    let mut model = Hiding::default();
    graph(&mut model);
    let (editor, _) = editor_over(model);
    let mut boxed: Box<dyn Widget> = Box::new(editor);
    let editor = boxed
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<NodeEditor>())
        .expect("NodeEditor downcasts");
    assert!(editor.select_node(NodeId(2), false));
    assert!(editor.selected_ids().contains(&NodeId(2)));
    assert!(boxed
        .as_any()
        .and_then(|a| a.downcast_ref::<NodeEditor>())
        .is_some_and(|e| e.open_context_menu().is_none()));
}

#[test]
fn socket_position_is_the_laid_out_centre_at_the_current_view() {
    let mut model = Hiding::default();
    graph(&mut model);
    let (mut editor, _) = editor_over(model);
    let centre = editor
        .snapshot_layouts()
        .iter()
        .find(|l| l.node_id == NodeId(3))
        .and_then(|l| l.sockets().find(|s| s.name == "Spare").map(|s| s.center))
        .unwrap();
    assert_eq!(
        editor.socket_position(NodeId(3), SocketSide::Input, "Spare"),
        Some(Point::new(centre[0], centre[1]))
    );
    assert!(editor.set_view(2.0, [10.0, -20.0]));
    assert_eq!(
        editor.socket_position(NodeId(3), SocketSide::Input, "Spare"),
        Some(Point::new(centre[0] * 2.0 + 10.0, centre[1] * 2.0 - 20.0))
    );
    // Wrong side, unknown socket, unknown node.
    assert_eq!(
        editor.socket_position(NodeId(3), SocketSide::Output, "Spare"),
        None
    );
    assert_eq!(
        editor.socket_position(NodeId(3), SocketSide::Input, "Nope"),
        None
    );
    assert_eq!(
        editor.socket_position(NodeId(9), SocketSide::Input, "In"),
        None
    );
}

#[test]
fn a_hidden_socket_is_not_hit_tested_but_its_noodle_still_draws() {
    let mut model = Hiding::default();
    graph(&mut model);
    model.hidden = vec![input(3, "In"), input(3, "Spare")];
    let (editor, _) = editor_over(model);
    let mut editor = editor.with_socket_hover(true);
    // Hidden sockets still report their place: the wired noodle ends there.
    let wired = editor
        .socket_position(NodeId(3), SocketSide::Input, "In")
        .unwrap();
    let mut r = Recorder::default();
    editor.paint_canvas(&mut r);
    let ends: Vec<[f64; 2]> = r
        .strokes()
        .filter_map(|s| match s.path.get(1) {
            Some(Op::Cubic(_, _, end)) => Some(*end),
            _ => None,
        })
        .collect();
    assert_eq!(ends, vec![[wired.x, wired.y]]);

    // No hover ring and no drag from a hidden socket.
    let spare = editor
        .socket_position(NodeId(3), SocketSide::Input, "Spare")
        .unwrap();
    editor.on_event(&Event::MouseMove { pos: spare });
    assert_eq!(editor.hovered_socket(), None);
    press(&mut editor, spare);
    assert!(!matches!(
        editor.interaction,
        CanvasState::DrawingConnection { .. }
    ));
}

#[test]
fn a_dragged_noodle_does_not_snap_to_a_hidden_socket() {
    let mut model = Hiding::default();
    graph(&mut model);
    model.hidden = vec![input(2, "Free")];
    let (editor, model) = editor_over(model);
    let free = editor
        .socket_position(NodeId(2), SocketSide::Input, "Free")
        .unwrap();
    let fixed = SocketRef {
        node: NodeId(1),
        side: SocketSide::Output,
        socket: "Out".into(),
        socket_type: SocketTypeId(0),
    };
    let near = |editor: &NodeEditor| {
        let layouts = editor.snapshot_layouts();
        let m = model.lock().unwrap();
        connect::find_target_near(&layouts, &*m, [free.x, free.y], &fixed, None)
            .map(|s| s.name.clone())
    };
    assert_eq!(near(&editor), None);
    model.lock().unwrap().hidden.clear();
    assert_eq!(near(&editor), Some("Free".to_string()));
}

#[test]
fn a_drop_on_a_card_body_offers_only_its_shown_sockets() {
    let mut model = Hiding::default();
    graph(&mut model);
    model.inner.noodles.clear();
    model.hidden = vec![input(3, "In")];
    let (mut editor, model) = editor_over(model);
    let out = editor
        .socket_position(NodeId(1), SocketSide::Output, "Out")
        .unwrap();
    // Node 3's body, between its title bar and its sockets' column.
    let spare = editor
        .socket_position(NodeId(3), SocketSide::Input, "Spare")
        .unwrap();
    let body = Point::new(spare.x + 60.0, spare.y);
    press(&mut editor, out);
    editor.on_event(&Event::MouseMove { pos: body });
    editor.on_event(&Event::MouseUp {
        pos: body,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    let m = model.lock().unwrap();
    assert_eq!(*m.offered.borrow(), vec![vec!["Spare".to_string()]]);
    // The pick of the one shown socket connected.
    assert!(m
        .inner
        .noodles
        .iter()
        .any(|n| n.to_node == NodeId(3) && n.to_socket == "Spare"));
}

#[test]
fn the_socket_dot_of_a_hidden_socket_paints_nothing() {
    let mut model = Hiding::default();
    graph(&mut model);
    model.hidden = vec![input(2, "Free")];
    let (mut editor, model) = editor_over(model);
    let paint_dots = |editor: &mut NodeEditor| {
        let mut r = Recorder::default();
        for child in editor.children_mut().iter_mut() {
            if child.type_name() == "NodeWidget" && child.bounds().x > 300.0 {
                r.save();
                let b = child.bounds();
                r.translate(b.x, b.y);
                agg_gui::widget::paint_subtree(child.as_mut(), &mut r);
                r.restore();
            }
        }
        r.shots.len()
    };
    let hidden_shots = paint_dots(&mut editor);
    model.lock().unwrap().hidden.clear();
    editor.layout(VIEW);
    let shown_shots = paint_dots(&mut editor);
    // The Simple circle socket is one fill and one stroke.
    assert_eq!(shown_shots - hidden_shots, 2);
}

/// A hosted card body with no children of its own.
#[derive(Default)]
struct EmptyBody {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for EmptyBody {
    fn type_name(&self) -> &'static str {
        "EmptyBody"
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
        40.0
    }
    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, 40.0)
    }
    fn paint(&mut self, _ctx: &mut dyn agg_gui::DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> agg_gui::EventResult {
        agg_gui::EventResult::Ignored
    }
}

#[test]
fn a_hosted_card_draws_only_its_visible_sockets() {
    let mut model = Hiding::default();
    let mut n = mk_node(1, "Box", [100.0, 400.0]);
    n.inputs.push(socket("Shown"));
    n.inputs.push(socket("Gone"));
    model.inner.nodes = vec![n];
    model.hidden = vec![input(1, "Gone")];
    let model = Arc::new(Mutex::new(model));
    let shared: SharedModel = model.clone();
    let mut editor = NodeEditor::new(shared).with_body_factory(|_n: &NodeView| {
        Some(HostedNodeBody::new(Box::new(EmptyBody::default())))
    });
    editor.set_bounds(Rect::new(0.0, 0.0, VIEW.width, VIEW.height));
    editor.layout(VIEW);
    let card = editor
        .children()
        .last()
        .and_then(|l| l.children().first())
        .and_then(|c| c.as_any())
        .and_then(|a| a.downcast_ref::<HostedCard>())
        .expect("hosted card");
    assert_eq!(card.chrome.sockets.len(), 1);
    // The hidden one keeps its laid-out place but takes no press.
    let gone = editor
        .socket_position(NodeId(1), SocketSide::Input, "Gone")
        .unwrap();
    let layouts = editor.snapshot_layouts();
    assert!(editor.hit_socket(&layouts, [gone.x, gone.y]).is_none());
    let shown = editor
        .socket_position(NodeId(1), SocketSide::Input, "Shown")
        .unwrap();
    assert!(editor.hit_socket(&layouts, [shown.x, shown.y]).is_some());
}

#[test]
fn the_editor_strokes_a_dashed_noodle_with_butt_caps() {
    let mut model = Hiding::default();
    graph(&mut model);
    model.dashed = true;
    let (mut editor, _) = editor_over(model);
    let mut r = Recorder::default();
    editor.paint_canvas(&mut r);
    let noodles: Vec<_> = r
        .strokes()
        .filter(|s| matches!(s.path.get(1), Some(Op::Cubic(..))))
        .collect();
    assert_eq!(noodles.len(), 1);
    assert!(noodles
        .iter()
        .all(|s| s.dash == vec![8.0, 8.0] && s.cap == agg_gui::LineCap::Butt));
}
