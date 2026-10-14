//! Tests for resizing hosted cards by their edges (`hosted_resize.rs`,
//! `hosted_events.rs`): the bands sit outside the drawn card, as
//! agg-sharp's `WindowWidget` puts its `GrabControl`s in the border around
//! its panel, so a press just inside the drawn edge reaches the body's
//! controls and one just outside resizes. A card that opts in through
//! `NodeGraphModel::node_vertical_resize` also gets top and bottom bands,
//! each moving only its own edge (MatterCAD's
//! `NodeWindowGrowsDownTests.DraggingAnEdgeMovesOnlyThatEdge`).

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::{DrawCtx, Event, EventResult, Point, Rect, Size};

use super::hosted::HostedNodeBody;
use super::hosted_card::HostedCard;
use super::hosted_resize::RESIZE_GRIP;
use super::tests_common::{mk_node, Memory};
use super::tests_hosted::{dispatch, press, release};
use super::*;
use crate::draw::{NODE_BOTTOM_PAD, NODE_WIDTH, TITLE_HEIGHT};
use crate::model::{NodeTypeView, NodeView, NoodleResult, NoodleView, PropertyValue};

const VIEW: Size = Size {
    width: 800.0,
    height: 600.0,
};
const START_HEIGHT: f64 = 120.0;
const MIN_HEIGHT: f64 = 80.0;

/// A host that keeps every card's width and height, as MatterCAD keeps a
/// node window's saved size, and answers the body's height from it.
struct SizingModel {
    inner: Memory,
    widths: Vec<(NodeId, f64)>,
    heights: Vec<(NodeId, f64)>,
    /// `set_node_height` calls, in order: `(node, height, top)`.
    height_calls: Vec<(NodeId, f64, f64)>,
    /// What `node_vertical_resize` answers for every node.
    vertical: Option<f64>,
}

impl SizingModel {
    fn height_of(&self, id: NodeId) -> f64 {
        self.heights
            .iter()
            .rev()
            .find(|(n, _)| *n == id)
            .map_or(START_HEIGHT, |(_, h)| *h)
    }
}

impl NodeGraphModel for SizingModel {
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
    fn node_vertical_resize(&self, _id: NodeId) -> Option<f64> {
        self.vertical
    }
    fn set_node_height(&mut self, id: NodeId, height: f64, top: f64) {
        self.height_calls.push((id, height, top));
        self.heights.push((id, height));
        if let Some(n) = self.inner.nodes.iter_mut().find(|n| n.id == id) {
            n.position[1] = top;
        }
    }
}

/// A body filling its card: its height is what the host saved for the
/// card, and it records (and consumes) every press it gets.
struct SizedBody {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    id: NodeId,
    model: Arc<Mutex<SizingModel>>,
    clicks: Rc<RefCell<Vec<(NodeId, Point)>>>,
}

impl Widget for SizedBody {
    fn type_name(&self) -> &'static str {
        "SizedBody"
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
        self.model.lock().unwrap().height_of(self.id) - TITLE_HEIGHT - NODE_BOTTOM_PAD
    }
    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, self.measure_min_height(available.width))
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::MouseDown { pos, .. } = event {
            self.clicks.borrow_mut().push((self.id, *pos));
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
}

struct Fixture {
    editor: NodeEditor,
    model: Arc<Mutex<SizingModel>>,
    clicks: Rc<RefCell<Vec<(NodeId, Point)>>>,
}

/// Nodes at the given canvas top-lefts, each a hosted [`SizedBody`] card
/// `START_HEIGHT` tall; `vertical` is the host's `node_vertical_resize`.
fn fixture(at: &[[f64; 2]], vertical: Option<f64>) -> Fixture {
    let model = Arc::new(Mutex::new(SizingModel {
        inner: Memory::default(),
        widths: Vec::new(),
        heights: Vec::new(),
        height_calls: Vec::new(),
        vertical,
    }));
    model.lock().unwrap().inner.nodes = at
        .iter()
        .enumerate()
        .map(|(i, p)| mk_node(i as u64 + 1, "Output", *p))
        .collect();
    let shared: SharedModel = model.clone();
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let (m, c) = (model.clone(), clicks.clone());
    let editor = NodeEditor::new(shared).with_body_factory(move |n: &NodeView| {
        let body = SizedBody {
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            children: Vec::new(),
            id: n.id,
            model: m.clone(),
            clicks: c.clone(),
        };
        Some(HostedNodeBody::new(Box::new(body)))
    });
    let mut f = Fixture {
        editor,
        model,
        clicks,
    };
    f.editor.layout(VIEW);
    f
}

