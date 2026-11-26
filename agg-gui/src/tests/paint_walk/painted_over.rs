//! What paints over a widget: `painted_over` lists what a card floating over
//! a view must keep clear of. A full-window layer the host names is looked
//! into rather than listed, so only the button on it counts, while one it
//! does not name (an opaque page) lists whole; a panel beside or over part of
//! the view lists as one rectangle, grown by what its children may paint past
//! its bounds; hidden subtrees list nothing; the host's own layer and what
//! paints after it are left out; and the walk over a tree the size of a real
//! app's window is counted and timed.

use super::Node;
use crate::geometry::Rect;
use crate::widget::paint_walk::painted_over_counted;
use crate::widget::{painted_over, walk_painted, PaintWalk, Widget};
use std::time::Instant;

const WINDOW: Rect = Rect::new(0.0, 0.0, 400.0, 300.0);

fn window() -> Node {
    Node::new(None, WINDOW)
}

/// The see-through layers of these trees, as a host names its own.
fn is_layer(widget: &dyn Widget) -> bool {
    matches!(widget.id(), Some("layer" | "cards"))
}

#[test]
fn a_covering_widget_the_host_does_not_name_as_a_layer_lists_whole() {
    let root = window().child(Node::new(Some("view"), WINDOW)).child(
        Node::new(Some("page"), WINDOW).child(Node::new(
            Some("button"),
            Rect::new(300.0, 250.0, 30.0, 20.0),
        )),
    );
    assert_eq!(painted_over(&root, "view", None, is_layer), vec![WINDOW]);
}

#[test]
fn a_panel_whose_children_paint_past_its_bounds_lists_what_they_may_cover() {
    let view = Rect::new(100.0, 0.0, 300.0, 300.0);
    // A 150-wide panel whose children are free to spill past its bounds, and
    // a narrow one clipping them: the free one lists out to the view's edges.
    let mut free = Node::new(Some("free"), Rect::new(0.0, 0.0, 150.0, 100.0))
        .child(Node::new(None, Rect::new(10.0, 10.0, 20.0, 20.0)));
    free.clips = false;
    let clipped = Node::new(Some("clipped"), Rect::new(0.0, 150.0, 150.0, 100.0))
        .child(Node::new(None, Rect::new(10.0, 10.0, 20.0, 20.0)));
    let root = window()
        .child(Node::new(Some("view"), view))
        .child(free)
        .child(clipped);
    assert_eq!(
        painted_over(&root, "view", None, is_layer),
        vec![
            Rect::new(0.0, 0.0, 400.0, 300.0),
            Rect::new(0.0, 150.0, 150.0, 100.0)
        ]
    );
}

#[test]
fn a_full_window_layer_lists_only_the_button_on_it() {
    let button = Rect::new(300.0, 250.0, 30.0, 20.0);
    let root = window()
        .child(Node::new(Some("view"), WINDOW))
        .child(Node::new(Some("layer"), WINDOW).child(Node::new(Some("button"), button)));
    assert_eq!(painted_over(&root, "view", None, is_layer), vec![button]);
}

#[test]
fn a_panel_beside_or_over_part_of_the_view_lists_as_one_rect() {
    let view = Rect::new(100.0, 0.0, 300.0, 300.0);
    // The panel reaches 50 units over the view's left edge and holds
    // controls of its own; it lists whole, its controls not at all.
    let panel = Rect::new(0.0, 0.0, 150.0, 300.0);
    let root = window().child(Node::new(Some("view"), view)).child(
        Node::new(Some("panel"), panel)
            .child(Node::new(Some("field"), Rect::new(10.0, 10.0, 130.0, 20.0)))
            .child(Node::new(
                Some("slider"),
                Rect::new(10.0, 40.0, 130.0, 20.0),
            )),
    );
    assert_eq!(painted_over(&root, "view", None, is_layer), vec![panel]);

    // A panel only beside the view, touching its edge, covers nothing of it.
    let root = window()
        .child(Node::new(Some("view"), view))
        .child(Node::new(Some("panel"), Rect::new(0.0, 0.0, 100.0, 300.0)));
    assert_eq!(
        painted_over(&root, "view", None, is_layer),
        Vec::<Rect>::new()
    );
}

#[test]
fn what_paints_before_the_view_does_not_count() {
    let root = window()
        .child(Node::new(Some("under"), Rect::new(10.0, 10.0, 50.0, 50.0)))
        .child(Node::new(Some("view"), WINDOW));
    assert_eq!(
        painted_over(&root, "view", None, is_layer),
        Vec::<Rect>::new()
    );
}

#[test]
fn controls_placed_on_the_view_count() {
    let gizmo = Rect::new(20.0, 20.0, 40.0, 40.0);
    let root = window().child(
        Node::new(Some("view"), Rect::new(100.0, 0.0, 300.0, 300.0))
            .child(Node::new(Some("gizmo"), gizmo)),
    );
    assert_eq!(
        painted_over(&root, "view", None, is_layer),
        vec![Rect::new(120.0, 20.0, 40.0, 40.0)]
    );
}

#[test]
fn hidden_subtrees_list_nothing() {
    let mut layer = Node::new(Some("layer"), WINDOW)
        .child(Node::new(Some("button"), Rect::new(10.0, 10.0, 20.0, 20.0)));
    layer.visible = false;
    let mut panel = Node::new(Some("panel"), Rect::new(0.0, 0.0, 100.0, 100.0));
    panel.visible = false;
    let root = window()
        .child(Node::new(Some("view"), WINDOW))
        .child(layer)
        .child(panel);
    assert_eq!(
        painted_over(&root, "view", None, is_layer),
        Vec::<Rect>::new()
    );

    // A hidden view has nothing showing to be painted over.
    let mut view = Node::new(Some("view"), WINDOW);
    view.visible = false;
    let root = window()
        .child(view)
        .child(Node::new(Some("button"), Rect::new(10.0, 10.0, 20.0, 20.0)));
    assert_eq!(
        painted_over(&root, "view", None, is_layer),
        Vec::<Rect>::new()
    );
}

