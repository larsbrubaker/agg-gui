//! Tests for hosted cards (`hosted.rs`, `hosted_card.rs`,
//! `hosted_events.rs`): a host-built body is laid out inside its card,
//! receives a click at the right place under pan and zoom through the
//! framework's own hit-test and dispatch, is rebuilt only when the body
//! epoch changes, and sizes the card (measured height reported back,
//! width from the model and its right-edge drag).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui::widget::{dispatch_event_dyn, hit_test_subtree};
use agg_gui::{DrawCtx, Event, EventResult, Modifiers, MouseButton, Point, Rect, Size};

use super::hosted::{HostedNodeBody, SocketAnchor};
use super::hosted_card::HostedCard;
use super::tests_common::{mk_node, Memory};
use super::*;
use crate::draw::{NODE_BOTTOM_PAD, NODE_WIDTH, TITLE_HEIGHT};
use crate::model::{NodeTypeView, NodeView, NoodleResult, NoodleView, PropertyValue, SocketView};

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};
const BODY_H: f64 = 50.0;

/// A model with the hosted-card hooks, delegating the rest to `Memory`.
#[derive(Default)]
struct HostedModel {
    inner: Memory,
    epoch: u64,
    widths: Vec<(NodeId, f64)>,
    measured: Vec<(NodeId, f64)>,
}

impl NodeGraphModel for HostedModel {
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
    fn on_primary_selection_changed(&mut self, id: Option<NodeId>) {
        self.inner.on_primary_selection_changed(id)
    }
    fn primary_selection(&self) -> Option<NodeId> {
        self.inner.primary_selection()
    }
    fn body_epoch(&self) -> u64 {
        self.epoch
    }
    fn node_width(&self, id: NodeId) -> Option<f64> {
        self.widths
            .iter()
            .rev()
            .find(|(n, _)| *n == id)
            .map(|(_, w)| *w)
    }
    fn set_node_width(&mut self, id: NodeId, width: f64) {
        self.widths.push((id, width));
    }
    fn on_node_measured(&mut self, id: NodeId, height: f64) {
        self.measured.push((id, height));
    }
}

/// A body widget of a chosen height that records the local position of
/// every mouse-down it gets, and consumes it.
struct ClickBody {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    height: Rc<Cell<f64>>,
    clicks: Rc<RefCell<Vec<Point>>>,
}

impl Widget for ClickBody {
    fn type_name(&self) -> &'static str {
        "ClickBody"
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
        self.height.get()
    }
    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, self.height.get())
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::MouseDown { pos, .. } = event {
            self.clicks.borrow_mut().push(*pos);
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
}

struct Fixture {
    editor: NodeEditor,
    model: Arc<Mutex<HostedModel>>,
    builds: Rc<Cell<usize>>,
    height: Rc<Cell<f64>>,
    clicks: Rc<RefCell<Vec<Point>>>,
}

fn node_with_sockets(id: u64, pos: [f64; 2]) -> NodeView {
    let mut n = mk_node(id, "Box", pos);
    n.outputs.push(SocketView {
        name: "Result".into(),
        socket_type: SocketTypeId(0),
        display_label: None,
    });
    n.inputs.push(SocketView {
        name: "Width".into(),
        socket_type: SocketTypeId(0),
        display_label: None,
    });
    n
}

/// One node at canvas top-left `(100, 400)` with a hosted `ClickBody`;
/// its `Width` input is anchored 30 units below the body's top.
fn fixture() -> Fixture {
    let model = Arc::new(Mutex::new(HostedModel::default()));
    model.lock().unwrap().inner.nodes = vec![node_with_sockets(1, [100.0, 400.0])];
    let shared: SharedModel = model.clone();
    let builds = Rc::new(Cell::new(0));
    let height = Rc::new(Cell::new(BODY_H));
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let (b, h, c) = (builds.clone(), height.clone(), clicks.clone());
    let editor = NodeEditor::new(shared).with_body_factory(move |_n: &NodeView| {
        b.set(b.get() + 1);
        let body = ClickBody {
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            children: Vec::new(),
            height: h.clone(),
            clicks: c.clone(),
        };
        Some(HostedNodeBody::new(Box::new(body)).with_socket_anchor(
            |_w: &dyn Widget, name: &str| -> Option<SocketAnchor> {
                (name == "Width").then_some((crate::draw::SocketSide::Input, 30.0))
            },
        ))
    });
    let mut f = Fixture {
        editor,
        model,
        builds,
        height,
        clicks,
    };
    f.editor.layout(VIEW);
    f
}

fn card(editor: &NodeEditor) -> &HostedCard {
    let layer = editor.children().last().expect("the hosted layer");
    layer.children()[0]
        .as_any()
        .and_then(|a| a.downcast_ref::<HostedCard>())
        .expect("a hosted card")
}

