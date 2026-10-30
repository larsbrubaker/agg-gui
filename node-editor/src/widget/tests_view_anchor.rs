//! Unit tests for `widget/view_anchor.rs`: the coordinate mapping, the
//! centre anchor and its resize, the drop ring, and NodeDesigner's
//! `CenterNodesInView` with its pending first-draw centring, and the
//! readable first view that opens a big graph from its top-left.
//! Shares the `Memory` model fixture via [`super::tests_common`].

use super::tests_common::{fixture_with_typed_handle, mk_node, seed_nodes, Memory};
use super::view_nav::content_bounds;
use super::*;
use agg_gui::{Modifiers, MouseButton, Point};

fn editor_with(nodes: Vec<crate::model::NodeView>) -> (NodeEditor, Arc<Mutex<Memory>>) {
    let (shared, typed) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(shared);
    seed_nodes(&mut editor, &typed, nodes);
    (editor, typed)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn screen_and_canvas_mapping_round_trip() {
    let (mut editor, _typed) = editor_with(vec![]);
    assert!(editor.set_view(2.0, [10.0, -20.0]));
    let c = editor.screen_to_canvas(Point::new(30.0, 40.0));
    assert_eq!(c, [10.0, 30.0]);
    let p = editor.canvas_to_screen(c);
    assert_eq!((p.x, p.y), (30.0, 40.0));
}

#[test]
fn app_to_canvas_goes_through_the_app_origin() {
    let (editor, _typed) = editor_with(vec![]);
    editor.last_abs_origin.set((100.0, 50.0));
    assert_eq!(editor.app_to_canvas(Point::new(130.0, 90.0)), [30.0, 40.0]);
}

#[test]
fn origin_anchor_is_the_default_and_keeps_the_bottom_left_on_resize() {
    let (mut editor, _typed) = editor_with(vec![]);
    assert_eq!(editor.view_anchor(), ViewAnchor::Origin);
    editor.set_view(1.0, [10.0, 20.0]);
    editor.layout(Size::new(600.0, 500.0));
    assert_eq!(editor.pan(), [10.0, 20.0]);
    assert_eq!(editor.screen_to_canvas(Point::new(10.0, 20.0)), [0.0, 0.0]);
}

#[test]
fn center_anchor_measures_the_pan_from_the_middle_and_keeps_it_on_resize() {
    let (shared, typed) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(shared).with_view_anchor(ViewAnchor::Center);
    seed_nodes(&mut editor, &typed, vec![]);
    // A zero pan puts the canvas origin at the centre, as NodeDesigner does.
    assert_eq!(editor.pan(), [0.0, 0.0]);
    let p = editor.canvas_to_screen([0.0, 0.0]);
    assert_eq!((p.x, p.y), (200.0, 150.0));

    editor.set_view(1.0, [10.0, -5.0]);
    assert_eq!(typed.lock().unwrap().pan, [10.0, -5.0]);
    editor.layout(Size::new(600.0, 500.0));
    assert_eq!(editor.pan(), [10.0, -5.0]);
    let p = editor.canvas_to_screen([0.0, 0.0]);
    assert_eq!((p.x, p.y), (310.0, 245.0));
}

#[test]
fn drop_feedback_toggles() {
    let (mut editor, _typed) = editor_with(vec![]);
    assert!(!editor.drop_feedback());
    editor.set_drop_feedback(true);
    assert!(editor.drop_feedback());
    editor.set_drop_feedback(false);
    assert!(!editor.drop_feedback());
}

#[test]
fn center_nodes_in_view_ports_nodedesigners_formula() {
    let (mut editor, _typed) = editor_with(vec![
        mk_node(1, "a", [0.0, 0.0]),
        mk_node(2, "b", [600.0, 100.0]),
    ]);
    let (min_x, min_y, max_x, max_y) = content_bounds(&editor.snapshot_layouts()).unwrap();
    assert!(editor.center_nodes_in_view());
    // fit = min((300 - 30 - 40) / bh, (400 - 30) / bw), at most 1.
    let fit = ((300.0 - 30.0 - 40.0) / (max_y - min_y)).min((400.0 - 30.0) / (max_x - min_x));
    let s = fit.min(1.0);
    assert!(close(editor.scale(), s));
    // U = -centre + (0, 20) * s; local = (canvas + U) * s + size / 2.
    let u = [-(min_x + max_x) / 2.0, -(min_y + max_y) / 2.0 + 20.0 * s];
    let o = editor.pan();
    assert!(close(o[0], u[0] * s + 200.0));
    assert!(close(o[1], u[1] * s + 150.0));
}

/// NodeDesigner's margins are device pixels: on a 2x display the graph keeps
/// half as many logical points of margin, so the fit matches C# there.
#[test]
fn center_nodes_in_view_margins_are_device_pixels() {
    let (mut editor, _typed) = editor_with(vec![
        mk_node(1, "a", [0.0, 0.0]),
        mk_node(2, "b", [600.0, 100.0]),
    ]);
    let (min_x, min_y, max_x, max_y) = content_bounds(&editor.snapshot_layouts()).unwrap();
    agg_gui::set_device_scale(2.0);
    let centred = editor.center_nodes_in_view();
    agg_gui::set_device_scale(1.0);
    assert!(centred);
    // fit = min((300 - 15 - 20) / bh, (400 - 15) / bw): 30 and 20 device pixels at 2x.
    let fit = ((300.0 - 15.0 - 20.0) / (max_y - min_y)).min((400.0 - 15.0) / (max_x - min_x));
    let s = fit.min(1.0);
    assert!(close(editor.scale(), s));
    let u = [-(min_x + max_x) / 2.0, -(min_y + max_y) / 2.0 + 10.0 * s];
    let o = editor.pan();
    assert!(close(o[0], u[0] * s + 200.0));
    assert!(close(o[1], u[1] * s + 150.0));
}

#[test]
fn center_nodes_in_view_caps_the_zoom_at_100_percent() {
    let (mut editor, _typed) = editor_with(vec![mk_node(1, "a", [0.0, 0.0])]);
    assert!(editor.center_nodes_in_view());
    assert_eq!(editor.scale(), 1.0);
}

#[test]
fn center_nodes_in_view_leaves_an_empty_graph_or_a_short_panel_alone() {
    let (mut editor, _typed) = editor_with(vec![]);
    assert!(!editor.center_nodes_in_view());
    let (mut editor, _typed) = editor_with(vec![mk_node(1, "a", [0.0, 0.0])]);
    editor.layout(Size::new(400.0, 60.0));
    editor.set_view(1.0, [5.0, 5.0]);
    assert!(!editor.center_nodes_in_view());
    assert_eq!(editor.pan(), [5.0, 5.0]);
}

#[test]
fn a_pending_centre_retries_until_the_panel_is_tall_enough() {
    let (mut editor, _typed) = editor_with(vec![mk_node(1, "a", [0.0, 0.0])]);
    editor.request_center_on_draw();
    editor.layout(Size::new(400.0, 60.0));
    assert!(editor.is_center_pending(), "too short: still pending");
    assert_eq!(editor.pan(), [0.0, 0.0]);
    editor.layout(Size::new(400.0, 300.0));
    assert!(!editor.is_center_pending());
    assert_ne!(editor.pan(), [0.0, 0.0]);
}

#[test]
fn a_user_pan_cancels_the_pending_centre() {
    let (mut editor, _typed) = editor_with(vec![]);
    editor.request_center_on_draw();
    editor.on_event(&Event::MouseDown {
        pos: Point::new(100.0, 100.0),
        button: MouseButton::Middle,
        modifiers: Modifiers::default(),
    });
    assert!(!editor.is_center_pending());
}

#[test]
fn a_wheel_cancels_the_pending_centre() {
    let (mut editor, _typed) = editor_with(vec![]);
    editor.request_center_on_draw();
    editor.on_event(&Event::MouseWheel {
        pos: Point::new(100.0, 100.0),
        delta_x: 0.0,
        delta_y: 1.0,
        modifiers: Modifiers::default(),
    });
    assert!(!editor.is_center_pending());
}

/// The minimum readable zoom the readable-view tests pass in.
const READABLE: f64 = 0.8;

#[test]
fn a_big_graph_opens_at_the_readable_minimum_from_its_top_left() {
    let (mut editor, _typed) = editor_with(vec![
        mk_node(1, "a", [0.0, 0.0]),
        mk_node(2, "b", [3000.0, -2000.0]),
    ]);
    let (min_x, _min_y, _max_x, max_y) = content_bounds(&editor.snapshot_layouts()).unwrap();
    assert!(editor.show_nodes_readable(READABLE));
    assert!(close(editor.scale(), READABLE));
    // The left edge at half the 30 px margin; the top at that and the 20 px
    // band below the 300 px editor's top.
    let left = editor.canvas_to_screen([min_x, max_y]);
    assert!(close(left.x, 15.0), "left edge at {}", left.x);
    assert!(close(left.y, 300.0 - 15.0 - 20.0), "top edge at {}", left.y);
}

#[test]
fn a_small_graph_still_opens_fitted_and_centred() {
    let nodes = || vec![mk_node(1, "a", [0.0, 0.0]), mk_node(2, "b", [150.0, 40.0])];
    let (mut fitted, _t1) = editor_with(nodes());
    assert!(fitted.center_nodes_in_view());
    let (mut readable, _t2) = editor_with(nodes());
    assert!(readable.show_nodes_readable(READABLE));
    assert!(readable.scale() >= READABLE);
    assert_eq!(readable.scale(), fitted.scale());
    assert_eq!(readable.pan(), fitted.pan());
}

#[test]
fn a_graph_whose_fit_is_readable_is_fitted_whole() {
    // Fits at about 0.9 of full size: readable, so not the 0.8 minimum.
    let (mut editor, _typed) = editor_with(vec![mk_node(1, "a", [0.0, 0.0])]);
    let (min_x, _, max_x, _) = content_bounds(&editor.snapshot_layouts()).unwrap();
    editor.layout(Size::new(30.0 + (max_x - min_x) * 0.9, 300.0));
    assert!(editor.show_nodes_readable(READABLE));
    assert!(close(editor.scale(), 0.9), "scale {}", editor.scale());
}

#[test]
fn a_wide_short_graph_is_centred_vertically_and_anchored_left() {
    let (mut editor, _typed) = editor_with(vec![
        mk_node(1, "a", [0.0, 0.0]),
        mk_node(2, "b", [3000.0, 0.0]),
    ]);
    let (min_x, min_y, _max_x, max_y) = content_bounds(&editor.snapshot_layouts()).unwrap();
    assert!(
        (max_y - min_y) * READABLE < 300.0 - 30.0 - 40.0,
        "short enough to fit"
    );
    assert!(editor.show_nodes_readable(READABLE));
    assert!(close(editor.scale(), READABLE));
    let left = editor.canvas_to_screen([min_x, 0.0]);
    assert!(close(left.x, 15.0), "left edge at {}", left.x);
    // Vertically as the home button centres: the middle at h / 2 plus
    // NodeDesigner's 20 px band, scaled twice.
    let middle = editor.canvas_to_screen([0.0, (min_y + max_y) * 0.5]);
    assert!(
        close(middle.y, 150.0 + 20.0 * READABLE * READABLE),
        "middle at {}",
        middle.y
    );
}

#[test]
fn a_readable_minimum_near_100_percent_snaps_to_it() {
    let (mut editor, _typed) = editor_with(vec![
        mk_node(1, "a", [0.0, 0.0]),
        mk_node(2, "b", [3000.0, -2000.0]),
    ]);
    assert!(editor.show_nodes_readable(0.97));
    assert_eq!(editor.scale(), 1.0);
}

#[test]
fn a_pending_readable_view_waits_for_room_then_opens_readable() {
    let (mut editor, _typed) = editor_with(vec![
        mk_node(1, "a", [0.0, 0.0]),
        mk_node(2, "b", [3000.0, -2000.0]),
    ]);
    editor.request_readable_on_draw(READABLE);
    editor.layout(Size::new(400.0, 60.0));
    assert!(editor.is_center_pending(), "too short: still pending");
    editor.layout(Size::new(400.0, 300.0));
    assert!(!editor.is_center_pending());
    assert!(close(editor.scale(), READABLE));
}

#[test]
fn a_pending_centre_after_a_readable_request_fits_the_whole_graph() {
    let (mut editor, _typed) = editor_with(vec![
        mk_node(1, "a", [0.0, 0.0]),
        mk_node(2, "b", [3000.0, -2000.0]),
    ]);
    editor.request_readable_on_draw(READABLE);
    editor.request_center_on_draw();
    editor.layout(Size::new(400.0, 300.0));
    assert!(editor.scale() < READABLE);
}