#[test]
fn the_stop_widget_and_everything_after_it_are_left_out() {
    let before = Rect::new(10.0, 10.0, 20.0, 20.0);
    let mut modal = Node::new(Some("modal"), Rect::new(100.0, 100.0, 100.0, 80.0));
    modal.overlay = true;
    let root = window()
        .child(Node::new(Some("view"), WINDOW))
        .child(Node::new(Some("before"), before))
        .child(
            Node::new(Some("cards"), WINDOW)
                .child(Node::new(Some("card"), Rect::new(50.0, 50.0, 80.0, 40.0))),
        )
        .child(Node::new(Some("after"), Rect::new(200.0, 10.0, 20.0, 20.0)))
        .child(modal);
    assert_eq!(
        painted_over(&root, "view", Some("cards"), is_layer),
        vec![before]
    );
    // Without a stop the later sibling and the overlay-pass modal, painted
    // after the whole tree, count too.
    assert_eq!(
        painted_over(&root, "view", None, is_layer),
        vec![
            before,
            Rect::new(50.0, 50.0, 80.0, 40.0),
            Rect::new(200.0, 10.0, 20.0, 20.0),
            Rect::new(100.0, 100.0, 100.0, 80.0),
        ]
    );
    // A stop no widget carries stops nothing.
    assert_eq!(
        painted_over(&root, "view", Some("nowhere"), is_layer),
        painted_over(&root, "view", None, is_layer)
    );
    // A stop painted before the view leaves nothing to list.
    let root = window()
        .child(Node::new(Some("first"), Rect::new(0.0, 0.0, 10.0, 10.0)))
        .child(Node::new(Some("view"), WINDOW))
        .child(Node::new(Some("button"), Rect::new(10.0, 10.0, 20.0, 20.0)));
    assert_eq!(
        painted_over(&root, "view", Some("first"), is_layer),
        Vec::<Rect>::new()
    );
}

/// A window laid out like a design app's: a toolbar of 40 buttons, a left
/// panel of 30 sections of 20 property rows (label, field and a reset
/// button each), the 3D view with 12 controls on it, a right panel of 400
/// rows, and a full-window card layer holding one card, painted last.
fn app_sized_window() -> Node {
    let mut toolbar = Node::new(Some("toolbar"), Rect::new(0.0, 270.0, 1600.0, 30.0));
    for i in 0..40 {
        toolbar = toolbar.child(Node::new(
            None,
            Rect::new(5.0 + 30.0 * f64::from(i), 2.0, 26.0, 26.0),
        ));
    }
    let mut left = Node::new(Some("left"), Rect::new(0.0, 0.0, 300.0, 270.0));
    for s in 0..30 {
        let mut section = Node::new(
            None,
            Rect::new(0.0, 270.0 - 9.0 * f64::from(s + 1), 300.0, 9.0),
        );
        for r in 0..20 {
            let y = 0.4 * f64::from(r);
            section = section.child(
                Node::new(None, Rect::new(0.0, y, 300.0, 0.4))
                    .child(Node::new(None, Rect::new(0.0, 0.0, 100.0, 0.4)))
                    .child(Node::new(None, Rect::new(100.0, 0.0, 170.0, 0.4)))
                    .child(Node::new(None, Rect::new(270.0, 0.0, 30.0, 0.4))),
            );
        }
        left = left.child(section);
    }
    let mut view = Node::new(Some("view"), Rect::new(300.0, 0.0, 1000.0, 270.0));
    for i in 0..12 {
        view = view.child(Node::new(
            None,
            Rect::new(10.0 + 40.0 * f64::from(i), 230.0, 32.0, 32.0),
        ));
    }
    let mut right = Node::new(Some("right"), Rect::new(1300.0, 0.0, 300.0, 270.0));
    for r in 0..400 {
        right = right.child(Node::new(
            None,
            Rect::new(0.0, 270.0 - 0.6 * f64::from(r + 1), 300.0, 0.6),
        ));
    }
    let cards = Node::new(Some("cards"), Rect::new(0.0, 0.0, 1600.0, 300.0)).child(Node::new(
        Some("card"),
        Rect::new(600.0, 100.0, 200.0, 80.0),
    ));
    Node::new(None, Rect::new(0.0, 0.0, 1600.0, 300.0))
        .child(toolbar)
        .child(left)
        .child(view)
        .child(right)
        .child(cards)
}

#[test]
fn an_app_sized_window_lists_the_views_controls_and_walks_less_than_the_tree() {
    let root = app_sized_window();
    let mut total = 0usize;
    walk_painted(&root, |_, _| {
        total += 1;
        PaintWalk::Descend
    });
    let (over, visited) = painted_over_counted(&root, "view", Some("cards"), is_layer);
    // The view's own 12 controls; the panels only touch its edges.
    assert_eq!(over.len(), 12);
    let runs = 200u32;
    let start = Instant::now();
    for _ in 0..runs {
        std::hint::black_box(painted_over(
            std::hint::black_box(&root),
            "view",
            Some("cards"),
            is_layer,
        ));
    }
    let each = start.elapsed() / runs;
    eprintln!("painted_over: {visited} of {total} painted widgets visited, {each:?} per call");
    // Everything painted before the view is walked to find it; after it,
    // only what can reach the view.
    assert!(visited < total, "{visited} of {total}");
}
