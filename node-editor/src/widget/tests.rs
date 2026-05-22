//! Tests for `NodeEditor` — extracted from `mod.rs` to keep the parent
//! file under the project's 800-line cap.  Uses `use super::*` so it
//! still reaches private fields/methods (canvas_offset, canvas_scale,
//! local_to_canvas) the way the inline tests did.

use super::*;
use crate::model::{EdgeResult, EdgeView, NodeTypeView, NodeView, PropertyValue};
use agg_gui::Point;

/// Trivial in-memory model for unit tests.
#[derive(Default)]
struct Memory {
    nodes: Vec<NodeView>,
    edges: Vec<EdgeView>,
    zoom: f64,
    last_selection: Option<NodeId>,
}

impl NodeGraphModel for Memory {
    fn nodes(&self) -> Vec<NodeView> {
        self.nodes.clone()
    }
    fn edges(&self) -> Vec<EdgeView> {
        self.edges.clone()
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
    fn try_add_edge(
        &mut self,
        from_node: NodeId,
        from_socket: &str,
        to_node: NodeId,
        to_socket: &str,
    ) -> EdgeResult {
        self.edges.push(EdgeView {
            from_node,
            from_socket: from_socket.into(),
            to_node,
            to_socket: to_socket.into(),
        });
        EdgeResult::Connected
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

/// The editor's `inspector_child_transform` must mirror the pan/zoom
/// the canvas applies during paint — otherwise the inspector hover
/// overlay lands at the un-transformed canvas position instead of the
/// actual screen pixels.
#[test]
fn editor_inspector_child_transform_reflects_pan_zoom() {
    let mut editor = NodeEditor::new(fixture());
    editor.canvas_offset = [30.0, 50.0];
    editor.canvas_scale = 2.0;

    let t = editor.inspector_child_transform();
    // Canvas-space (0, 0) should land at screen (offset_x, offset_y).
    let mut x = 0.0;
    let mut y = 0.0;
    t.transform(&mut x, &mut y);
    assert!((x - 30.0).abs() < 1e-6 && (y - 50.0).abs() < 1e-6, "got ({x},{y})");

    // Canvas-space (10, 5) should land at screen (offset + scale * canvas).
    let mut x = 10.0;
    let mut y = 5.0;
    t.transform(&mut x, &mut y);
    assert!(
        (x - (30.0 + 2.0 * 10.0)).abs() < 1e-6
            && (y - (50.0 + 2.0 * 5.0)).abs() < 1e-6,
        "got ({x},{y})"
    );
}

/// End-to-end: with the editor positioned at non-zero screen origin AND
/// pan/zoom active, a `collect_inspector_nodes` walk must report the
/// child node's bounds at the correct screen pixels.
#[test]
fn collect_inspector_nodes_transforms_through_editor_pan_zoom() {
    use agg_gui::widget::collect_inspector_nodes;
    use agg_gui::Point;

    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(100.0, 200.0, 800.0, 600.0));
    editor.canvas_offset = [25.0, 40.0];
    editor.canvas_scale = 1.5;
    // Use a node at canvas-space (50, 60).  Default node layout
    // produces a positive size; we read the resulting NodeWidget's
    // canvas-space bounds back from `editor.children()` so the test
    // doesn't have to bake the node-layout math.
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "N", [50.0, 60.0])]);

    let nw_canvas = editor.children()[0].bounds();

    let mut nodes = Vec::new();
    collect_inspector_nodes(&editor, 0, Point::ORIGIN, &mut nodes);
    let node = nodes
        .iter()
        .find(|n| n.type_name == "NodeWidget")
        .expect("NodeWidget missing from inspector snapshot");

    let s = editor.canvas_scale;
    let expected_x = editor.bounds().x + editor.canvas_offset[0] + nw_canvas.x * s;
    let expected_y = editor.bounds().y + editor.canvas_offset[1] + nw_canvas.y * s;
    let expected_w = nw_canvas.width * s;
    let expected_h = nw_canvas.height * s;
    let b = node.screen_bounds;
    assert!(
        (b.x - expected_x).abs() < 1e-6
            && (b.y - expected_y).abs() < 1e-6
            && (b.width - expected_w).abs() < 1e-6
            && (b.height - expected_h).abs() < 1e-6,
        "NodeWidget screen_bounds must be the editor's pan/zoom applied to canvas-space; \
         expected x={expected_x} y={expected_y} w={expected_w} h={expected_h}; got {:?}",
        b
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
