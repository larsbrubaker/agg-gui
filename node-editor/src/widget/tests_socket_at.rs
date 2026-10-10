//! Tests for `NodeEditor::socket_at` (the public socket hit test, the
//! same area a press, hover or drop uses) and `set_show_sockets` (sockets
//! drawn or not, hit-tested either way), both in `presentation.rs`.

use agg_gui::{Point, Size};

use super::hosted::HostedNodeBody;
use super::tests_common::{mk_node, Memory};
use super::*;
use crate::draw::SOCKET_HIT_RADIUS;
use crate::model::{NodeView, NoodleView, SocketView};
use crate::socket_style::NoodleStyle;
use crate::test_recorder::Recorder;

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};

fn socket(name: &str) -> SocketView {
    SocketView {
        name: name.into(),
        socket_type: SocketTypeId(0),
        display_label: None,
    }
}

/// Node 1 (`Out`) wired into node 2's `In`.
fn model() -> Memory {
    let mut m = Memory::default();
    let mut a = mk_node(1, "A", [0.0, 500.0]);
    a.outputs.push(socket("Out"));
    let mut b = mk_node(2, "B", [400.0, 400.0]);
    b.inputs.push(socket("In"));
    m.nodes = vec![a, b];
    m.noodles = vec![NoodleView {
        from_node: NodeId(1),
        from_socket: "Out".into(),
        to_node: NodeId(2),
        to_socket: "In".into(),
    }];
    m
}

fn editor_with(style: NoodleStyle) -> NodeEditor {
    let shared: SharedModel = Arc::new(Mutex::new(model()));
    let mut editor = NodeEditor::new(shared).with_noodle_style(style);
    editor.set_bounds(Rect::new(0.0, 0.0, VIEW.width, VIEW.height));
    editor.layout(VIEW);
    editor
}

fn out_hit() -> Option<(NodeId, SocketSide, String)> {
    Some((NodeId(1), SocketSide::Output, "Out".to_string()))
}

#[test]
fn socket_at_names_the_socket_within_the_simple_hit_circle() {
    let mut editor = editor_with(NoodleStyle::Simple);
    assert!(editor.set_view(1.5, [20.0, -10.0]));
    let c = editor
        .socket_position(NodeId(1), SocketSide::Output, "Out")
        .unwrap();
    assert_eq!(editor.socket_at(c), out_hit());
    let r = SOCKET_HIT_RADIUS * 1.5;
    assert_eq!(editor.socket_at(Point::new(c.x + r - 0.5, c.y)), out_hit());
    assert_eq!(editor.socket_at(Point::new(c.x + r + 0.5, c.y)), None);
    let input = editor
        .socket_position(NodeId(2), SocketSide::Input, "In")
        .unwrap();
    assert_eq!(
        editor.socket_at(input),
        Some((NodeId(2), SocketSide::Input, "In".to_string()))
    );
    assert_eq!(editor.socket_at(Point::new(700.0, 20.0)), None);
}

#[test]
fn a_node_designer_socket_is_hit_six_by_ten_units_either_side_never_under_eight_pixels() {
    for (scale, half_w, half_h) in [(1.5, 9.0, 15.0), (0.5, 8.0, 8.0)] {
        let mut editor = editor_with(NoodleStyle::NodeDesigner);
        assert!(editor.set_view(scale, [0.0, 0.0]));
        let c = editor
            .socket_position(NodeId(1), SocketSide::Output, "Out")
            .unwrap();
        assert_eq!(editor.socket_at(c), out_hit());
        for (dx, dy, inside) in [
            (half_w - 0.5, 0.0, true),
            (half_w + 0.5, 0.0, false),
            (0.0, half_h - 0.5, true),
            (0.0, -(half_h + 0.5), false),
            (half_w - 0.5, half_h - 0.5, true),
        ] {
            let hit = editor.socket_at(Point::new(c.x + dx, c.y + dy));
            assert_eq!(hit.is_some(), inside, "scale {scale}, ({dx}, {dy})");
        }
    }
}

/// Draw ops of the simplified (non-hosted) socket dots.
fn dot_shots(editor: &mut NodeEditor) -> usize {
    let mut r = Recorder::default();
    for child in editor.children_mut().iter_mut() {
        if child.type_name() == "NodeWidget" {
            r.save();
            let b = child.bounds();
            r.translate(b.x, b.y);
            agg_gui::widget::paint_subtree(child.as_mut(), &mut r);
            r.restore();
        }
    }
    r.shots.len()
}

#[test]
fn with_sockets_off_no_socket_dot_is_drawn_but_sockets_still_hit_test() {
    let mut editor = editor_with(NoodleStyle::Simple);
    assert!(editor.show_sockets());
    let shown = dot_shots(&mut editor);
    editor.set_show_sockets(false);
    assert!(!editor.show_sockets());
    editor.layout(VIEW);
    let hidden = dot_shots(&mut editor);
    // Two Simple circle sockets, one fill and one stroke each.
    assert_eq!(shown - hidden, 4);
    let c = editor
        .socket_position(NodeId(1), SocketSide::Output, "Out")
        .unwrap();
    assert_eq!(editor.socket_at(c), out_hit());
    editor.set_show_sockets(true);
    editor.layout(VIEW);
    assert_eq!(dot_shots(&mut editor), shown);
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
    fn on_event(&mut self, _: &agg_gui::Event) -> agg_gui::EventResult {
        agg_gui::EventResult::Ignored
    }
}

#[test]
fn with_sockets_off_a_hosted_card_draws_no_socket_but_keeps_its_hit_area() {
    let shared: SharedModel = Arc::new(Mutex::new(model()));
    let mut editor = NodeEditor::new(shared).with_body_factory(|_n: &NodeView| {
        Some(HostedNodeBody::new(Box::new(EmptyBody::default())))
    });
    editor.set_bounds(Rect::new(0.0, 0.0, VIEW.width, VIEW.height));
    editor.layout(VIEW);
    let overlay_shots = |editor: &mut NodeEditor| {
        let card = editor
            .children_mut()
            .last_mut()
            .and_then(|l| l.children_mut().first_mut())
            .and_then(|c| c.as_any_mut())
            .and_then(|a| a.downcast_mut::<HostedCard>())
            .expect("hosted card");
        let mut r = Recorder::default();
        card.paint_overlay(&mut r);
        (r.shots.len(), card.chrome.sockets.len())
    };
    let (shown, sockets) = overlay_shots(&mut editor);
    editor.set_show_sockets(false);
    editor.layout(VIEW);
    let (hidden, kept) = overlay_shots(&mut editor);
    assert_eq!(kept, sockets);
    // One Simple circle socket: one fill and one stroke.
    assert_eq!(shown - hidden, 2);
    let c = editor
        .socket_position(NodeId(1), SocketSide::Output, "Out")
        .unwrap();
    assert_eq!(editor.socket_at(c), out_hit());
}
