//! The paint-order walk: `walk_painted` visits widgets in the order painting
//! paints them, each placed where painting puts it, so a host can tell what
//! is drawn on top of what. Every widget's visible rect from the walk is the
//! one `find_widget_visible_rect` gives, through a scrolled ScrollView, a
//! row's column clip, a zoomed canvas and the overlay pass, and hidden and
//! faded subtrees are never visited. `painted_over` (in `painted_over`)
//! builds on it.

mod painted_over;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::widget::{find_widget_visible_rect, walk_painted, PaintWalk, Widget};
use crate::widgets::{FlexRow, Opacity, ScrollView};
use crate::TransAffine;
use std::cell::Cell;
use std::rc::Rc;

/// A plain widget at fixed bounds: an optional id, its children clipped to
/// its bounds or free to spill past them, an optional zoom of its children,
/// a move to the overlay pass or on top of its siblings, and a visibility
/// switch.
pub(super) struct Node {
    pub id: Option<&'static str>,
    pub bounds: Rect,
    pub children: Vec<Box<dyn Widget>>,
    pub clips: bool,
    pub overlay: bool,
    pub on_top: bool,
    pub visible: bool,
    pub zoom: Option<TransAffine>,
}

impl Node {
    pub fn new(id: Option<&'static str>, bounds: Rect) -> Self {
        Self {
            id,
            bounds,
            children: Vec::new(),
            clips: true,
            overlay: false,
            on_top: false,
            visible: true,
            zoom: None,
        }
    }
    pub fn child(mut self, c: impl Widget + 'static) -> Self {
        self.children.push(Box::new(c));
        self
    }
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
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn clip_children_rect(&self) -> Option<(f64, f64, f64, f64)> {
        // Painting clips children to a widget's bounds unless it names a
        // wider rect; a non-clipping node names one far past any test tree.
        Some(if self.clips {
            (0.0, 0.0, self.bounds.width, self.bounds.height)
        } else {
            (-1.0e4, -1.0e4, 2.0e4, 2.0e4)
        })
    }
    fn child_transform(&self) -> Option<TransAffine> {
        self.zoom
    }
    fn defer_paint_to_overlay(&self) -> bool {
        self.overlay
    }
    fn draw_on_top_of_siblings(&self) -> bool {
        self.on_top
    }
    fn is_visible(&self) -> bool {
        self.visible
    }
}

/// Every id in [`fixture`], hidden and faded ones included.
const IDS: &[&str] = &[
    "root",
    "top",
    "half",
    "far",
    "cell",
    "spill",
    "canvas",
    "card",
    "card label",
    "clipper",
    "in place",
    "modal",
    "modal button",
    "hidden",
    "hidden child",
    "faded",
    "raised",
    "after raised",
];

/// A 400 × 300 window holding each rule the walk replays: a ScrollView
/// scrolled part way, a row clipping a spill to its column, a canvas zoomed
/// 2× and panned, a clipping box with a child in place outside it and a
/// child painted in the overlay pass, a hidden subtree, a faded-out one, and
/// a child drawn on top of its later sibling.
pub(super) fn fixture() -> Node {
    let content = Node::new(None, Rect::new(0.0, 0.0, 150.0, 600.0))
        .child(Node::new(Some("top"), Rect::new(10.0, 500.0, 120.0, 100.0)))
        .child(Node::new(
            Some("half"),
            Rect::new(10.0, 450.0, 120.0, 100.0),
        ))
        .child(Node::new(Some("far"), Rect::new(10.0, 0.0, 120.0, 100.0)));
    let mut view = ScrollView::new(Box::new(content));
    view.set_scroll_offset(0.0);
    view.layout(Size::new(200.0, 100.0));
    view.set_bounds(Rect::new(50.0, 180.0, 200.0, 100.0));

    let mut cell = Node::new(Some("cell"), Rect::new(0.0, 0.0, 50.0, 40.0))
        .child(Node::new(Some("spill"), Rect::new(30.0, 10.0, 60.0, 20.0)));
    cell.clips = false;
    let mut row = FlexRow::new()
        .with_clip_children_to_slots(true)
        .add(Box::new(cell));
    row.layout(Size::new(120.0, 40.0));
    row.set_bounds(Rect::new(260.0, 240.0, 120.0, 40.0));

    let mut canvas = Node::new(Some("canvas"), Rect::new(260.0, 100.0, 120.0, 120.0)).child(
        Node::new(Some("card"), Rect::new(10.0, 10.0, 40.0, 30.0)).child(Node::new(
            Some("card label"),
            Rect::new(5.0, 5.0, 20.0, 10.0),
        )),
    );
    let mut zoom = TransAffine::new_scaling_uniform(2.0);
    zoom.translate(-15.0, 7.5);
    canvas.zoom = Some(zoom);

    let mut modal = Node::new(Some("modal"), Rect::new(150.0, 20.0, 60.0, 40.0)).child(Node::new(
        Some("modal button"),
        Rect::new(5.0, 5.0, 20.0, 10.0),
    ));
    modal.overlay = true;
    let clipper = Node::new(Some("clipper"), Rect::new(0.0, 0.0, 100.0, 100.0))
        .child(Node::new(
            Some("in place"),
            Rect::new(150.0, 20.0, 50.0, 40.0),
        ))
        .child(modal);

    let mut hidden = Node::new(Some("hidden"), Rect::new(10.0, 120.0, 30.0, 30.0)).child(
        Node::new(Some("hidden child"), Rect::new(0.0, 0.0, 10.0, 10.0)),
    );
    hidden.visible = false;

    let mut fade = Opacity::new(
        Rc::new(Cell::new(0.0)),
        Box::new(Node::new(Some("faded"), Rect::new(0.0, 0.0, 30.0, 30.0))),
    );
    fade.layout(Size::new(30.0, 30.0));
    fade.set_bounds(Rect::new(50.0, 120.0, 30.0, 30.0));

    let mut raised = Node::new(Some("raised"), Rect::new(120.0, 120.0, 30.0, 30.0));
    raised.on_top = true;

    Node::new(Some("root"), Rect::new(0.0, 0.0, 400.0, 300.0))
        .child(view)
        .child(row)
        .child(canvas)
        .child(clipper)
        .child(hidden)
        .child(fade)
        .child(raised)
        .child(Node::new(
            Some("after raised"),
            Rect::new(130.0, 120.0, 30.0, 30.0),
        ))
}