/// The canvas-space rect of node `id`'s card.
fn card_rect(editor: &NodeEditor, id: u64) -> Rect {
    super::tests_hosted::card_by_id(editor, NodeId(id)).bounds()
}

/// Editor-local (screen) point of a canvas point.
fn screen(editor: &NodeEditor, canvas: [f64; 2]) -> Point {
    let s = editor.canvas_scale;
    Point::new(
        canvas[0] * s + editor.canvas_offset[0],
        canvas[1] * s + editor.canvas_offset[1],
    )
}

/// Press at `from`, move by `by` (screen units) and release, through the
/// framework's dispatch as the App routes them. Returns the press result.
fn drag(editor: &mut NodeEditor, from: Point, by: [f64; 2]) -> EventResult {
    let r = dispatch(editor, press(from), from);
    let to = Point::new(from.x + by[0], from.y + by[1]);
    editor.on_event(&Event::MouseMove { pos: to });
    editor.on_event(&release(to));
    editor.layout(VIEW);
    r
}

#[test]
fn a_click_just_inside_the_drawn_right_edge_reaches_the_body() {
    // MatterCAD at 2x and zoom 0.5: a Width field reaching close to the
    // card's edge takes a click 2 px inside its drawn right end.
    for scale in [1.0, 0.5] {
        let mut f = fixture(&[[100.0, 400.0]], None);
        f.editor.canvas_scale = scale;
        f.editor.layout(VIEW);
        let b = card_rect(&f.editor, 1);
        let edge = screen(&f.editor, [b.x + b.width, b.y + 20.0]);
        let p = Point::new(edge.x - 2.0, edge.y);
        let r = dispatch(&mut f.editor, press(p), p);
        assert!(r.is_consumed());
        assert_eq!(f.clicks.borrow().len(), 1, "the body took the press");
        assert!(!f.editor.hosted_resizing(), "no resize at zoom {scale}");
        f.editor.on_event(&release(p));
    }
}

#[test]
fn a_press_just_outside_the_right_edge_starts_a_resize() {
    for scale in [1.0, 0.5] {
        let mut f = fixture(&[[100.0, 400.0]], None);
        f.editor.canvas_scale = scale;
        f.editor.layout(VIEW);
        let b = card_rect(&f.editor, 1);
        let edge = screen(&f.editor, [b.x + b.width, b.y + 20.0]);
        // 2 screen px outside is inside the band at either zoom.
        let p = Point::new(edge.x + 2.0, edge.y);
        assert!(dispatch(&mut f.editor, press(p), p).is_consumed());
        assert!(f.editor.hosted_resizing(), "a resize at zoom {scale}");
        let to = Point::new(p.x + 20.0, p.y);
        f.editor.on_event(&Event::MouseMove { pos: to });
        f.editor.on_event(&release(to));
        assert!(f.clicks.borrow().is_empty(), "the band is the editor's");
        let width = f.model.lock().unwrap().node_width(NodeId(1));
        assert_eq!(width, Some(NODE_WIDTH + 20.0 / scale), "zoom {scale}");
    }
}

#[test]
fn a_press_beyond_the_band_does_not_resize() {
    let mut f = fixture(&[[100.0, 400.0]], None);
    let b = card_rect(&f.editor, 1);
    let p = screen(&f.editor, [b.x + b.width + RESIZE_GRIP + 1.0, b.y + 20.0]);
    dispatch(&mut f.editor, press(p), p);
    assert!(!f.editor.hosted_resizing());
    f.editor.on_event(&release(p));
    assert!(f.model.lock().unwrap().widths.is_empty());
}

#[test]
fn dragging_an_edge_moves_only_that_edge() {
    let mut f = fixture(&[[100.0, 400.0]], Some(MIN_HEIGHT));
    let before = card_rect(&f.editor, 1);
    assert_eq!(before.height, START_HEIGHT);

    // The bottom band, 2 units below the drawn bottom: down 40.
    let p = screen(&f.editor, [before.x + 50.0, before.y - 2.0]);
    assert!(drag(&mut f.editor, p, [0.0, -40.0]).is_consumed());
    let after = card_rect(&f.editor, 1);
    assert_eq!(after.y + after.height, 400.0, "the top stays");
    assert_eq!(
        after.y,
        before.y - 40.0,
        "the bottom follows the mouse down"
    );

    // The top band, 2 units above the drawn top: up 40.
    let p = screen(&f.editor, [after.x + 50.0, after.y + after.height + 2.0]);
    assert!(drag(&mut f.editor, p, [0.0, 40.0]).is_consumed());
    let after_top = card_rect(&f.editor, 1);
    assert_eq!(
        after_top.y, after.y,
        "dragging the top edge up leaves the bottom"
    );
    assert_eq!(
        after_top.y + after_top.height,
        440.0,
        "the top follows the mouse up"
    );
    assert_eq!(
        f.model.lock().unwrap().height_calls.last(),
        Some(&(NodeId(1), START_HEIGHT + 80.0, 440.0))
    );
    assert!(f.clicks.borrow().is_empty(), "the bands are the editor's");
}