/// Run `event` through the framework's hit-test and dispatch, as the App
/// does, with `pos` in editor-local coordinates.
fn dispatch(editor: &mut NodeEditor, event: Event, pos: Point) -> EventResult {
    let path = hit_test_subtree(editor, pos).expect("the editor is hit");
    dispatch_event_dyn(editor, &path, &event, pos)
}

fn press(pos: Point) -> Event {
    Event::MouseDown {
        pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    }
}

fn release(pos: Point) -> Event {
    Event::MouseUp {
        pos,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    }
}

fn card_height(body: f64) -> f64 {
    TITLE_HEIGHT + body + NODE_BOTTOM_PAD
}

#[test]
fn a_hosted_body_is_laid_out_inside_its_card() {
    let f = fixture();
    let card = card(&f.editor);
    let height = card_height(BODY_H);
    assert_eq!(
        card.bounds(),
        Rect::new(100.0, 400.0 - height, NODE_WIDTH, height)
    );
    let body = &card.children()[0];
    assert_eq!(
        body.bounds(),
        Rect::new(0.0, NODE_BOTTOM_PAD, NODE_WIDTH, BODY_H)
    );
    // No simplified NodeWidget is built for a hosted node.
    assert!(f
        .editor
        .children()
        .iter()
        .all(|c| c.type_name() != "NodeWidget"));
}

#[test]
fn a_hosted_body_gets_a_click_at_the_right_place_under_pan_and_zoom() {
    let mut f = fixture();
    f.editor.canvas_offset = [37.0, -21.0];
    f.editor.canvas_scale = 1.5;
    f.editor.layout(VIEW);
    // Body-local (20, 10) is canvas (120, card bottom + PAD + 10).
    let canvas = [120.0, 400.0 - card_height(BODY_H) + NODE_BOTTOM_PAD + 10.0];
    let screen = Point::new(canvas[0] * 1.5 + 37.0, canvas[1] * 1.5 - 21.0);
    let r = dispatch(&mut f.editor, press(screen), screen);
    assert!(r.is_consumed());
    let clicks = f.clicks.borrow();
    assert_eq!(clicks.len(), 1, "the body received the press");
    assert!(
        (clicks[0].x - 20.0).abs() < 1e-9 && (clicks[0].y - 10.0).abs() < 1e-9,
        "body-local press was {:?}",
        clicks[0]
    );
}

#[test]
fn a_click_on_empty_canvas_does_not_reach_the_body() {
    let mut f = fixture();
    let p = Point::new(600.0, 100.0);
    dispatch(&mut f.editor, press(p), p);
    assert!(f.clicks.borrow().is_empty());
}

#[test]
fn bodies_are_kept_until_the_body_epoch_changes() {
    let mut f = fixture();
    assert_eq!(f.builds.get(), 1);
    // Pan, select and move: the paint fingerprint changes, the body stays.
    f.editor.canvas_offset = [10.0, 5.0];
    f.editor.selected.insert(NodeId(1));
    f.model.lock().unwrap().inner.nodes[0].position = [150.0, 420.0];
    f.editor.layout(VIEW);
    f.editor.layout(VIEW);
    assert_eq!(f.builds.get(), 1);
    f.model.lock().unwrap().epoch = 1;
    f.editor.layout(VIEW);
    assert_eq!(f.builds.get(), 2);
    f.editor.layout(VIEW);
    assert_eq!(f.builds.get(), 2);
}

#[test]
fn the_card_resizes_to_its_measured_height_and_reports_it() {
    let mut f = fixture();
    let first = card_height(BODY_H);
    assert_eq!(f.model.lock().unwrap().measured, vec![(NodeId(1), first)]);
    f.height.set(90.0);
    f.editor.layout(VIEW);
    let grown = card_height(90.0);
    let b = card(&f.editor).bounds();
    assert_eq!(
        (b.height, b.y + b.height),
        (grown, 400.0),
        "the top edge stays put"
    );
    f.editor.layout(VIEW);
    let measured = f.model.lock().unwrap().measured.clone();
    assert_eq!(measured, vec![(NodeId(1), first), (NodeId(1), grown)]);
    // Hit-testing follows the card's measured size.
    let layouts = f.editor.snapshot_layouts();
    assert_eq!(layouts[0].size, [NODE_WIDTH, grown]);
}

