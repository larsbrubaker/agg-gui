//! Core tests for `NodeEditor` — backbuffer invalidation, drag/snap,
//! layout, and identity. Originally extracted from `mod.rs`; further
//! split into sibling modules (`tests_common`, `tests_overlay`,
//! `tests_noodle`) so each file stays under the project's 800-line
//! cap. Uses `use super::*` so it still reaches private fields/methods
//! (canvas_offset, canvas_scale, local_to_canvas) the way the inline
//! tests did.

use super::tests_common::{fixture, fixture_with_typed_handle, mk_node, seed_nodes};
use super::*;
use agg_gui::{Modifiers, MouseButton, Point};

#[test]
fn editor_exposes_node_children_after_layout() {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(
        &mut editor,
        &memory,
        vec![
            mk_node(1, "Extrude", [50.0, 50.0]),
            mk_node(2, "Output", [250.0, 50.0]),
        ],
    );

    let children = editor.children();
    assert_eq!(
        children.len(),
        2,
        "NodeEditor must expose one child Widget per model node so the inspector sees them"
    );
    let names: Vec<_> = children.iter().map(|c| c.type_name()).collect();
    assert!(
        names.iter().all(|n| *n == "NodeWidget"),
        "every child of NodeEditor must be a NodeWidget; got {names:?}"
    );
}

#[test]
fn editor_advertises_gl_fbo_backbuffer() {
    let mut editor = NodeEditor::new(fixture());
    let spec = editor.backbuffer_spec();
    assert!(
        !matches!(spec.kind, agg_gui::BackbufferKind::None),
        "NodeEditor must opt into a hardware backbuffer (GL FBO) so the host can cache its render across frames; got kind={:?}",
        spec.kind
    );
}

#[test]
fn editor_advertises_backbuffer_state() {
    let mut editor = NodeEditor::new(fixture());
    assert!(
        editor.backbuffer_state_mut().is_some(),
        "NodeEditor with a hardware backbuffer must return Some from backbuffer_state_mut so the framework can track its dirty / size state"
    );
}

#[test]
fn adding_a_node_invalidates_the_backbuffer() {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(
        &mut editor,
        &memory,
        vec![mk_node(1, "A", [0.0, 0.0]), mk_node(2, "B", [100.0, 0.0])],
    );
    if let Some(state) = editor.backbuffer_state_mut() {
        state.dirty = false;
    }

    seed_nodes(
        &mut editor,
        &memory,
        vec![
            mk_node(1, "A", [0.0, 0.0]),
            mk_node(2, "B", [100.0, 0.0]),
            mk_node(3, "C", [200.0, 0.0]),
        ],
    );
    let dirty = editor
        .backbuffer_state_mut()
        .map(|s| s.dirty)
        .unwrap_or(false);
    assert!(
        dirty,
        "a model change (new node) must invalidate the editor's retained backbuffer so the next paint regenerates the texture"
    );
    assert_eq!(editor.children().len(), 3);
}

/// Regression: switching the global theme (light ↔ dark) must
/// invalidate the cached children-widget tree so the next layout
/// rebuilds every NodeWidget against the fresh `CanvasPalette`.
/// Without this the chrome (body fill, border, label colours,
/// connector socket strokes) stays at the old palette's colours
/// after a theme flip — visually the nodes "stay light in dark
/// mode" even though `current_visuals()` has already swapped.
#[test]
fn theme_change_invalidates_paint_fingerprint() {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(
        &mut editor,
        &memory,
        vec![mk_node(1, "Extrude", [50.0, 50.0])],
    );

    let layouts = editor.snapshot_layouts();
    agg_gui::set_visuals(agg_gui::Visuals::light());
    let fp_light = editor.compute_fingerprint(&layouts, None);
    agg_gui::set_visuals(agg_gui::Visuals::dark());
    let fp_dark = editor.compute_fingerprint(&layouts, None);
    assert_ne!(
        fp_light, fp_dark,
        "compute_fingerprint must include the theme epoch — otherwise a light↔dark switch reuses the cached node-widget tree built against the old palette and the canvas paints stale colours",
    );
}

#[test]
fn moving_a_node_invalidates_the_backbuffer() {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "A", [0.0, 0.0])]);
    if let Some(state) = editor.backbuffer_state_mut() {
        state.dirty = false;
    }

    seed_nodes(&mut editor, &memory, vec![mk_node(1, "A", [50.0, 50.0])]);
    let dirty = editor
        .backbuffer_state_mut()
        .map(|s| s.dirty)
        .unwrap_or(false);
    assert!(
        dirty,
        "moving an existing node must invalidate the editor's retained backbuffer"
    );
}

