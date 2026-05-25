//! Tests for `NodeEditor` — extracted from `mod.rs` to keep the parent
//! file under the project's 800-line cap.  Uses `use super::*` so it
//! still reaches private fields/methods (canvas_offset, canvas_scale,
//! local_to_canvas) the way the inline tests did.

use super::*;
use crate::draw::{layout_node_with_connections, SocketSide};
use crate::model::{NodeTypeView, NodeView, NoodleResult, NoodleView, PropertyValue, SocketView};
use agg_gui::{Modifiers, MouseButton, Point};

/// Trivial in-memory model for unit tests.
#[derive(Default)]
struct Memory {
    nodes: Vec<NodeView>,
    noodles: Vec<NoodleView>,
    zoom: f64,
    last_selection: Option<NodeId>,
}

impl NodeGraphModel for Memory {
    fn nodes(&self) -> Vec<NodeView> {
        self.nodes.clone()
    }
    fn noodles(&self) -> Vec<NoodleView> {
        self.noodles.clone()
    }
    fn node_types_by_category(&self) -> Vec<(String, Vec<NodeTypeView>)> {
        vec![]
    }
    fn set_node_position(&mut self, id: NodeId, pos: [f64; 2]) {
        if let Some(n) = self.nodes.iter_mut().find(|n| n.id == id) {
            n.position = pos;
        }
    }
    fn add_node(&mut self, _type_id: &str, _pos: [f64; 2]) -> Option<NodeId> {
        None
    }
    fn remove_node(&mut self, id: NodeId) {
        self.nodes.retain(|n| n.id != id);
    }
    fn try_add_noodle(
        &mut self,
        from_node: NodeId,
        from_socket: &str,
        to_node: NodeId,
        to_socket: &str,
    ) -> NoodleResult {
        self.noodles.push(NoodleView {
            from_node,
            from_socket: from_socket.into(),
            to_node,
            to_socket: to_socket.into(),
        });
        NoodleResult::Connected
    }
    fn remove_noodle(
        &mut self,
        from_node: NodeId,
        from_socket: &str,
        to_node: NodeId,
        to_socket: &str,
    ) -> bool {
        let before = self.noodles.len();
        self.noodles.retain(|n| {
            !(n.from_node == from_node
                && n.from_socket == from_socket
                && n.to_node == to_node
                && n.to_socket == to_socket)
        });
        self.noodles.len() < before
    }
    fn set_property(&mut self, _id: NodeId, _name: &str, _value: PropertyValue) {}
    fn on_canvas_zoom_changed(&mut self, zoom: f64) {
        self.zoom = zoom;
    }
    fn on_primary_selection_changed(&mut self, id: Option<NodeId>) {
        self.last_selection = id;
    }
}

fn fixture() -> SharedModel {
    Arc::new(Mutex::new(Memory::default()))
}

/// Same as [`fixture`] but returns both the trait-object SharedModel
/// AND a typed Arc<Mutex<Memory>> handle so tests can mutate the
/// concrete `nodes` field (the trait surface returns owned `Vec`s
/// only, no direct mutation of the node list).
fn fixture_with_typed_handle() -> (SharedModel, Arc<Mutex<Memory>>) {
    let typed = Arc::new(Mutex::new(Memory::default()));
    let shared: SharedModel = typed.clone();
    (shared, typed)
}

/// Build a fresh `NodeView` for tests — no sockets, no properties.
fn mk_node(id: u64, name: &str, pos: [f64; 2]) -> NodeView {
    NodeView {
        id: NodeId(id),
        type_id: format!("type{id}"),
        display_name: name.to_string(),
        category: "test".to_string(),
        position: pos,
        inputs: vec![],
        outputs: vec![],
        properties: vec![],
    }
}

/// Replace the typed memory's node list, then re-layout `editor`.
fn seed_nodes(editor: &mut NodeEditor, memory: &Arc<Mutex<Memory>>, nodes: Vec<NodeView>) {
    memory.lock().unwrap().nodes = nodes;
    editor.layout(Size::new(400.0, 300.0));
}

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

    // Leave the thread-local flag in the default off-state so
    // sibling tests don't inherit our toggle.
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

// ── Overlay-sink hand-off tests ──────────────────────────────────────
//
// The sink is the channel that lets app shells (today: AtomArtist's
// `build_app`) reparent the color-picker dialog from this editor's
// pane up to a screen-level host so the user can drag it anywhere.
// The branch in `open_color_picker` is critical: with a sink installed
// the editor MUST NOT keep the dialog as `self.overlay`, otherwise the
// dialog would render twice (once here, once at the screen-level host)
// and double-handle every event.

const TEST_FONT_FOR_PICKER: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn install_test_font_once() {
    use agg_gui::font_settings::{current_system_font, set_system_font};
    use agg_gui::text::Font;
    use std::sync::Arc;
    // Tests run in parallel; setting the system font is idempotent so
    // overwriting it from multiple tests is fine — they all set the
    // same Cascadia bytes.
    if current_system_font().is_none() {
        let font = Arc::new(Font::from_slice(TEST_FONT_FOR_PICKER).unwrap());
        set_system_font(Some(font));
    }
}

