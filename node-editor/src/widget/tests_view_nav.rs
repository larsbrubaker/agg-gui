//! Unit tests for `widget/view_nav.rs` — the interaction-mode switch and
//! the animated fit-to-content operation, plus the drag semantics each
//! mode binds to the left button.
//!
//! Shares the `Memory` model fixture with the other test modules via
//! [`super::tests_common`].

use super::tests_common::{fixture_with_typed_handle, mk_node, seed_nodes};
use super::view_nav::{content_bounds, fit_view, FIT_PADDING};
use super::*;

use agg_gui::{Event, Modifiers, MouseButton, Point};

/// Pane the fixture editor is laid out at.
const PANE: Size = Size {
    width: 400.0,
    height: 300.0,
};

fn editor_with_nodes(nodes: Vec<crate::model::NodeView>) -> NodeEditor {
    let (shared, typed) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(shared);
    seed_nodes(&mut editor, &typed, nodes);
    editor
}

fn mouse_down(editor: &mut NodeEditor, x: f64, y: f64) {
    editor.on_event(&Event::MouseDown {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

fn mouse_move(editor: &mut NodeEditor, x: f64, y: f64) {
    editor.on_event(&Event::MouseMove {
        pos: Point::new(x, y),
    });
}

fn mouse_up(editor: &mut NodeEditor, x: f64, y: f64) {
    editor.on_event(&Event::MouseUp {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

/// Bounds are the union of node rectangles, and Y-up means
/// `top_left[1]` is the TOP edge — the max, not the min.
#[test]
fn content_bounds_unions_every_node_rect_y_up() {
    let editor = editor_with_nodes(vec![
        mk_node(1, "a", [0.0, 100.0]),
        mk_node(2, "b", [500.0, 400.0]),
    ]);
    let layouts = editor.snapshot_layouts();
    let (min_x, min_y, max_x, max_y) = content_bounds(&layouts).expect("two nodes have bounds");
    let w = layouts[0].size[0];
    assert_eq!(min_x, 0.0);
    assert_eq!(max_x, 500.0 + w);
    assert_eq!(max_y, 400.0, "top edge of the higher node");
    assert!(
        min_y < 100.0,
        "bottom edge sits below the lower node's top ({min_y})"
    );
}

/// An empty graph has nothing to frame — the home button is inert.
#[test]
fn fit_is_a_no_op_on_an_empty_graph() {
    let mut editor = editor_with_nodes(vec![]);
    assert!(editor.target_fit_view().is_none());
    assert!(!editor.fit_to_content());
    assert!(!editor.is_view_animating());
}

/// The scale is NodeDesigner's `min(w/(bw+100), h/(bh+100))` and the
/// offset puts the bounds' centre at the pane's centre.
#[test]
fn fit_view_matches_nodedesigners_formula() {
    // 200 × 100 bounds, origin-anchored, in a 400 × 300 pane.
    let bounds = (0.0, 0.0, 200.0, 100.0);
    let (scale, offset) = fit_view(bounds, 400.0, 300.0);
    let pad = FIT_PADDING * 2.0;
    let expected = (400.0f64 / (200.0 + pad)).min(300.0 / (100.0 + pad));
    assert!((scale - expected).abs() < 1e-12, "scale {scale}");
    // Centre of the bounds maps to the centre of the pane.
    assert!((100.0 * scale + offset[0] - 200.0).abs() < 1e-9);
    assert!((50.0 * scale + offset[1] - 150.0).abs() < 1e-9);
}

/// Zoom stays inside the editor's limits even for a single tiny node in
/// a large pane (which would otherwise want a huge scale).
#[test]
fn fit_view_clamps_to_the_editors_zoom_limits() {
    let (scale, _) = fit_view((0.0, 0.0, 0.0, 0.0), 100_000.0, 100_000.0);
    assert_eq!(scale, ZOOM_MAX);
    let (scale, _) = fit_view((0.0, 0.0, 1e9, 1e9), 400.0, 300.0);
    assert_eq!(scale, ZOOM_MIN);
}

/// The animation eases from the current view to the fit target over
/// `FIT_ANIM_MS`, and `layout()` is what advances it.
#[test]
fn fit_animates_from_the_current_view_towards_the_target() {
    let mut editor = editor_with_nodes(vec![mk_node(1, "a", [0.0, 0.0])]);
    editor.set_view(3.0, [999.0, -999.0]);
    let (target_scale, target_offset) = editor.target_fit_view().expect("one node has a target");

    assert!(editor.fit_to_content());
    assert!(editor.is_view_animating());
    // First tick: barely any time has passed, so the view is still
    // essentially where it started — the ease is *in*, not a jump.
    editor.layout(PANE);
    assert!(
        (editor.scale() - 3.0).abs() < 0.5,
        "eased start, got {}",
        editor.scale()
    );

    // Force the tween to its end by rewinding the clock: pump layouts
    // until the animation reports done (500 ms of real frames).
    let deadline = web_time::Instant::now() + std::time::Duration::from_millis(2_000);
    while editor.is_view_animating() && web_time::Instant::now() < deadline {
        editor.layout(PANE);
    }
    assert!(!editor.is_view_animating(), "animation must terminate");
    assert!((editor.scale() - target_scale).abs() < 1e-9);
    assert!((editor.pan()[0] - target_offset[0]).abs() < 1e-9);
    assert!((editor.pan()[1] - target_offset[1]).abs() < 1e-9);
}

/// `SetView` is the restore path: instant, clamped, and mirrored to the
/// model's hooks so a host tracking the view sees it.
#[test]
fn set_view_applies_instantly_and_reports_to_the_model() {
    let (shared, typed) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(shared);
    seed_nodes(&mut editor, &typed, vec![mk_node(1, "a", [0.0, 0.0])]);

    editor.set_view(1.75, [-40.0, 25.0]);
    assert_eq!(editor.scale(), 1.75);
    assert_eq!(editor.pan(), [-40.0, 25.0]);
    assert!(!editor.is_view_animating());
    assert_eq!(typed.lock().unwrap().zoom, 1.75);
    assert_eq!(typed.lock().unwrap().pan, [-40.0, 25.0]);

    // Out-of-range zoom is clamped rather than rejected.
    editor.set_view(500.0, [0.0, 0.0]);
    assert_eq!(editor.scale(), ZOOM_MAX);
}

/// Pan mode rebinds the left button: a drag that starts *on a node*
/// pans the canvas and leaves the node where it was.
#[test]
fn pan_mode_pans_on_left_drag_and_suppresses_node_dragging() {
    let (shared, typed) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(shared);
    seed_nodes(&mut editor, &typed, vec![mk_node(1, "a", [0.0, 200.0])]);
    editor.set_interaction_mode(InteractionMode::Pan);
    assert_eq!(editor.interaction_mode(), InteractionMode::Pan);

    // (10, 190) is inside the node body (top-left [0, 200], Y-up).
    mouse_down(&mut editor, 10.0, 190.0);
    mouse_move(&mut editor, 60.0, 220.0);
    mouse_up(&mut editor, 60.0, 220.0);

    assert_eq!(editor.pan(), [50.0, 30.0], "canvas panned by the drag");
    assert_eq!(
        typed.lock().unwrap().nodes[0].position,
        [0.0, 200.0],
        "the node under the press must not move"
    );
    assert!(editor.selected_ids().is_empty(), "pan mode does not select");
}

/// Zoom mode: dragging up zooms in, and the canvas point under the press
/// stays under the press.
#[test]
fn zoom_mode_scales_about_the_press_point() {
    let mut editor = editor_with_nodes(vec![mk_node(1, "a", [0.0, 0.0])]);
    editor.set_view(1.0, [0.0, 0.0]);
    editor.set_interaction_mode(InteractionMode::Zoom);

    let press = Point::new(120.0, 80.0);
    mouse_down(&mut editor, press.x, press.y);
    // 40 px UP the screen in Y-up coords.
    mouse_move(&mut editor, press.x, press.y + 40.0);

    let expected_scale = (40.0f64 * 0.005).exp();
    assert!(
        (editor.scale() - expected_scale).abs() < 1e-9,
        "scale {} vs {}",
        editor.scale(),
        expected_scale
    );
    // Anchor: canvas (120, 80) at scale 1 must still land on the press.
    assert!((120.0 * editor.scale() + editor.pan()[0] - press.x).abs() < 1e-9);
    assert!((80.0 * editor.scale() + editor.pan()[1] - press.y).abs() < 1e-9);

    mouse_up(&mut editor, press.x, press.y + 40.0);
    // Dragging down from the same press zooms back out below 1.
    mouse_down(&mut editor, press.x, press.y);
    mouse_move(&mut editor, press.x, press.y - 40.0);
    assert!(editor.scale() < expected_scale);
}

/// Select mode is unchanged — the node still drags.
#[test]
fn select_mode_still_drags_nodes() {
    let (shared, typed) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(shared);
    seed_nodes(&mut editor, &typed, vec![mk_node(1, "a", [0.0, 200.0])]);
    assert_eq!(editor.interaction_mode(), InteractionMode::Select);

    mouse_down(&mut editor, 10.0, 190.0);
    mouse_move(&mut editor, 60.0, 220.0);
    mouse_up(&mut editor, 60.0, 220.0);
    assert_eq!(typed.lock().unwrap().nodes[0].position, [50.0, 230.0]);
}

/// Middle-drag pans whatever the mode is.
#[test]
fn middle_drag_pans_in_every_mode() {
    for mode in [
        InteractionMode::Select,
        InteractionMode::Pan,
        InteractionMode::Zoom,
    ] {
        let mut editor = editor_with_nodes(vec![mk_node(1, "a", [0.0, 0.0])]);
        editor.set_interaction_mode(mode);
        editor.on_event(&Event::MouseDown {
            pos: Point::new(10.0, 10.0),
            button: MouseButton::Middle,
            modifiers: Modifiers::default(),
        });
        mouse_move(&mut editor, 30.0, 25.0);
        assert_eq!(editor.pan(), [20.0, 15.0], "mode {mode:?}");
    }
}

/// A user who touches the view mid-tween wins: the animation is
/// abandoned where it is, not resumed. Without the cancel the tween
/// overwrites every drag frame from `layout()` and then snaps to the fit
/// target when its 500 ms are up.
#[test]
fn a_middle_drag_during_the_fit_cancels_it() {
    let mut editor = editor_with_nodes(vec![mk_node(1, "a", [0.0, 0.0])]);
    editor.set_view(3.0, [900.0, -900.0]);
    assert!(editor.fit_to_content());

    editor.on_event(&Event::MouseDown {
        pos: Point::new(10.0, 10.0),
        button: MouseButton::Middle,
        modifiers: Modifiers::default(),
    });
    assert!(
        !editor.is_view_animating(),
        "the press must abandon the tween"
    );
    mouse_move(&mut editor, 40.0, 30.0);
    let panned = editor.pan();
    assert_eq!(panned, [930.0, -880.0], "the drag owns the view");

    // Frames past the tween's would-be end change nothing.
    let deadline = web_time::Instant::now() + std::time::Duration::from_millis(600);
    while web_time::Instant::now() < deadline {
        editor.layout(PANE);
    }
    assert_eq!(editor.pan(), panned, "no snap to the fit target");
    assert_eq!(editor.scale(), 3.0);
}

/// Same contract for the wheel, which is always live whatever the mode.
#[test]
fn a_wheel_during_the_fit_cancels_it() {
    let mut editor = editor_with_nodes(vec![mk_node(1, "a", [0.0, 0.0])]);
    editor.set_view(1.0, [0.0, 0.0]);
    assert!(editor.fit_to_content());

    editor.on_event(&Event::MouseWheel {
        pos: Point::new(50.0, 50.0),
        delta_x: 0.0,
        delta_y: 1.0,
        modifiers: Modifiers::default(),
    });
    assert!(!editor.is_view_animating(), "the wheel abandons the tween");
    let zoomed = editor.scale();
    assert!(zoomed > 1.0);

    let deadline = web_time::Instant::now() + std::time::Duration::from_millis(600);
    while web_time::Instant::now() < deadline {
        editor.layout(PANE);
    }
    assert_eq!(editor.scale(), zoomed, "no snap to the fit target");
}

/// A left-drag in Pan / Zoom mode cancels it too — those are the other
/// two ways a user moves the view.
#[test]
fn a_left_drag_in_pan_or_zoom_mode_cancels_the_fit() {
    for mode in [InteractionMode::Pan, InteractionMode::Zoom] {
        let mut editor = editor_with_nodes(vec![mk_node(1, "a", [0.0, 0.0])]);
        editor.set_view(2.0, [400.0, 400.0]);
        editor.set_interaction_mode(mode);
        assert!(editor.fit_to_content(), "mode {mode:?}");
        mouse_down(&mut editor, 30.0, 30.0);
        assert!(!editor.is_view_animating(), "mode {mode:?}");
    }
}

/// A NaN view is not a view: `SetView` must leave the canvas alone
/// rather than poison it (`f64::clamp` passes NaN straight through).
#[test]
fn a_non_finite_set_view_is_ignored() {
    let mut editor = editor_with_nodes(vec![mk_node(1, "a", [0.0, 0.0])]);
    editor.set_view(1.25, [10.0, 20.0]);

    editor.set_view(f64::NAN, [1.0, 2.0]);
    editor.set_view(1.0, [f64::NAN, 2.0]);
    editor.set_view(f64::INFINITY, [1.0, 2.0]);
    editor.set_view(1.0, [1.0, f64::NEG_INFINITY]);

    assert_eq!(editor.scale(), 1.25, "scale survived");
    assert_eq!(editor.pan(), [10.0, 20.0], "offset survived");
}

/// Switching mode mid-gesture drops the drag — and must clean up after
/// it exactly as a mouse-up would, or the guides drawn during a node
/// drag stay on screen (they live in a thread-local registry that the
/// paint cache's fingerprint knows nothing about).
#[test]
fn a_mode_switch_mid_drag_clears_the_snap_guides() {
    let (shared, typed) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(shared);
    seed_nodes(
        &mut editor,
        &typed,
        vec![
            mk_node(1, "a", [0.0, 200.0]),
            mk_node(2, "b", [300.0, 200.0]),
        ],
    );
    agg_gui::snap::set_enabled(true);
    mouse_down(&mut editor, 10.0, 190.0);
    mouse_move(&mut editor, 14.0, 191.0);
    agg_gui::snap::set_guides(vec![agg_gui::snap::SnapGuide::VLine {
        x: 0.0,
        y0: 0.0,
        y1: 100.0,
    }]);

    editor.set_interaction_mode(InteractionMode::Pan);
    assert!(
        agg_gui::snap::guides_snapshot().is_empty(),
        "the abandoned drag's guides must go with it"
    );
    // …and the drag really is gone: a move no longer drags the node.
    let before = typed.lock().unwrap().nodes[0].position;
    mouse_move(&mut editor, 200.0, 300.0);
    assert_eq!(typed.lock().unwrap().nodes[0].position, before);
}

/// The host command queue reaches all three new operations.
#[test]
fn commands_drive_mode_view_and_fit() {
    let (shared, typed) = fixture_with_typed_handle();
    let handle = NodeEditorHandle::new();
    let mut editor = NodeEditor::new(shared).with_command_handle(handle.clone());
    seed_nodes(&mut editor, &typed, vec![mk_node(1, "a", [0.0, 0.0])]);

    handle.push(NodeEditorCommand::SetInteractionMode(InteractionMode::Zoom));
    handle.push(NodeEditorCommand::SetView {
        scale: 0.5,
        offset: [12.0, 34.0],
    });
    editor.layout(PANE);
    assert_eq!(editor.interaction_mode(), InteractionMode::Zoom);
    assert_eq!(editor.scale(), 0.5);
    assert_eq!(editor.pan(), [12.0, 34.0]);

    handle.push(NodeEditorCommand::FitToContent);
    editor.layout(PANE);
    assert!(editor.is_view_animating());
}
