//! agg-sharp `ScaledParentClipTests` (`Tests/Agg.Tests/Agg.UI`), ported
//! against agg-gui's paint, hit-test and coordinate chains and this crate's
//! lookups (`tree_query`, `name_pick`, the runner's search regions): a widget
//! whose children are drawn scaled, flipped or turned (a zoomable canvas such
//! as the node editor's card layer) has its children clipped, hit, searched
//! and painted where they are drawn. Same test names; C#'s `[Arguments]` and
//! `[MethodDataSource]` rows run as loops.
//!
//! C# `ParentToChildTransform` is a widget's whole placement; agg-gui splits
//! it into the widget's `bounds()` origin and its parent's `child_transform`.
//! C#'s scaled canvas therefore becomes a canvas at the origin whose
//! `child_transform` carries the zoom and pan. C# search regions are Y-up
//! window rectangles; the runner's are Y-down pixels of the same 400 x 300
//! window, converted by hand below. agg-gui has no rectangle invalidation
//! (a dirty widget repaints its retained layer whole), so the invalidation
//! halves of these tests are not ported.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use agg_gui::widget::coords::{rect_to, CoordSpace};
use agg_gui::widget::paint_subtree;
use agg_gui::{
    App, Color, Framebuffer, GfxCtx, Modifiers, MouseButton, Rect, Size, TransAffine, Widget,
};
use agg_gui_automation::name_pick::find_on_screen;
use agg_gui_automation::tree_query::{actually_visible_on_screen, clipped_rect, WidgetHandle};
use agg_gui_automation::{
    AutomationConfig, AutomationRunner, HeadlessDriver, ScreenRectangle, SearchRegion, WaitOpts,
};
use support::Switchable;

const VIEWPORT: Size = Size {
    width: 400.0,
    height: 300.0,
};

/// C# `Affine.NewScaling(sx, sy) * Affine.NewTranslation(tx, ty)`.
fn scale_then_move(sx: f64, sy: f64, tx: f64, ty: f64) -> TransAffine {
    let mut t = TransAffine::new_scaling(sx, sy);
    t.multiply(&TransAffine::new_translation(tx, ty));
    t
}

/// C# `Affine.NewRotation(angle) * Affine.NewTranslation(tx, ty)`.
fn turn_then_move(angle: f64, tx: f64, ty: f64) -> TransAffine {
    let mut t = TransAffine::new_rotation(angle);
    t.multiply(&TransAffine::new_translation(tx, ty));
    t
}

fn sw(name: &str, x: f64, y: f64, w: f64, h: f64) -> Switchable {
    Switchable::new(name, Rect::new(x, y, w, h))
}

/// A 400 x 300 window holding a container that holds a canvas drawing its
/// children through `transform`, with `child` on the canvas. The canvas is
/// at path `[0, 0]`, its child at `[0, 0, 0]`.
fn build_tree(transform: TransAffine, canvas_size: Size, child: Option<Switchable>) -> Switchable {
    let mut canvas = sw("canvas", 0.0, 0.0, canvas_size.width, canvas_size.height)
        .with_child_transform(transform);
    if let Some(child) = child {
        canvas = canvas.with_child(Box::new(child));
    }
    let container = sw("container", 0.0, 0.0, 400.0, 300.0).with_child(Box::new(canvas));
    sw("window", 0.0, 0.0, 400.0, 300.0).with_child(Box::new(container))
}

const CHILD: [usize; 3] = [0, 0, 0];

fn handle(root: &dyn Widget, path: &[usize]) -> WidgetHandle {
    WidgetHandle::new(root, path).expect("in the tree")
}

/// A half-scale canvas, and a child near its top that is drawn at (20, 190)
/// to (40, 200) in the container. At full size the same child would sit at
/// y 380 to 400, past the container's 300.
fn half_scale_tree_with_a_visible_child(presses: &Arc<AtomicUsize>) -> Switchable {
    let child = sw("target", 0.0, 360.0, 40.0, 20.0).counting_presses(presses);
    build_tree(
        scale_then_move(0.5, 0.5, 20.0, 10.0),
        Size::new(600.0, 400.0),
        Some(child),
    )
}

