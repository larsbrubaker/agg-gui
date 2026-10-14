//! Tests for the hosted cards' host settings `with_card_title_height` and
//! `with_min_card_width` (`hosted.rs`): MatterCAD's node windows have a
//! 20 unit title bar (`TitleBar.Height = 20 * DeviceScale`) and go down to
//! a drawn width of 30 (agg-sharp `WindowWidget`'s 40 unit minimum less
//! its 5 unit grab border on each side). Everything that derives from the
//! title height follows it: socket anchors, the body's place, the reported
//! height and the title-bar double-click; the least width bounds both the
//! layout and a right-edge drag.

use agg_gui::{DrawCtx, Event, EventResult, Point, Rect, Size};

use super::hosted::{HostedNodeBody, SocketAnchor, MIN_HOSTED_CARD_WIDTH};
use super::hosted_card::HostedCard;
use super::tests_common::Memory;
use super::tests_hosted::{dispatch, node_with_sockets, press, release};
use super::*;
use crate::draw::{SocketSide, NODE_BOTTOM_PAD, NODE_WIDTH, TITLE_HEIGHT};
use crate::model::{NodeTypeView, NodeView, NoodleResult, NoodleView, PropertyValue};

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};
const BODY_H: f64 = 50.0;
const TOP: f64 = 400.0;
const LEFT: f64 = 100.0;

/// A host that keeps card widths and records measured heights and
/// activations; the rest goes to `Memory`.
#[derive(Default)]
struct TitleModel {
    inner: Memory,
    widths: Vec<(NodeId, f64)>,
    measured: Vec<(NodeId, f64)>,
}

impl NodeGraphModel for TitleModel {
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
    fn on_node_activated(&mut self, id: NodeId) -> bool {
        self.inner.on_node_activated(id)
    }
}

/// A `BODY_H` tall body that ignores every event, so a press anywhere on
/// the card reaches the editor.
struct PlainBody {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for PlainBody {
    fn type_name(&self) -> &'static str {
        "PlainBody"
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
        BODY_H
    }
    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, BODY_H)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

struct Fixture {
    editor: NodeEditor,
    model: Arc<Mutex<TitleModel>>,
}

/// One node at canvas top-left `(LEFT, TOP)` with a hosted [`PlainBody`].
/// Its `Result` output sits on the title bar (anchored half of `title`
/// above the body's top, as MatterCAD's host anchors title-bar sockets),
/// its `Width` input 30 units below the body's top. `configure` sets the
/// host settings under test.
fn fixture(title: f64, configure: impl FnOnce(NodeEditor) -> NodeEditor) -> Fixture {
    let model = Arc::new(Mutex::new(TitleModel::default()));
    model.lock().unwrap().inner.nodes = vec![node_with_sockets(1, [LEFT, TOP])];
    let shared: SharedModel = model.clone();
    let editor = NodeEditor::new(shared).with_body_factory(move |_n: &NodeView| {
        let body = PlainBody {
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            children: Vec::new(),
        };
        Some(HostedNodeBody::new(Box::new(body)).with_socket_anchor(
            move |_w: &dyn Widget, name: &str| -> Option<SocketAnchor> {
                match name {
                    "Result" => Some((SocketSide::Output, -title / 2.0)),
                    "Width" => Some((SocketSide::Input, 30.0)),
                    _ => None,
                }
            },
        ))
    });
    let mut f = Fixture {
        editor: configure(editor),
        model,
    };
    f.editor.layout(VIEW);
    f
}

fn card(editor: &NodeEditor) -> &HostedCard {
    super::tests_hosted::card_by_id(editor, NodeId(1))
}

/// Editor-local (screen) point of a canvas point.
fn screen(editor: &NodeEditor, canvas: [f64; 2]) -> Point {
    let s = editor.canvas_scale;
    Point::new(
        canvas[0] * s + editor.canvas_offset[0],
        canvas[1] * s + editor.canvas_offset[1],
    )
}

#[test]
fn the_defaults_are_unchanged() {
    let f = fixture(TITLE_HEIGHT, |e| e);
    assert_eq!(f.editor.card_title_height(), TITLE_HEIGHT);
    assert_eq!(TITLE_HEIGHT, 26.0);
    assert_eq!(f.editor.min_card_width(), MIN_HOSTED_CARD_WIDTH);
    assert_eq!(MIN_HOSTED_CARD_WIDTH, 80.0);
    let height = TITLE_HEIGHT + BODY_H + NODE_BOTTOM_PAD;
    assert_eq!(
        card(&f.editor).bounds(),
        Rect::new(LEFT, TOP - height, NODE_WIDTH, height)
    );
    let layouts = f.editor.snapshot_layouts();
    let result = layouts[0].sockets().find(|s| s.name == "Result").unwrap();
    assert_eq!(result.center, [LEFT + NODE_WIDTH, TOP - 13.0]);
    // A model width under the default minimum is laid out at 80.
    f.model.lock().unwrap().widths.push((NodeId(1), 10.0));
    let mut f = f;
    f.editor.layout(VIEW);
    assert_eq!(card(&f.editor).bounds().width, 80.0);
}