/// Each visited id with its visible rect, in visiting order.
fn walked(root: &dyn Widget) -> Vec<(String, Option<Rect>)> {
    let mut seen = Vec::new();
    walk_painted(root, |widget, placement| {
        if let Some(id) = widget.id() {
            let own = widget.bounds();
            let shown = placement.visible_rect(Size::new(own.width, own.height));
            seen.push((id.to_string(), shown));
        }
        PaintWalk::Descend
    });
    seen
}

/// The visible rect the walk gave `id`; `None` when it never visited it.
fn shown(seen: &[(String, Option<Rect>)], id: &str) -> Option<Option<Rect>> {
    seen.iter()
        .find(|(seen_id, _)| seen_id == id)
        .map(|(_, r)| *r)
}

#[test]
fn the_walk_shows_every_widget_where_the_visible_rect_lookup_does() {
    let root = fixture();
    let seen = walked(&root);
    for id in IDS {
        let from_walk = shown(&seen, id).flatten();
        assert_eq!(
            from_walk,
            find_widget_visible_rect(&root, id),
            "visible rect of {id}"
        );
    }
    // The fixture exercises each rule: something is cut, zoomed, escapes.
    assert_eq!(
        shown(&seen, "half").flatten(),
        Some(Rect::new(60.0, 180.0, 120.0, 50.0))
    );
    assert_eq!(shown(&seen, "far").flatten(), None);
    assert_eq!(shown(&seen, "in place").flatten(), None);
    assert_eq!(
        shown(&seen, "modal").flatten(),
        Some(Rect::new(150.0, 20.0, 60.0, 40.0))
    );
    // Zoomed 2x and panned to (265, 127.5), the card paints snapped
    // onto a whole pixel, as widgets enforcing integer bounds do.
    assert_eq!(
        shown(&seen, "card").flatten(),
        Some(Rect::new(265.0, 127.0, 80.0, 60.0))
    );
}

#[test]
fn hidden_and_faded_subtrees_are_never_visited() {
    let seen = walked(&fixture());
    for id in ["hidden", "hidden child", "faded"] {
        assert_eq!(shown(&seen, id), None, "{id} paints nothing");
    }
}

#[test]
fn the_walk_visits_in_paint_order() {
    let seen = walked(&fixture());
    let order: Vec<&str> = seen.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        order,
        [
            "root",
            "top",
            "half",
            "far",
            "cell",
            "spill",
            "canvas",
            "card",
            "card label",
            "clipper",
            "in place",
            "after raised",
            "raised",
            "modal",
            "modal button",
        ],
        "a raised child paints after its later sibling, the overlay pass after the whole tree"
    );
}

#[test]
fn skip_leaves_out_the_subtree_and_stop_ends_the_walk() {
    let root = fixture();
    let mut order = Vec::new();
    walk_painted(&root, |widget, _| {
        let id = widget.id().unwrap_or("");
        order.push(id.to_string());
        match id {
            "clipper" => PaintWalk::Skip,
            _ if widget.type_name() == "ScrollView" => PaintWalk::Skip,
            "raised" => PaintWalk::Stop,
            _ => PaintWalk::Descend,
        }
    });
    let named: Vec<&str> = order
        .iter()
        .map(String::as_str)
        .filter(|id| !id.is_empty())
        .collect();
    assert_eq!(
        named,
        [
            "root",
            "cell",
            "spill",
            "canvas",
            "card",
            "card label",
            "clipper",
            "after raised",
            "raised",
        ],
        "the overlay pass never ran"
    );

    // Skipping the clipper leaves its in-place child out, but its overlay
    // child still paints in the overlay pass.
    let mut seen = Vec::new();
    walk_painted(&root, |widget, _| {
        let id = widget.id().unwrap_or("");
        seen.push(id.to_string());
        if id == "clipper" {
            PaintWalk::Skip
        } else {
            PaintWalk::Descend
        }
    });
    assert!(!seen.iter().any(|id| id == "in place"));
    assert!(seen.iter().any(|id| id == "modal button"));
}