/// A double-scale canvas, and a child drawn at y 320 to 360 in the
/// container, above its 300, which at full size would have looked inside.
fn double_scale_tree_with_a_hidden_child() -> Switchable {
    let child = sw("target", 0.0, 160.0, 40.0, 20.0);
    build_tree(
        scale_then_move(2.0, 2.0, 0.0, 0.0),
        Size::new(200.0, 200.0),
        Some(child),
    )
}

/// A runner search region from C#'s Y-up window rectangle (left, bottom,
/// right, top).
fn region(left: i32, bottom: i32, right: i32, top: i32) -> SearchRegion {
    SearchRegion::new(ScreenRectangle::new(left, 300 - top, right, 300 - bottom))
}

/// How many widgets named `name` the runner finds overlapping `search`.
fn found_in(root: Switchable, name: &str, search: &SearchRegion) -> usize {
    let mut runner = AutomationRunner::attach(
        HeadlessDriver::new(Box::new(root), 400, 300),
        AutomationConfig::default(),
    );
    let opts = WaitOpts {
        secs_to_wait: 0.0,
        search_region: Some(search),
        only_visible: false,
    };
    let found = runner.get_widgets_by_name(name, &opts).len();
    runner.mark_test_complete();
    found
}

#[test]
fn a_child_drawn_inside_a_shrunk_canvas_can_be_selected_and_clicked() {
    let presses = Arc::new(AtomicUsize::new(0));
    let root = half_scale_tree_with_a_visible_child(&presses);

    assert!(
        actually_visible_on_screen(&root, &handle(&root, &CHILD), VIEWPORT),
        "the child is drawn inside the container, so it has to take the mouse"
    );

    let drawn = rect_to(
        &root,
        &CHILD,
        CoordSpace::Ancestor(0),
        Rect::new(0.0, 0.0, 40.0, 20.0),
    )
    .unwrap();
    assert!((drawn.x - 20.0).abs() <= 1e-9);
    assert!((drawn.y - 190.0).abs() <= 1e-9);

    let mut app = App::new(Box::new(root));
    app.layout(VIEWPORT);
    // (30, 195) Y-up is y 105 Y-down.
    app.on_mouse_down(30.0, 105.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(30.0, 105.0, MouseButton::Left, Modifiers::default());
    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "a press on the child where it is drawn has to reach it"
    );
}

#[test]
fn a_child_drawn_outside_an_enlarged_canvas_stays_unselectable() {
    let root = double_scale_tree_with_a_hidden_child();
    assert!(
        !actually_visible_on_screen(&root, &handle(&root, &CHILD), VIEWPORT),
        "scaled up, the child is drawn above the container and cannot be seen"
    );
    assert!(find_on_screen(&root, "target", VIEWPORT).is_none());
}

/// C# judges the canvas itself, scaled by its own transform, against the
/// container. In agg-gui the scale belongs to the parent (`child_transform`),
/// so the widget judged is a child of the scaling container.
#[test]
fn a_shrunk_canvas_is_visible_on_its_parent_where_it_is_drawn() {
    // Drawn at x 250 to 300 of the 400-wide container; at full size it
    // would be at 500 to 600.
    let canvas = sw("canvas", 500.0, 0.0, 100.0, 100.0);
    let container = sw("container", 0.0, 0.0, 400.0, 300.0)
        .with_child_transform(scale_then_move(0.5, 0.5, 0.0, 0.0))
        .with_child(Box::new(canvas));
    let root = sw("window", 0.0, 0.0, 400.0, 300.0).with_child(Box::new(container));
    assert!(actually_visible_on_screen(
        &root,
        &handle(&root, &[0, 0]),
        VIEWPORT
    ));
}