/// Sink branch: with an overlay sink installed, opening the color
/// picker hands the dialog off via the callback and leaves
/// `self.overlay` unset.
#[test]
fn open_color_picker_hands_off_to_sink_when_installed() {
    use std::cell::RefCell;
    install_test_font_once();
    let (model, memory) = fixture_with_typed_handle();
    let captured: Rc<RefCell<Option<(Box<dyn Widget>, Rc<Cell<bool>>)>>> =
        Rc::new(RefCell::new(None));
    let sink_captured = Rc::clone(&captured);
    let mut editor = NodeEditor::new(model).with_overlay_sink(move |dialog, flag| {
        *sink_captured.borrow_mut() = Some((dialog, flag));
    });
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "N", [0.0, 0.0])]);

    editor.open_color_picker(NodeId(1), "Color".to_string(), [0.4, 0.6, 0.8, 1.0]);

    assert!(
        captured.borrow().is_some(),
        "sink must receive the constructed dialog when installed"
    );
    assert!(
        editor.overlay.is_none(),
        "with a sink installed the editor must NOT keep the dialog locally — that would paint it twice"
    );
    assert!(
        editor.overlay_close_flag.is_none(),
        "with a sink installed the close flag belongs to the host, not the editor"
    );
}

/// Back-compat branch: without a sink the editor still owns the
/// dialog as before (the gallery demo + standalone embedders rely on
/// this).
#[test]
fn open_color_picker_uses_local_overlay_when_no_sink() {
    install_test_font_once();
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "N", [0.0, 0.0])]);

    editor.open_color_picker(NodeId(1), "Color".to_string(), [0.4, 0.6, 0.8, 1.0]);

    assert!(
        editor.overlay.is_some(),
        "without a sink the editor MUST keep the dialog as its own overlay (legacy behaviour)"
    );
    assert!(
        editor.overlay_close_flag.is_some(),
        "without a sink the close flag is tracked locally so `drain_overlay_close` can tear the dialog down"
    );
}

// ---------------------------------------------------------------------------
// resolve_noodle_endpoints — noodle endpoint side-disambiguation
// ---------------------------------------------------------------------------

/// Regression: when a target node has both an input and an output that
/// share a name (the AtomArtist `Output` node's adopted slot + mirror
/// output pattern), the inline name-only lookup the paint loop used
/// originally would resolve the noodle's `to` endpoint to whichever
/// row came first — outputs, since `layout_node_with_connections`
/// emits output rows ahead of input rows. The visual result was a
/// noodle landing on the wrong side of the node (see screenshot in the
/// bug report). The resolver now side-restricts the lookup; this test
/// pins both halves of that fix.
#[test]
fn resolve_noodle_endpoints_filters_by_socket_side_when_names_collide() {
    let producer = NodeView {
        id: NodeId(1),
        type_id: "Producer".into(),
        display_name: "Producer".into(),
        category: "test".into(),
        position: [0.0, 200.0],
        inputs: vec![],
        outputs: vec![SocketView {
            name: "Geometry".into(),
            socket_type: SocketTypeId(7),
            display_label: None,
        }],
        properties: vec![],
    };
    // The target node has both an INPUT and an OUTPUT called
    // "Geometry" — the same shape an AtomArtist `Output` node takes
    // once the user wires a node's `Geometry` output into its trailing
    // empty slot (the slot is renamed `Geometry`, and a mirror output
    // also named `Geometry` is appended).
    let ambiguous_target = NodeView {
        id: NodeId(2),
        type_id: "Output".into(),
        display_name: "Output".into(),
        category: "test".into(),
        position: [300.0, 200.0],
        inputs: vec![SocketView {
            name: "Geometry".into(),
            socket_type: SocketTypeId(7),
            display_label: Some("Extrude - Geometry".into()),
        }],
        outputs: vec![SocketView {
            name: "Geometry".into(),
            socket_type: SocketTypeId(7),
            display_label: None,
        }],
        properties: vec![],
    };

    let layouts = vec![
        layout_node_with_connections(&producer, |_| false),
        layout_node_with_connections(&ambiguous_target, |_| true),
    ];

    // Sanity: confirm the row order that triggered the original bug.
    // Outputs come before inputs in the sockets() iterator, so a
    // pre-fix `.find(|s| s.name == "Geometry")` on the target node
    // would have returned the *Output*-side socket.
    let pre_fix_first_hit = layouts[1]
        .sockets()
        .find(|s| s.name == "Geometry")
        .expect("test fixture should expose at least one matching socket");
    assert_eq!(
        pre_fix_first_hit.side,
        SocketSide::Output,
        "pre-fix lookup hits the Output side first — this is what the screenshot showed; \
         the resolver must NOT rely on naked-name lookup here",
    );

    // The fix: resolve_noodle_endpoints filters by side.
    let noodle = NoodleView {
        from_node: NodeId(1),
        from_socket: "Geometry".into(),
        to_node: NodeId(2),
        to_socket: "Geometry".into(),
    };
    let (from, to) =
        resolve_noodle_endpoints(&layouts, &noodle).expect("both endpoints must resolve");
    assert_eq!(
        from.side,
        SocketSide::Output,
        "source endpoint is an output"
    );
    assert_eq!(
        to.side,
        SocketSide::Input,
        "target endpoint must resolve to the Input-side socket — not the same-named Output",
    );
    // The label on the input row carries the human-readable form;
    // verify we got the *input* SocketLayout, not the bare mirror
    // output (which has no display_label).
    assert_eq!(to.display_label, "Extrude - Geometry");
}

