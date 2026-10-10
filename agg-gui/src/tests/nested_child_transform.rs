//! Screen placement of widgets nested more than one level under a widget
//! with a scaling `child_transform` (NodeEditor's hosted cards: a card
//! positioned on the zoomed canvas, a row inside the card). A child's own
//! bounds offset is in its parent's transformed space, so it must be
//! scaled with it: `find_widget_screen_rect` and the inspector snapshot
//! must report the grandchild where it is painted (and where pointer
//! events reach it), not with its parent's offset left unscaled.

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::widget::{collect_inspector_nodes, find_widget_screen_rect, Widget};
use crate::TransAffine;

/// A canvas whose children paint at `local * scale + offset`.
struct Canvas {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    scale: f64,
    offset: [f64; 2],
}

impl Widget for Canvas {
    fn type_name(&self) -> &'static str {
        "Canvas"
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
    fn layout(&mut self, available: Size) -> Size {
        available
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn child_transform(&self) -> Option<TransAffine> {
        let mut t = TransAffine::new_scaling_uniform(self.scale);
        t.translate(self.offset[0], self.offset[1]);
        Some(t)
    }
}

struct Node {
    id: Option<&'static str>,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Node {
    fn type_name(&self) -> &'static str {
        "Node"
    }
    fn id(&self) -> Option<&str> {
        self.id
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
    fn layout(&mut self, _: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Canvas at (100, 50), zoom 0.8, pan (10, 20); a card at canvas (200,
/// 100) holding a row at card-local (5, 30), 40 x 10.
fn scene() -> Canvas {
    let row = Node {
        id: Some("row"),
        bounds: Rect::new(5.0, 30.0, 40.0, 10.0),
        children: vec![],
    };
    let card = Node {
        id: Some("card"),
        bounds: Rect::new(200.0, 100.0, 120.0, 80.0),
        children: vec![Box::new(row)],
    };
    Canvas {
        bounds: Rect::new(100.0, 50.0, 800.0, 600.0),
        children: vec![Box::new(card)],
        scale: 0.8,
        offset: [10.0, 20.0],
    }
}

/// The row is painted at canvas (205, 130): screen x = 100 + 10 + 205 *
/// 0.8 = 274, y = 50 + 20 + 130 * 0.8 = 174, 32 x 8.
fn assert_row_rect(r: Rect, what: &str) {
    assert!(
        (r.x - 274.0).abs() < 1e-9
            && (r.y - 174.0).abs() < 1e-9
            && (r.width - 32.0).abs() < 1e-9
            && (r.height - 8.0).abs() < 1e-9,
        "{what}: a row inside a card on a zoomed canvas must be placed through the zoom, got {r:?}"
    );
}

#[test]
fn find_widget_screen_rect_scales_a_nested_childs_offset() {
    let canvas = scene();
    let r = find_widget_screen_rect(&canvas, "row").expect("the row");
    assert_row_rect(r, "find_widget_screen_rect");
}

#[test]
fn inspector_snapshot_scales_a_nested_childs_offset() {
    let canvas = scene();
    let mut nodes = Vec::new();
    collect_inspector_nodes(&canvas, 0, Point::ORIGIN, &mut nodes);
    let row = nodes
        .iter()
        .find(|n| n.type_name == "Node" && n.depth == 2)
        .expect("the row's inspector node");
    assert_row_rect(row.screen_bounds, "collect_inspector_nodes");
}