#[test]
fn an_enlarged_canvas_drawn_past_its_parent_is_not_visible_on_it() {
    // Drawn at x 500 to 600, past the 400-wide container; at full size it
    // would be at 250 to 300.
    let canvas = sw("canvas", 250.0, 0.0, 50.0, 50.0);
    let container = sw("container", 0.0, 0.0, 400.0, 300.0)
        .with_child_transform(scale_then_move(2.0, 2.0, 0.0, 0.0))
        .with_child(Box::new(canvas));
    let root = sw("window", 0.0, 0.0, 400.0, 300.0).with_child(Box::new(container));
    assert!(!actually_visible_on_screen(
        &root,
        &handle(&root, &[0, 0]),
        VIEWPORT
    ));
}

#[test]
fn on_screen_bounds_of_a_scaled_child_are_where_it_is_drawn() {
    let root = half_scale_tree_with_a_visible_child(&Arc::new(AtomicUsize::new(0)));
    let child = handle(&root, &CHILD);
    assert!(actually_visible_on_screen(&root, &child, VIEWPORT));
    assert_eq!(
        clipped_rect(&root, &child, VIEWPORT),
        Some(Rect::new(20.0, 190.0, 20.0, 10.0))
    );
}

#[test]
fn a_search_region_finds_a_scaled_child_where_it_is_drawn() {
    let tree = || half_scale_tree_with_a_visible_child(&Arc::new(AtomicUsize::new(0)));
    assert_eq!(
        found_in(tree(), "target", &region(25, 192, 35, 198)),
        1,
        "the region covers the middle of the child as it is drawn"
    );
    assert_eq!(
        found_in(tree(), "target", &region(45, 375, 55, 385)),
        0,
        "that region is where the child would be at full size, not where it is drawn"
    );
}

/// C# narrows its search region to each widget whose name matches, so a
/// region beside a matching widget reached its children inverted. The
/// runner tests each named widget's own drawn rectangle against the region,
/// so the case it pins is that a region beside both finds neither.
#[test]
fn an_empty_search_region_stays_empty_through_a_translated_child() {
    let container = sw("container", 0.0, 0.0, 50.0, 300.0)
        .with_child(Box::new(sw("target", 60.0, 0.0, 20.0, 20.0)));
    let root = sw("window", 0.0, 0.0, 400.0, 300.0).with_child(Box::new(container));
    assert_eq!(
        found_in(root, "target", &region(100, 5, 101, 15)),
        0,
        "the region lies past both the container and the child it holds at x 60 to 80"
    );
}

/// A flipped canvas (negative scale) maps its children mirrored.
#[test]
fn a_flipped_canvas_invalidates_selects_and_finds_where_its_child_is_drawn() {
    let tree = || {
        build_tree(
            scale_then_move(-1.0, 1.0, 200.0, 0.0),
            Size::new(400.0, 300.0),
            Some(sw("target", 0.0, 0.0, 40.0, 20.0)),
        )
    };
    let root = tree();
    // x 0 to 40 mirrored is -40 to 0, then moved right by 200.
    let drawn = rect_to(
        &root,
        &CHILD,
        CoordSpace::Ancestor(2),
        Rect::new(0.0, 0.0, 40.0, 20.0),
    )
    .unwrap();
    assert_eq!(drawn, Rect::new(160.0, 0.0, 40.0, 20.0));
    assert!(actually_visible_on_screen(
        &root,
        &handle(&root, &CHILD),
        VIEWPORT
    ));
    assert_eq!(found_in(tree(), "target", &region(170, 5, 190, 15)), 1);
}

/// A canvas that turns its children maps a rectangle by its four corners.
#[test]
fn a_rotated_canvas_invalidates_and_finds_where_its_child_is_drawn() {
    // A quarter turn counter-clockwise, (x, y) -> (-y, x), then moved to
    // (200, 100).
    let tree = || {
        build_tree(
            turn_then_move(std::f64::consts::FRAC_PI_2, 200.0, 100.0),
            Size::new(400.0, 300.0),
            Some(sw("target", 0.0, 0.0, 40.0, 20.0)),
        )
    };
    let root = tree();
    let drawn = rect_to(
        &root,
        &CHILD,
        CoordSpace::Ancestor(2),
        Rect::new(0.0, 0.0, 40.0, 20.0),
    )
    .unwrap();
    assert!((drawn.x - 180.0).abs() <= 1e-9);
    assert!((drawn.y - 100.0).abs() <= 1e-9);
    assert!((drawn.x + drawn.width - 200.0).abs() <= 1e-9);
    assert!((drawn.y + drawn.height - 140.0).abs() <= 1e-9);

    assert_eq!(found_in(tree(), "target", &region(185, 110, 195, 130)), 1);
    assert_eq!(
        found_in(tree(), "target", &region(205, 105, 235, 115)),
        0,
        "that is where the child would be drawn unrotated"
    );
}

