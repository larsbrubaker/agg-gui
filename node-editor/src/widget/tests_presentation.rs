//! Tests for socket and noodle presentation on the live editor
//! (`presentation.rs`, `paint.rs`, the hosted card's and socket dot's
//! paint): model-chosen shapes reach the painted sockets, multi-input
//! noodles land in model order, dashes and colour overrides reach the
//! noodles, and the hover ring names the socket under the pointer.

use agg_gui::{Color, Event, Point, Size};

use super::hosted::HostedNodeBody;
use super::tests_common::{mk_node, Memory};
use super::*;
use crate::model::{NodeTypeView, NodeView, NoodleResult, NoodleView, PropertyValue, SocketView};
use crate::socket_style::{outline_color, NoodleStyle, SocketShape};
use crate::test_recorder::{same, Op, Recorder};

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};
const RED: Color = Color::rgb(1.0, 0.0, 0.0);

/// `Memory` plus the presentation hooks.
#[derive(Default)]
struct Styled {
    inner: Memory,
    shapes: Vec<(String, SocketShape)>,
    multi: Vec<String>,
    dashed: bool,
    color: Option<Color>,
    hover: Option<String>,
}

impl NodeGraphModel for Styled {
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
    fn socket_shape(&self, _: NodeId, _: SocketSide, socket: &str, _: SocketTypeId) -> SocketShape {
        self.shapes
            .iter()
            .find(|(n, _)| n == socket)
            .map(|(_, s)| *s)
            .unwrap_or_default()
    }
    fn socket_multi_input(&self, _: NodeId, socket: &str) -> bool {
        self.multi.iter().any(|m| m == socket)
    }
    fn noodle_dashed(&self, _: &NoodleView) -> bool {
        self.dashed
    }
    fn noodle_color(&self, _: &NoodleView) -> Option<Color> {
        self.color
    }
    fn socket_hover_text(&self, _: NodeId, _: SocketSide, socket: &str) -> Option<String> {
        self.hover.as_ref().map(|h| format!("{socket} ({h})"))
    }
}

fn socket(name: &str) -> SocketView {
    SocketView {
        name: name.into(),
        socket_type: SocketTypeId(0),
        display_label: None,
    }
}

fn noodle(from: u64, to: u64, to_socket: &str) -> NoodleView {
    NoodleView {
        from_node: NodeId(from),
        from_socket: "Out".into(),
        to_node: NodeId(to),
        to_socket: to_socket.into(),
    }
}

/// Two sources (`Out`) at different heights feeding node 3's `In`.
fn three_nodes(model: &mut Styled) {
    let mut a = mk_node(1, "A", [0.0, 500.0]);
    a.outputs.push(socket("Out"));
    let mut b = mk_node(2, "B", [0.0, 300.0]);
    b.outputs.push(socket("Out"));
    let mut c = mk_node(3, "C", [400.0, 400.0]);
    c.inputs.push(socket("In"));
    model.inner.nodes = vec![a, b, c];
    model.inner.noodles = vec![noodle(1, 3, "In"), noodle(2, 3, "In")];
}

fn editor_over(model: Styled) -> (NodeEditor, Arc<Mutex<Styled>>) {
    let model = Arc::new(Mutex::new(model));
    let shared: SharedModel = model.clone();
    let mut editor = NodeEditor::new(shared);
    editor.set_bounds(Rect::new(0.0, 0.0, VIEW.width, VIEW.height));
    editor.layout(VIEW);
    (editor, model)
}

/// The end points of every noodle core the canvas painted.
fn noodle_ends(r: &Recorder) -> Vec<[f64; 2]> {
    r.strokes()
        .filter_map(|s| match s.path.get(1) {
            Some(Op::Cubic(_, _, end)) => Some(*end),
            _ => None,
        })
        .collect()
}

fn in_socket_center(editor: &NodeEditor) -> [f64; 2] {
    editor
        .snapshot_layouts()
        .iter()
        .find(|l| l.node_id == NodeId(3))
        .and_then(|l| l.sockets().next().map(|s| s.center))
        .expect("node 3's In socket")
}

#[test]
fn multi_input_noodles_land_on_the_pill_in_model_order() {
    let mut model = Styled::default();
    three_nodes(&mut model);
    model.multi.push("In".into());
    let (mut editor, _) = editor_over(model);
    let c = in_socket_center(&editor);
    let mut r = Recorder::default();
    editor.paint_canvas(&mut r);
    assert_eq!(
        noodle_ends(&r),
        vec![[c[0], c[1] + 5.0], [c[0], c[1] - 5.0]]
    );
    let layouts = editor.snapshot_layouts();
    let s = layouts[2].sockets().next().unwrap();
    assert_eq!((s.landed, s.stretch()), (2, 5.0));
}

#[test]
fn without_multi_input_every_noodle_lands_on_the_socket_centre() {
    let mut model = Styled::default();
    three_nodes(&mut model);
    let (mut editor, _) = editor_over(model);
    let c = in_socket_center(&editor);
    let mut r = Recorder::default();
    editor.paint_canvas(&mut r);
    assert_eq!(noodle_ends(&r), vec![c, c]);
}

