//! Where a widget shows on screen: `find_widget_visible_rect` cuts a widget's
//! screen rectangle by every clip painting puts it under, so a host pointing
//! at a widget (a hint card's arrow, a tutorial highlight) can tell a widget
//! scrolled out of a `ScrollView` from one on show, and how much of it shows.
//! A subtree painted in the overlay pass escapes its ancestors' clips and
//! shows wherever the window does; a row's per-column clip cuts what spills
//! out of a column; a subtree an `Opacity` has faded out shows nothing; and
//! the window is where the root sits, less what the on-screen keyboard's
//! lift pushes off its top.

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::widget::{find_widget_screen_rect, find_widget_visible_rect, Widget};
use crate::widgets::{FlexRow, Opacity, ScrollView};
use std::cell::Cell;
use std::rc::Rc;

/// A plain widget at fixed bounds: an optional id, its children clipped to
/// its bounds or free to spill past them, an optional move to the overlay pass, and a
/// visibility switch.
struct Node {
    id: Option<&'static str>,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    clips: bool,
    overlay: bool,
    visible: bool,
}

impl Node {
    fn new(id: Option<&'static str>, bounds: Rect) -> Self {
        Self {
            id,
            bounds,
            children: Vec::new(),
            clips: false,
            overlay: false,
            visible: true,
        }
    }
    fn child(mut self, c: Node) -> Self {
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
    fn defer_paint_to_overlay(&self) -> bool {
        self.overlay
    }
    fn is_visible(&self) -> bool {
        self.visible
    }
}

/// The ScrollView's place in the 400 × 300 window.
const VIEW: Rect = Rect::new(50.0, 50.0, 200.0, 100.0);

/// A 400 × 300 window holding a 200 × 100 ScrollView over 150 × 600 of
/// content, scrolled down by `offset` from the content's top. The content
/// holds `top` (its top 100 units), `half` (50 units below the content's
/// top to 50 units under the view at offset 0) and `far` (the bottom 100).
fn scrolled(offset: f64) -> Node {
    let content = Node::new(None, Rect::new(0.0, 0.0, 150.0, 600.0))
        .child(Node::new(Some("top"), Rect::new(10.0, 500.0, 120.0, 100.0)))
        .child(Node::new(
            Some("half"),
            Rect::new(10.0, 450.0, 120.0, 100.0),
        ))
        .child(Node::new(Some("far"), Rect::new(10.0, 0.0, 120.0, 100.0)));
    let mut view = ScrollView::new(Box::new(content));
    view.set_scroll_offset(offset);
    view.layout(Size::new(VIEW.width, VIEW.height));
    view.set_bounds(VIEW);
    let mut root = Node::new(None, Rect::new(0.0, 0.0, 400.0, 300.0));
    root.children.push(Box::new(view));
    root
}

#[test]
fn a_widget_scrolled_wholly_out_of_view_has_no_visible_rect() {
    let root = scrolled(0.0);
    assert!(
        find_widget_screen_rect(&root, "far").is_some(),
        "the widget is still in the tree"
    );
    assert_eq!(find_widget_visible_rect(&root, "far"), None);
}

#[test]
fn a_widget_half_scrolled_out_shows_only_its_part_inside_the_view() {
    let root = scrolled(0.0);
    let whole = find_widget_screen_rect(&root, "half").expect("half is in the tree");
    let shown = find_widget_visible_rect(&root, "half").expect("half partly shows");
    assert_eq!(whole.height, 100.0);
    assert_eq!(shown.x, whole.x);
    assert_eq!(shown.width, whole.width);
    // The view's bottom edge cuts the lower half off.
    assert_eq!(shown.y, VIEW.y);
    assert_eq!(shown.height, 50.0);
    assert_eq!(shown.y + shown.height, whole.y + whole.height);
}

#[test]
fn a_widget_scrolled_into_view_shows_whole() {
    let root = scrolled(0.0);
    let whole = find_widget_screen_rect(&root, "top").expect("top is in the tree");
    assert_eq!(find_widget_visible_rect(&root, "top"), Some(whole));
    assert_eq!(whole.y + whole.height, VIEW.y + VIEW.height);

    // Scrolled to the bottom, the far widget shows whole and the top one not at all.
    let root = scrolled(500.0);
    let far = find_widget_screen_rect(&root, "far").expect("far is in the tree");
    assert_eq!(find_widget_visible_rect(&root, "far"), Some(far));
    assert_eq!(far.y, VIEW.y);
    assert_eq!(find_widget_visible_rect(&root, "top"), None);
}

#[test]
fn an_overlay_escapes_its_parents_clip() {
    // A 100 × 100 clipping box at the window's corner, with two children
    // placed wholly outside it: one painted in place, one in the overlay pass.
    let mut clipper = Node::new(None, Rect::new(0.0, 0.0, 100.0, 100.0)).child(Node::new(
        Some("in place"),
        Rect::new(150.0, 20.0, 50.0, 40.0),
    ));
    let mut overlay = Node::new(Some("overlay"), Rect::new(150.0, 20.0, 50.0, 40.0));
    overlay.overlay = true;
    clipper = clipper.child(overlay);
    clipper.clips = true;
    let root = Node::new(None, Rect::new(0.0, 0.0, 400.0, 300.0)).child(clipper);

    assert_eq!(find_widget_visible_rect(&root, "in place"), None);
    assert_eq!(
        find_widget_visible_rect(&root, "overlay"),
        Some(Rect::new(150.0, 20.0, 50.0, 40.0))
    );
}

#[test]
fn a_widget_past_the_window_edge_shows_only_inside_the_window() {
    let root = Node::new(None, Rect::new(0.0, 0.0, 400.0, 300.0))
        .child(Node::new(Some("edge"), Rect::new(350.0, 10.0, 100.0, 20.0)));
    assert_eq!(
        find_widget_visible_rect(&root, "edge"),
        Some(Rect::new(350.0, 10.0, 50.0, 20.0))
    );
}

#[test]
fn hidden_or_missing_widgets_have_no_visible_rect() {
    let root = scrolled(0.0);
    assert_eq!(find_widget_visible_rect(&root, "nowhere"), None);

    let mut hidden = Node::new(Some("hidden"), Rect::new(10.0, 10.0, 50.0, 50.0));
    hidden.visible = false;
    let root = Node::new(None, Rect::new(0.0, 0.0, 400.0, 300.0)).child(hidden);
    assert_eq!(find_widget_visible_rect(&root, "hidden"), None);
}

#[test]
fn a_root_placed_away_from_the_origin_shows_its_whole_width() {
    let root = Node::new(None, Rect::new(100.0, 0.0, 300.0, 300.0))
        .child(Node::new(Some("right"), Rect::new(200.0, 10.0, 50.0, 20.0)));
    assert_eq!(
        find_widget_visible_rect(&root, "right"),
        Some(Rect::new(300.0, 10.0, 50.0, 20.0))
    );
}

/// Puts the keyboard lift back to none even when an assertion fails.
struct LiftGuard;
impl Drop for LiftGuard {
    fn drop(&mut self) {
        crate::widget::keyboard_scroll::reset_lift_for_test();
    }
}

#[test]
fn the_keyboard_lift_hides_what_it_pushes_off_the_top() {
    let root = Node::new(None, Rect::new(0.0, 0.0, 400.0, 300.0))
        .child(Node::new(Some("high"), Rect::new(10.0, 250.0, 50.0, 40.0)))
        .child(Node::new(
            Some("middle"),
            Rect::new(10.0, 180.0, 50.0, 40.0),
        ))
        .child(Node::new(Some("low"), Rect::new(10.0, 20.0, 50.0, 40.0)));
    let _guard = LiftGuard;
    crate::widget::keyboard_lift::set_lift_for_test(100.0);
    // Lifted by 100, root space above y 200 is off the top of the window.
    assert_eq!(find_widget_visible_rect(&root, "high"), None);
    assert_eq!(
        find_widget_visible_rect(&root, "middle"),
        Some(Rect::new(10.0, 180.0, 50.0, 20.0))
    );
    assert_eq!(
        find_widget_visible_rect(&root, "low"),
        Some(Rect::new(10.0, 20.0, 50.0, 40.0))
    );
}

#[test]
fn a_widget_under_a_faded_out_opacity_shows_nothing() {
    let opacity = Rc::new(Cell::new(0.0));
    let mut fade = Opacity::new(
        opacity.clone(),
        Box::new(Node::new(Some("faded"), Rect::new(0.0, 0.0, 50.0, 40.0))),
    );
    fade.layout(Size::new(50.0, 40.0));
    fade.set_bounds(Rect::new(20.0, 30.0, 50.0, 40.0));
    let mut root = Node::new(None, Rect::new(0.0, 0.0, 400.0, 300.0));
    root.children.push(Box::new(fade));

    assert!(find_widget_screen_rect(&root, "faded").is_some());
    assert_eq!(find_widget_visible_rect(&root, "faded"), None);

    opacity.set(1.0);
    assert_eq!(
        find_widget_visible_rect(&root, "faded"),
        find_widget_screen_rect(&root, "faded")
    );
}

/// A 300 × 40 window holding a row with one 50 × 40 column whose child
/// spills 40 units past the column's right edge, the row clipping each
/// child to its column when `clip_to_slots`.
fn row_with_spill(clip_to_slots: bool) -> Node {
    let cell = Node::new(Some("cell"), Rect::new(0.0, 0.0, 50.0, 40.0))
        .child(Node::new(Some("spill"), Rect::new(30.0, 10.0, 60.0, 20.0)));
    let mut row = FlexRow::new()
        .with_clip_children_to_slots(clip_to_slots)
        .add(Box::new(cell));
    row.layout(Size::new(300.0, 40.0));
    row.set_bounds(Rect::new(0.0, 0.0, 300.0, 40.0));
    let mut root = Node::new(None, Rect::new(0.0, 0.0, 300.0, 40.0));
    root.children.push(Box::new(row));
    root
}

#[test]
fn a_rows_column_clip_cuts_what_spills_out_of_the_column() {
    let root = row_with_spill(true);
    let cell = find_widget_screen_rect(&root, "cell").expect("the column's child");
    let spill = find_widget_screen_rect(&root, "spill").expect("the spilling widget");
    assert!(spill.x + spill.width > cell.x + cell.width, "it spills");
    assert_eq!(
        find_widget_visible_rect(&root, "spill"),
        Some(Rect::new(
            spill.x,
            spill.y,
            cell.x + cell.width - spill.x,
            spill.height
        ))
    );

    // Without the column clip the row shows the spill whole.
    let root = row_with_spill(false);
    let spill = find_widget_screen_rect(&root, "spill").expect("the spilling widget");
    assert_eq!(find_widget_visible_rect(&root, "spill"), Some(spill));
}