/// A search by name finds the child of a turned or sheared widget under a
/// shrinking canvas. C# searches from an unbounded region that the shrinking
/// canvas scales to infinity, which a turn or shear below turned into NaN;
/// the runner's lookup walks names and maps each one's finite rectangle, so
/// what is left to pin is that the drawn child is found, on show.
///
/// C#'s `turned` carries its own turn as its placement; here a holder the
/// size of the canvas turns its one child, `turned`, into the same place.
#[test]
fn a_search_by_name_finds_a_child_of_a_rotated_widget_under_a_shrunk_canvas() {
    for sheared in [true, false] {
        let mut turn = if sheared {
            TransAffine::new_skewing(0.5, 0.0)
        } else {
            TransAffine::new_rotation(std::f64::consts::PI / 6.0)
        };
        turn.multiply(&TransAffine::new_translation(100.0, 100.0));
        let turned = sw("turned", 0.0, 0.0, 100.0, 100.0)
            .with_child(Box::new(sw("target", 0.0, 0.0, 20.0, 20.0)));
        let holder = sw("holder", 0.0, 0.0, 600.0, 400.0)
            .with_child_transform(turn)
            .with_child(Box::new(turned));
        let root = build_tree(
            scale_then_move(0.5, 0.5, 0.0, 0.0),
            Size::new(600.0, 400.0),
            Some(holder),
        );
        let found = find_on_screen(&root, "target", VIEWPORT);
        assert_eq!(
            found.map(|hit| hit.handle.path().to_vec()),
            Some(vec![0, 0, 0, 0, 0]),
            "sheared: {sheared}"
        );
    }
}

/// A child of a scaled, flipped or turned canvas is drawn whole: its clip is
/// where the canvas draws it, not the canvas's scale applied twice.
#[test]
fn a_child_of_a_transformed_canvas_is_drawn_whole() {
    let rows = [
        ("doubled", scale_then_move(2.0, 2.0, 5.0, 5.0), 1600.0),
        ("halved", scale_then_move(0.5, 0.5, 5.0, 5.0), 100.0),
        ("flipped", scale_then_move(-1.0, 1.0, 100.0, 50.0), 400.0),
        (
            "quarter turned",
            turn_then_move(std::f64::consts::FRAC_PI_2, 100.0, 50.0),
            400.0,
        ),
        (
            "turned 30 degrees",
            turn_then_move(std::f64::consts::PI / 6.0, 100.0, 50.0),
            400.0,
        ),
    ];
    for (name, transform, drawn_area) in rows {
        let red = Color::rgba(1.0, 0.0, 0.0, 1.0);
        let canvas = sw("canvas", 0.0, 0.0, 200.0, 200.0)
            .with_child_transform(transform)
            .with_child(Box::new(sw("red", 10.0, 10.0, 20.0, 20.0).filled(red)));
        let mut root = sw("root", 0.0, 0.0, 200.0, 200.0).with_child(Box::new(canvas));
        root.layout(Size::new(200.0, 200.0));

        let mut fb = Framebuffer::new(200, 200);
        {
            let mut ctx = GfxCtx::new(&mut fb);
            ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
            paint_subtree(&mut root, &mut ctx);
        }
        let red_pixels = fb
            .pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 128 && p[1] < 128)
            .count() as f64;
        assert!(
            (red_pixels - drawn_area).abs() <= drawn_area * 0.1,
            "the {name} canvas draws its 20 x 20 child over {drawn_area} pixels, got {red_pixels}"
        );
    }
}