#[test]
fn dashed_noodles_and_colour_overrides_come_from_the_model() {
    let mut model = Styled::default();
    three_nodes(&mut model);
    model.dashed = true;
    model.color = Some(RED);
    let (mut editor, _) = editor_over(model);
    let mut r = Recorder::default();
    editor.paint_canvas(&mut r);
    let noodles: Vec<_> = r
        .strokes()
        .filter(|s| matches!(s.path.get(1), Some(Op::Cubic(..))))
        .collect();
    assert_eq!(noodles.len(), 2);
    assert!(noodles
        .iter()
        .all(|s| s.dash == vec![8.0, 8.0] && same(s.color, RED)));
}

#[test]
fn the_node_designer_style_paints_edge_core_and_dot_per_noodle() {
    let mut model = Styled::default();
    three_nodes(&mut model);
    let (editor, _) = editor_over(model);
    let mut editor = editor.with_noodle_style(NoodleStyle::NodeDesigner);
    assert_eq!(editor.noodle_style(), NoodleStyle::NodeDesigner);
    let mut r = Recorder::default();
    editor.paint_canvas(&mut r);
    let widths: Vec<f64> = r
        .strokes()
        .filter(|s| matches!(s.path.get(1), Some(Op::Cubic(..))))
        .map(|s| s.width)
        .collect();
    assert_eq!(widths, vec![7.0, 3.0, 7.0, 3.0]);
    let dots = r
        .fills()
        .filter(|s| matches!(s.path[..], [Op::Circle(_, r)] if r == 5.0))
        .count();
    assert_eq!(dots, 2);
}

#[test]
fn the_socket_dot_widget_paints_the_models_shape() {
    let mut model = Styled::default();
    three_nodes(&mut model);
    model.shapes.push(("In".into(), SocketShape::DiamondDot));
    let (mut editor, _) = editor_over(model);
    editor.layout(VIEW);
    let mut found = false;
    let mut r = Recorder::default();
    // The node widgets directly: the editor itself blits a backbuffer.
    for child in editor.children_mut().iter_mut() {
        if child.type_name() == "NodeWidget" {
            r.save();
            let b = child.bounds();
            r.translate(b.x, b.y);
            agg_gui::widget::paint_subtree(child.as_mut(), &mut r);
            r.restore();
        }
    }
    // The diamond's points and its dot were drawn: 3 fills with no stroke
    // in the outline colour round a dot of radius 2.
    for w in r.fills().collect::<Vec<_>>().windows(3) {
        if matches!(w[0].path[0], Op::MoveTo(_))
            && matches!(w[1].path[0], Op::MoveTo(_))
            && matches!(w[2].path[..], [Op::Circle(_, r)] if (r - 2.0).abs() < 1e-9)
        {
            found = true;
        }
    }
    assert!(
        found,
        "a diamond with a dot was painted for the field input"
    );
}

/// A hosted body with a fixed height and nothing to paint.
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
fn a_hosted_card_paints_shaped_sockets_in_its_style() {
    let mut model = Styled::default();
    let mut n = mk_node(1, "Box", [100.0, 400.0]);
    n.inputs.push(socket("Count"));
    model.inner.nodes = vec![n];
    model.shapes.push(("Count".into(), SocketShape::Bar));
    let model = Arc::new(Mutex::new(model));
    let shared: SharedModel = model.clone();
    let mut editor = NodeEditor::new(shared)
        .with_noodle_style(NoodleStyle::NodeDesigner)
        .with_body_factory(|_n: &NodeView| {
            Some(HostedNodeBody::new(Box::new(EmptyBody::default())))
        });
    editor.set_bounds(Rect::new(0.0, 0.0, VIEW.width, VIEW.height));
    editor.layout(VIEW);
    let layer = editor.children_mut().last_mut().expect("hosted layer");
    let card = &mut layer.children_mut()[0];
    let mut r = Recorder::default();
    card.paint_overlay(&mut r);
    let fills: Vec<_> = r.fills().collect();
    assert_eq!(fills.len(), 2);
    assert!(same(fills[0].color, outline_color()));
    assert!(matches!(fills[0].path[..], [Op::Rect(_, [w, h])] if w == 8.0 && h == 16.0));
}

#[test]
fn hovering_a_socket_rings_it_and_names_it() {
    let mut model = Styled::default();
    three_nodes(&mut model);
    model.hover = Some("geometry".into());
    let (editor, _) = editor_over(model);
    let mut editor = editor.with_socket_hover(true);
    let c = in_socket_center(&editor);
    editor.on_event(&Event::MouseMove {
        pos: Point::new(c[0], c[1]),
    });
    assert_eq!(
        editor.hovered_socket(),
        Some((NodeId(3), SocketSide::Input, "In"))
    );
    let mut r = Recorder::default();
    editor.paint_socket_hover(&mut r);
    assert!(r
        .strokes()
        .any(|s| s.width == 2.0 && matches!(s.path[..], [Op::Circle(p, _)] if p == c)));
    assert_eq!(r.texts, vec!["In (geometry)".to_string()]);

    // Moving off the socket clears the ring.
    editor.on_event(&Event::MouseMove {
        pos: Point::new(c[0] + 100.0, c[1] - 100.0),
    });
    assert_eq!(editor.hovered_socket(), None);
}

#[test]
fn socket_hover_is_off_by_default() {
    let mut model = Styled::default();
    three_nodes(&mut model);
    let (mut editor, _) = editor_over(model);
    let c = in_socket_center(&editor);
    editor.on_event(&Event::MouseMove {
        pos: Point::new(c[0], c[1]),
    });
    assert_eq!(editor.hovered_socket(), None);
}