#[test]
fn a_title_height_of_20_puts_title_bar_sockets_10_below_the_card_top() {
    for scale in [1.0, 0.5] {
        let mut f = fixture(20.0, |e| e.with_card_title_height(20.0));
        f.editor.canvas_scale = scale;
        f.editor.canvas_offset = [15.0, -10.0];
        f.editor.layout(VIEW);
        let layouts = f.editor.snapshot_layouts();
        let result = layouts[0].sockets().find(|s| s.name == "Result").unwrap();
        assert_eq!(
            result.center,
            [LEFT + NODE_WIDTH, TOP - 10.0],
            "zoom {scale}"
        );
        let width = layouts[0].sockets().find(|s| s.name == "Width").unwrap();
        assert_eq!(width.center, [LEFT, TOP - 20.0 - 30.0], "zoom {scale}");
        // The card paints the socket at the same place.
        let painted: Vec<f64> = card(&f.editor)
            .chrome
            .sockets
            .iter()
            .map(|s| s.from_top)
            .collect();
        assert_eq!(painted, vec![10.0, 50.0], "zoom {scale}");
        // A press there on screen starts a noodle from it.
        let p = screen(&f.editor, [LEFT + NODE_WIDTH, TOP - 10.0]);
        assert!(dispatch(&mut f.editor, press(p), p).is_consumed());
        assert!(
            matches!(f.editor.interaction, CanvasState::DrawingConnection { .. }),
            "zoom {scale}"
        );
        f.editor.on_event(&release(p));
    }
}

#[test]
fn the_body_starts_20_below_the_card_top() {
    let f = fixture(20.0, |e| e.with_card_title_height(20.0));
    let c = card(&f.editor);
    let height = 20.0 + BODY_H + NODE_BOTTOM_PAD;
    assert_eq!(
        c.bounds(),
        Rect::new(LEFT, TOP - height, NODE_WIDTH, height)
    );
    let body = c.children()[0].bounds();
    assert_eq!(body, Rect::new(0.0, NODE_BOTTOM_PAD, NODE_WIDTH, BODY_H));
    assert_eq!(
        height - (body.y + body.height),
        20.0,
        "body top to card top"
    );
}

#[test]
fn the_reported_height_follows_the_title_height() {
    let mut f = fixture(20.0, |e| e.with_card_title_height(20.0));
    let h20 = 20.0 + BODY_H + NODE_BOTTOM_PAD;
    assert_eq!(f.model.lock().unwrap().measured, vec![(NodeId(1), h20)]);
    assert_eq!(f.editor.snapshot_layouts()[0].size, [NODE_WIDTH, h20]);
    // Changed on a live editor, the next layout reports the new height.
    f.editor.set_card_title_height(30.0);
    assert_eq!(f.editor.card_title_height(), 30.0);
    f.editor.layout(VIEW);
    let h30 = 30.0 + BODY_H + NODE_BOTTOM_PAD;
    let measured = f.model.lock().unwrap().measured.clone();
    assert_eq!(measured, vec![(NodeId(1), h20), (NodeId(1), h30)]);
    assert_eq!(card(&f.editor).bounds().height, h30);
}

/// Double-click canvas point `at` (press, release, press, release).
fn double_click(editor: &mut NodeEditor, at: [f64; 2]) {
    let p = screen(editor, at);
    for _ in 0..2 {
        dispatch(editor, press(p), p);
        editor.on_event(&release(p));
    }
}

#[test]
fn the_title_bar_double_click_follows_the_title_height() {
    let mut f = fixture(20.0, |e| e.with_card_title_height(20.0));
    // 23 below the top: in a 26 unit title bar, under a 20 unit one.
    double_click(&mut f.editor, [LEFT + 40.0, TOP - 23.0]);
    assert!(f.model.lock().unwrap().inner.activated.is_empty());
    f.editor.layout(VIEW);
    double_click(&mut f.editor, [LEFT + 40.0, TOP - 15.0]);
    assert_eq!(f.model.lock().unwrap().inner.activated, vec![NodeId(1)]);
}

/// Press the right band 2 units outside the drawn edge and drag by `dx`
/// screen units.
fn drag_right_edge(f: &mut Fixture, dx: f64) {
    let b = card(&f.editor).bounds();
    let p = screen(&f.editor, [b.x + b.width + 2.0, b.y + 20.0]);
    assert!(dispatch(&mut f.editor, press(p), p).is_consumed());
    assert!(f.editor.hosted_resizing());
    let to = Point::new(p.x + dx, p.y);
    f.editor.on_event(&Event::MouseMove { pos: to });
    f.editor.on_event(&release(to));
    f.editor.layout(VIEW);
}

#[test]
fn a_min_width_of_30_lets_a_right_edge_drag_shrink_the_card_to_30_and_no_further() {
    let mut f = fixture(TITLE_HEIGHT, |e| e.with_min_card_width(30.0));
    assert_eq!(f.editor.min_card_width(), 30.0);
    // Exactly to 30.
    drag_right_edge(&mut f, 30.0 - NODE_WIDTH);
    assert_eq!(f.model.lock().unwrap().node_width(NodeId(1)), Some(30.0));
    assert_eq!(card(&f.editor).bounds().width, 30.0);
    // Past it: stops at 30.
    drag_right_edge(&mut f, -50.0);
    assert_eq!(f.model.lock().unwrap().node_width(NodeId(1)), Some(30.0));
    assert_eq!(card(&f.editor).bounds().width, 30.0);
    // A model width under the minimum is laid out at it.
    f.model.lock().unwrap().widths.push((NodeId(1), 10.0));
    f.editor.layout(VIEW);
    assert_eq!(card(&f.editor).bounds().width, 30.0);
}

#[test]
fn the_default_min_width_stops_a_right_edge_drag_at_80() {
    let mut f = fixture(TITLE_HEIGHT, |e| e);
    drag_right_edge(&mut f, -NODE_WIDTH);
    assert_eq!(f.model.lock().unwrap().node_width(NodeId(1)), Some(80.0));
    assert_eq!(card(&f.editor).bounds().width, 80.0);
}