#[test]
fn unchanged_model_does_not_invalidate_the_backbuffer() {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "A", [0.0, 0.0])]);
    if let Some(state) = editor.backbuffer_state_mut() {
        state.dirty = false;
    }
    editor.layout(Size::new(400.0, 300.0));
    let dirty = editor
        .backbuffer_state_mut()
        .map(|s| s.dirty)
        .unwrap_or(false);
    assert!(
        !dirty,
        "re-laying out with an unchanged model must NOT invalidate the backbuffer"
    );
}

#[test]
fn dragging_with_snap_enabled_aligns_left_edge_to_neighbour() {
    // End-to-end regression: with the framework's snap toggle ON,
    // dragging a node within `SNAP_DEFAULT_THRESHOLD` of another
    // node's left edge should pull the dragged node into alignment.
    //
    // Place A and B at the same X but very different Y so the only
    // candidate snap is the LEFT edge — Y edges are 200 px apart
    // and well outside the 8-px threshold.
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0));
    seed_nodes(
        &mut editor,
        &memory,
        vec![
            mk_node(1, "A", [50.0, 300.0]),
            mk_node(2, "B", [400.0, 100.0]),
        ],
    );

    agg_gui::snap::set_enabled(true);
    editor.interaction = CanvasState::DraggingNode {
        ids: vec![NodeId(1)],
        start_positions: vec![[50.0, 300.0]],
        start_canvas: [50.0, 300.0],
    };
    // Move cursor so the un-snapped new position is x=403 — 3 px off
    // node B's left edge (400).  Engine should pull node A's left
    // edge into 400.
    editor.on_mouse_move(Point::new(403.0, 300.0));

    let pos = memory.lock().unwrap().nodes[0].position;
    assert!(
        (pos[0] - 400.0).abs() < 1e-6,
        "snap should align A.left to B.left (400); got x={}",
        pos[0]
    );

    // Reset the thread-local flag so sibling tests within this
    // thread don't inherit our toggle.  (Other snap-aware tests in
    // this file are defensive and explicitly set the flag they need.)
    agg_gui::snap::set_enabled(false);
    agg_gui::snap::clear_guides();
}

#[test]
fn drag_release_clears_guides_and_invalidates_backbuffer() {
    // After a snap engages, the user releases the drag.  The guide
    // list must be empty AND the canvas's backbuffer must be marked
    // dirty so the next paint re-rasters without the alignment line
    // — otherwise the cached pixels keep showing the stale guide.
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0));
    seed_nodes(
        &mut editor,
        &memory,
        vec![
            mk_node(1, "A", [50.0, 300.0]),
            mk_node(2, "B", [400.0, 100.0]),
        ],
    );
    agg_gui::snap::set_enabled(true);
    editor.interaction = CanvasState::DraggingNode {
        ids: vec![NodeId(1)],
        start_positions: vec![[50.0, 300.0]],
        start_canvas: [50.0, 300.0],
    };
    editor.on_mouse_move(Point::new(403.0, 300.0));
    // Snap should have written a guide.
    assert!(
        !agg_gui::snap::guides_snapshot().is_empty(),
        "drag-time snap should have written guides"
    );
    // Clear the backbuffer-dirty flag so the post-mouseup check can
    // detect the invalidate.
    if let Some(state) = editor.backbuffer_state_mut() {
        state.dirty = false;
    }
    editor.on_mouse_up(
        Point::new(403.0, 300.0),
        MouseButton::Left,
        Modifiers::default(),
    );
    assert!(
        agg_gui::snap::guides_snapshot().is_empty(),
        "MouseUp must clear the snap-guide list"
    );
    let dirty = editor
        .backbuffer_state_mut()
        .map(|s| s.dirty)
        .unwrap_or(false);
    assert!(
        dirty,
        "MouseUp on a drag must invalidate the canvas backbuffer so the next paint re-rasters without the snap guide"
    );
    agg_gui::snap::set_enabled(false);
}

#[test]
fn dragging_with_snap_disabled_does_not_align() {
    // Opposite control: with snap toggle OFF, the same drag must
    // leave the node at the raw cursor position — no edge attraction.
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0));
    seed_nodes(
        &mut editor,
        &memory,
        vec![
            mk_node(1, "A", [50.0, 300.0]),
            mk_node(2, "B", [400.0, 100.0]),
        ],
    );
    agg_gui::snap::set_enabled(false);
    editor.interaction = CanvasState::DraggingNode {
        ids: vec![NodeId(1)],
        start_positions: vec![[50.0, 300.0]],
        start_canvas: [50.0, 300.0],
    };
    editor.on_mouse_move(Point::new(403.0, 300.0));
    let pos = memory.lock().unwrap().nodes[0].position;
    assert!(
        (pos[0] - 403.0).abs() < 1e-6,
        "snap OFF must leave the raw drag position untouched; got x={}",
        pos[0]
    );
}

