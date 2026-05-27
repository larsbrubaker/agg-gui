//! Shared fixtures and helpers for the `NodeEditor` test modules
//! (`tests`, `tests_overlay`, `tests_noodle`). Extracted from
//! `tests.rs` so that the parent `tests.rs` stays under the project's
//! 800-line cap while keeping the same access to private fields via
//! `use super::*`.

use super::*;
use crate::model::{NodeTypeView, NodeView, NoodleResult, NoodleView, PropertyValue};

/// Trivial in-memory model for unit tests.
#[derive(Default)]
pub(super) struct Memory {
    pub nodes: Vec<NodeView>,
    pub noodles: Vec<NoodleView>,
    pub zoom: f64,
    pub last_selection: Option<NodeId>,
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

pub(super) fn fixture() -> SharedModel {
    Arc::new(Mutex::new(Memory::default()))
}

/// Same as [`fixture`] but returns both the trait-object SharedModel
/// AND a typed Arc<Mutex<Memory>> handle so tests can mutate the
/// concrete `nodes` field (the trait surface returns owned `Vec`s
/// only, no direct mutation of the node list).
pub(super) fn fixture_with_typed_handle() -> (SharedModel, Arc<Mutex<Memory>>) {
    let typed = Arc::new(Mutex::new(Memory::default()));
    let shared: SharedModel = typed.clone();
    (shared, typed)
}

/// Build a fresh `NodeView` for tests — no sockets, no properties.
pub(super) fn mk_node(id: u64, name: &str, pos: [f64; 2]) -> NodeView {
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
pub(super) fn seed_nodes(
    editor: &mut NodeEditor,
    memory: &Arc<Mutex<Memory>>,
    nodes: Vec<NodeView>,
) {
    memory.lock().unwrap().nodes = nodes;
    editor.layout(Size::new(400.0, 300.0));
}

// ── Overlay-sink hand-off test font ──────────────────────────────────
//
// The color-picker dialog needs a system font to lay out its labels.
// Tests that exercise `open_color_picker` install Cascadia Code from
// the demo assets the first time they run.

pub(super) const TEST_FONT_FOR_PICKER: &[u8] =
    include_bytes!("../../../demo/assets/CascadiaCode.ttf");

pub(super) fn install_test_font_once() {
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