/// `resolve_noodle_endpoints` returns `None` when one endpoint's node
/// is missing from the layout list — defensive guard so a stale noodle
/// (e.g. while the host's mutex is mid-update) doesn't panic the paint
/// loop.
#[test]
fn resolve_noodle_endpoints_returns_none_for_missing_node() {
    let producer = NodeView {
        id: NodeId(1),
        type_id: "Producer".into(),
        display_name: "Producer".into(),
        category: "test".into(),
        position: [0.0, 0.0],
        inputs: vec![],
        outputs: vec![SocketView {
            name: "out".into(),
            socket_type: SocketTypeId(0),
            display_label: None,
        }],
        properties: vec![],
    };
    let layouts = vec![layout_node_with_connections(&producer, |_| false)];
    let dangling = NoodleView {
        from_node: NodeId(1),
        from_socket: "out".into(),
        to_node: NodeId(42), // not in the layout list
        to_socket: "in".into(),
    };
    assert!(resolve_noodle_endpoints(&layouts, &dangling).is_none());
}

#[test]
fn chevron_click_in_title_bar_toggles_collapsed_state() {
    // The chevron is a real `ChevronWidget` child of the node's
    // header. Clicking it should set the editor's shared
    // `pending_collapse` channel; the next `layout` pass drains it
    // and toggles the collapsed set.
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0));
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "A", [50.0, 300.0])]);

    assert!(
        !editor.collapsed_nodes.contains(&NodeId(1)),
        "fresh node must start expanded"
    );

    // Find the ChevronWidget child of NodeHeaderWidget.  Tree:
    // NodeEditor → NodeWidget → NodeHeaderWidget (children[0]) →
    // ChevronWidget (children[0]).  Directly fire its on_event so
    // we exercise the real on_click closure that pumps the cell.
    let chevron = editor.children_mut()[0]
        .children_mut()[0]
        .children_mut()[0]
        .as_mut();
    assert_eq!(chevron.type_name(), "ChevronWidget");
    let event = agg_gui::Event::MouseDown {
        pos: agg_gui::Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    };
    let consumed = chevron.on_event(&event);
    assert_eq!(
        consumed,
        agg_gui::EventResult::Consumed,
        "ChevronWidget must consume left-clicks inside its bounds"
    );

    // NodeEditor drains the pending channel on the next layout pass.
    editor.layout(Size::new(800.0, 600.0));
    assert!(
        editor.collapsed_nodes.contains(&NodeId(1)),
        "chevron click must have toggled the collapse set via the drain"
    );

    // Second click toggles back.
    let chevron2 = editor.children_mut()[0]
        .children_mut()[0]
        .children_mut()[0]
        .as_mut();
    let _ = chevron2.on_event(&event);
    editor.layout(Size::new(800.0, 600.0));
    assert!(
        !editor.collapsed_nodes.contains(&NodeId(1)),
        "second chevron click must restore expanded state"
    );
}

#[test]
fn collapsed_node_layout_is_title_height_only() {
    // A collapsed node carries no body rows — its layout height equals
    // TITLE_HEIGHT exactly so the framework lays out a single title-bar
    // strip plus the surrounding shadow halo.
    use crate::draw::{layout_node_with_state, TITLE_HEIGHT};
    let node = NodeView {
        id: NodeId(7),
        type_id: "T".into(),
        display_name: "Collapsed".into(),
        category: "test".into(),
        position: [0.0, 0.0],
        inputs: vec![SocketView {
            name: "in".into(),
            display_label: Some("In".into()),
            socket_type: crate::model::SocketTypeId(0),
        }],
        outputs: vec![SocketView {
            name: "out".into(),
            display_label: Some("Out".into()),
            socket_type: crate::model::SocketTypeId(0),
        }],
        properties: vec![],
    };
    let info = layout_node_with_state(&node, |_| false, true);
    assert!(info.collapsed);
    assert!(
        (info.size[1] - TITLE_HEIGHT).abs() < 1e-9,
        "collapsed layout height must be TITLE_HEIGHT exactly; got {}",
        info.size[1]
    );
    // Sockets still exist for noodle endpoint resolution, anchored at
    // the title-bar side-center.
    assert_eq!(info.rows.len(), 2);
    let center_y = -TITLE_HEIGHT * 0.5;
    for row in &info.rows {
        if let Some(s) = row.socket() {
            assert!(
                (s.center[1] - center_y).abs() < 1e-9,
                "socket Y must land at title-bar centre; got {}",
                s.center[1]
            );
        }
    }
}