#[test]
fn local_to_canvas_round_trip_with_pan_and_zoom() {
    let editor = {
        let mut e = NodeEditor::new(fixture());
        e.canvas_offset = [50.0, 30.0];
        e.canvas_scale = 1.5;
        e
    };
    let lp = Point::new(80.0, 60.0);
    let cp = editor.local_to_canvas(lp);
    assert!((cp[0] - (80.0 - 50.0) / 1.5).abs() < 1e-9);
    assert!((cp[1] - (60.0 - 30.0) / 1.5).abs() < 1e-9);
}

/// End-to-end: with the editor positioned at non-zero screen origin AND
/// pan/zoom active, a `collect_inspector_nodes` walk must report each
/// NodeWidget's `screen_bounds` at the actual painted pixels.
///
/// The fix is to bake the canvas transform into the NodeWidget's
/// bounds during `layout()` (rather than push `ctx.scale`/`ctx.translate`
/// in `paint()`) — the framework's per-child translate composes
/// additively in screen-space, so canvas-space child bounds can't be
/// scaled by a parent transform reliably.
#[test]
fn collect_inspector_nodes_reports_pan_zoom_baked_bounds() {
    use agg_gui::widget::collect_inspector_nodes;
    use agg_gui::Point;

    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(100.0, 200.0, 800.0, 600.0));
    editor.canvas_offset = [25.0, 40.0];
    editor.canvas_scale = 1.5;
    // Canvas-space (50, 60) → screen-relative (25 + 50*1.5, 40 + (60 - h)*1.5)
    // for the node's bottom-left.  We don't hard-code the node layout
    // dimensions; we just verify that the editor's screen origin plus the
    // child's editor-local bounds equals the inspector's reported
    // screen_bounds.
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "N", [50.0, 60.0])]);

    let nw_local = editor.children()[0].bounds();
    let mut nodes = Vec::new();
    collect_inspector_nodes(&editor, 0, Point::ORIGIN, &mut nodes);
    let node = nodes
        .iter()
        .find(|n| n.type_name == "NodeWidget")
        .expect("NodeWidget missing from inspector snapshot");

    let expected_x = editor.bounds().x + nw_local.x;
    let expected_y = editor.bounds().y + nw_local.y;
    let expected_w = nw_local.width;
    let expected_h = nw_local.height;
    let b = node.screen_bounds;
    assert!(
        (b.x - expected_x).abs() < 1e-6
            && (b.y - expected_y).abs() < 1e-6
            && (b.width - expected_w).abs() < 1e-6
            && (b.height - expected_h).abs() < 1e-6,
        "NodeWidget screen_bounds must equal editor screen origin + child's editor-local bounds; \
         expected x={expected_x} y={expected_y} w={expected_w} h={expected_h}; got {:?}",
        b
    );

    // Sanity: the editor-local bounds reflect the canvas transform.
    let s = editor.canvas_scale;
    let canvas_x = 50.0;
    let canvas_top_y = 60.0;
    let expected_nw_local_x = canvas_x * s + editor.canvas_offset[0];
    assert!(
        (nw_local.x - expected_nw_local_x).abs() < 1e-6,
        "NodeWidget editor-local x must be canvas_x * scale + offset; expected \
         {expected_nw_local_x}, got {}",
        nw_local.x
    );
    // For y, the canvas top maps to (canvas_top_y - h) * scale + offset_y at
    // the screen bottom.  We don't know h here, so verify via the child's
    // height instead.
    let nw_h = nw_local.height;
    let expected_nw_local_y = (canvas_top_y - nw_h / s) * s + editor.canvas_offset[1];
    assert!(
        (nw_local.y - expected_nw_local_y).abs() < 1e-6,
        "NodeWidget editor-local y must be (canvas_top - canvas_h) * scale + offset; \
         expected {expected_nw_local_y}, got {}",
        nw_local.y
    );
}

#[test]
fn editor_has_default_id() {
    let e = NodeEditor::new(fixture());
    assert_eq!(e.id(), Some("node-editor"));
}

#[test]
fn with_id_overrides_default() {
    let e = NodeEditor::new(fixture()).with_id("custom-canvas");
    assert_eq!(e.id(), Some("custom-canvas"));
}