#[test]
fn a_top_drag_stops_at_the_minimum_height_and_keeps_the_bottom() {
    let mut f = fixture(&[[100.0, 400.0]], Some(MIN_HEIGHT));
    let before = card_rect(&f.editor, 1);
    let p = screen(&f.editor, [before.x + 50.0, before.y + before.height + 2.0]);
    drag(&mut f.editor, p, [0.0, -200.0]);
    let after = card_rect(&f.editor, 1);
    assert_eq!(after.height, MIN_HEIGHT);
    assert_eq!(after.y, before.y, "the bottom stays put");
}

#[test]
fn vertical_drags_follow_the_mouse_under_zoom() {
    let mut f = fixture(&[[100.0, 400.0]], Some(MIN_HEIGHT));
    f.editor.canvas_scale = 0.5;
    f.editor.layout(VIEW);
    let before = card_rect(&f.editor, 1);
    let bottom = screen(&f.editor, [before.x + 50.0, before.y]);
    drag(
        &mut f.editor,
        Point::new(bottom.x, bottom.y - 2.0),
        [0.0, -20.0],
    );
    let after = card_rect(&f.editor, 1);
    assert_eq!(after.height, START_HEIGHT + 40.0, "20 px at zoom 0.5");
    assert_eq!(after.y + after.height, 400.0);
}

#[test]
fn a_card_that_fits_its_content_has_no_vertical_handles() {
    let mut f = fixture(&[[100.0, 400.0]], None);
    let b = card_rect(&f.editor, 1);
    for canvas in [[b.x + 50.0, b.y - 2.0], [b.x + 50.0, b.y + b.height + 2.0]] {
        let p = screen(&f.editor, canvas);
        dispatch(&mut f.editor, press(p), p);
        assert!(!f.editor.hosted_resizing());
        f.editor.on_event(&release(p));
    }
    assert!(f.model.lock().unwrap().height_calls.is_empty());
    assert_eq!(card_rect(&f.editor, 1), b);
}

#[test]
fn an_upper_cards_band_over_a_lower_cards_body_resizes_the_upper_card() {
    // Card 2 (later in model order, so on top) ends 1 unit left of card
    // 1's left edge, so its right band reaches over card 1's body.
    let mut f = fixture(&[[100.0, 400.0], [100.0 - NODE_WIDTH - 1.0, 400.0]], None);
    let lower = card_rect(&f.editor, 1);
    let upper = card_rect(&f.editor, 2);
    let x = upper.x + upper.width + 3.0;
    assert!(x > lower.x && x < lower.x + RESIZE_GRIP, "inside card 1");
    let p = screen(&f.editor, [x, lower.y + 10.0]);
    drag(&mut f.editor, p, [10.0, 0.0]);
    assert!(f.clicks.borrow().is_empty(), "card 1's body got no press");
    let widths = f.model.lock().unwrap().widths.clone();
    assert_eq!(widths, vec![(NodeId(2), NODE_WIDTH + 10.0)]);
}

#[test]
fn hosted_card_hit_test_leaves_the_band_to_the_editor() {
    let f = fixture(&[[100.0, 400.0]], Some(MIN_HEIGHT));
    let b = card_rect(&f.editor, 1);
    let layer = f.editor.children().last().expect("the hosted layer");
    let card = layer.children()[0]
        .as_any()
        .and_then(|a| a.downcast_ref::<HostedCard>())
        .expect("a hosted card");
    // Card-local: the band is outside the card, so the card does not
    // claim it and the layer does not hit-test there.
    assert!(!card.claims_pointer_exclusively(Point::new(b.width - 2.0, 20.0)));
    assert!(!layer.hit_test(Point::new(b.x + b.width + 2.0, b.y + 20.0)));
    assert!(!layer.hit_test(Point::new(b.x + 50.0, b.y - 2.0)));
    assert!(layer.hit_test(Point::new(b.x + b.width - 2.0, b.y + 20.0)));
}