#[test]
fn sockets_follow_the_bodys_anchor() {
    let f = fixture();
    let layouts = f.editor.snapshot_layouts();
    let width = layouts[0].sockets().find(|s| s.name == "Width").unwrap();
    assert_eq!(width.center, [100.0, 400.0 - TITLE_HEIGHT - 30.0]);
    // An unanchored socket stacks: the first output sits half a row down.
    let result = layouts[0].sockets().find(|s| s.name == "Result").unwrap();
    let row = crate::draw::ROW_HEIGHT;
    assert_eq!(
        result.center,
        [100.0 + NODE_WIDTH, 400.0 - TITLE_HEIGHT - row * 0.5]
    );
}

#[test]
fn a_press_on_a_socket_over_the_body_starts_a_noodle_not_a_body_click() {
    let mut f = fixture();
    // The Width socket's inner half lies over the body widget.
    let p = Point::new(100.0 + 2.0, 400.0 - TITLE_HEIGHT - 30.0);
    assert!(dispatch(&mut f.editor, press(p), p).is_consumed());
    assert!(f.clicks.borrow().is_empty());
    assert!(matches!(
        f.editor.interaction,
        CanvasState::DrawingConnection { .. }
    ));
}

#[test]
fn the_width_comes_from_the_model_and_the_right_edge_drags_it() {
    let mut f = fixture();
    f.model.lock().unwrap().widths.push((NodeId(1), 260.0));
    f.editor.layout(VIEW);
    assert_eq!(card(&f.editor).bounds().width, 260.0);
    // Press inside the right-edge band, below the title, then drag 40.
    let b = card(&f.editor).bounds();
    let p = Point::new(b.x + b.width - 2.0, b.y + 20.0);
    assert!(dispatch(&mut f.editor, press(p), p).is_consumed());
    assert!(
        f.clicks.borrow().is_empty(),
        "the band belongs to the editor"
    );
    let to = Point::new(p.x + 40.0, p.y);
    f.editor.on_event(&Event::MouseMove { pos: to });
    f.editor.on_event(&release(to));
    assert_eq!(f.model.lock().unwrap().node_width(NodeId(1)), Some(300.0));
    f.editor.layout(VIEW);
    assert_eq!(card(&f.editor).bounds().width, 300.0);
}

#[test]
fn the_pressed_card_paints_on_top() {
    let mut f = fixture();
    f.model
        .lock()
        .unwrap()
        .inner
        .nodes
        .push(node_with_sockets(2, [400.0, 400.0]));
    f.editor.layout(VIEW);
    assert_eq!(f.editor.hosted_card_order(), &[NodeId(1), NodeId(2)]);
    // A press on card 1's title bar (not the body) raises and selects it.
    let p = Point::new(150.0, 400.0 - TITLE_HEIGHT * 0.5);
    dispatch(&mut f.editor, press(p), p);
    f.editor.on_event(&release(p));
    f.editor.layout(VIEW);
    assert_eq!(f.editor.hosted_card_order(), &[NodeId(2), NodeId(1)]);
    assert!(f.editor.selected_ids().contains(&NodeId(1)));
    assert!(card_by_id(&f.editor, NodeId(1)).chrome.selected);
    let layer = f.editor.children().last().unwrap();
    let top = layer.children().last().unwrap().as_any().unwrap();
    assert_eq!(
        top.downcast_ref::<HostedCard>().unwrap().node_id(),
        NodeId(1)
    );
}

fn card_by_id(editor: &NodeEditor, id: NodeId) -> &HostedCard {
    let layer = editor.children().last().unwrap();
    layer
        .children()
        .iter()
        .filter_map(|c| c.as_any()?.downcast_ref::<HostedCard>())
        .find(|c| c.node_id() == id)
        .unwrap()
}

#[test]
fn a_factory_that_declines_keeps_the_simplified_card() {
    let (model, memory) = super::tests_common::fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model).with_body_factory(|_n: &NodeView| None);
    memory.lock().unwrap().nodes = vec![mk_node(1, "Box", [10.0, 200.0])];
    editor.layout(VIEW);
    assert_eq!(editor.children()[0].type_name(), "NodeWidget");
}

#[test]
fn without_a_factory_the_editor_builds_no_hosted_layer() {
    let (model, memory) = super::tests_common::fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    memory.lock().unwrap().nodes = vec![mk_node(1, "Box", [10.0, 200.0])];
    editor.layout(VIEW);
    assert_eq!(editor.children().len(), 1);
    assert_eq!(editor.children()[0].type_name(), "NodeWidget");
}

#[test]
fn collapse_can_be_turned_off() {
    let (model, memory) = super::tests_common::fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model).with_collapse_enabled(false);
    memory.lock().unwrap().nodes = vec![mk_node(1, "Box", [10.0, 200.0])];
    editor.layout(VIEW);
    editor.toggle_collapsed(NodeId(1));
    assert!(editor.collapsed_nodes.is_empty());
}
