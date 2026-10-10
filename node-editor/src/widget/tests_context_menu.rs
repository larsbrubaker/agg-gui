//! Unit tests for the host's say over node menus and keys: a host-built
//! node context menu (`NodeGraphModel::node_context_menu` /
//! `on_node_context_action`, `widget/popup.rs`), the keyboard-delete gate
//! (`NodeGraphModel::can_delete_from_keyboard`) and
//! [`NodeEditor::select_node`] / [`NodeEditorCommand::SelectNode`].
//! Shares the `Memory` model fixture via [`super::tests_common`].

use super::tests_common::{fixture_with_typed_handle, mk_node, seed_nodes, Memory};
use super::*;
use agg_gui::{Key, MenuEntry, MenuItem, Modifiers, MouseButton, Point};

/// Two nodes with their title bars on screen at (50, 250) and (250, 250)
/// in a 400 x 300 pane (view at offset 0, scale 1: local == canvas).
fn editor_with_two_nodes() -> (NodeEditor, Arc<Mutex<Memory>>) {
    let (shared, typed) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(shared);
    seed_nodes(
        &mut editor,
        &typed,
        vec![
            mk_node(1, "a", [50.0, 250.0]),
            mk_node(2, "b", [250.0, 250.0]),
        ],
    );
    (editor, typed)
}

fn right_click(editor: &mut NodeEditor, x: f64, y: f64) {
    editor.on_event(&Event::MouseDown {
        pos: Point::new(x, y),
        button: MouseButton::Right,
        modifiers: Modifiers::default(),
    });
}

fn actions(entries: &[MenuEntry]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Item(item) => Some(item.action.clone().unwrap_or_default()),
            MenuEntry::Separator => None,
        })
        .collect()
}

#[test]
fn node_menu_defaults_to_the_built_in_delete_menu() {
    let (mut editor, _typed) = editor_with_two_nodes();
    right_click(&mut editor, 60.0, 240.0);
    let menu = editor.open_context_menu().expect("a menu opens");
    assert_eq!(actions(&menu.items), ["delete"]);
}

#[test]
fn host_menu_replaces_the_built_in_one_and_gets_its_actions() {
    let (mut editor, typed) = editor_with_two_nodes();
    typed.lock().unwrap().context_menu = Some(vec![MenuEntry::Item(MenuItem::action(
        "Delete Node",
        "Delete Node Menu Item",
    ))]);
    typed.lock().unwrap().context_command = Some(NodeEditorCommand::DeleteSelection);

    right_click(&mut editor, 260.0, 240.0);
    let menu = editor.open_context_menu().expect("the host's menu opens");
    assert_eq!(actions(&menu.items), ["Delete Node Menu Item"]);
    assert_eq!(editor.selected_ids().len(), 1);

    editor.handle_popup_action("Delete Node Menu Item");
    let memory = typed.lock().unwrap();
    assert_eq!(
        memory.context_actions,
        [(NodeId(2), "Delete Node Menu Item".to_string())]
    );
    // The returned command ran: the right-clicked node is gone.
    assert_eq!(memory.nodes.len(), 1);
    assert_eq!(memory.nodes[0].id, NodeId(1));
}

#[test]
fn empty_host_menu_opens_nothing_but_still_selects() {
    let (mut editor, typed) = editor_with_two_nodes();
    typed.lock().unwrap().context_menu = Some(Vec::new());
    right_click(&mut editor, 60.0, 240.0);
    assert!(editor.open_context_menu().is_none());
    let ids: HashSet<NodeId> = [NodeId(1)].into_iter().collect();
    assert_eq!(editor.selected_ids(), &ids);
}

#[test]
fn empty_canvas_menu_stays_built_in_after_a_host_node_menu() {
    let (mut editor, typed) = editor_with_two_nodes();
    typed.lock().unwrap().context_menu = Some(vec![MenuEntry::Item(MenuItem::action(
        "Delete Node",
        "Delete Node Menu Item",
    ))]);
    right_click(&mut editor, 60.0, 240.0);
    editor.popup.close();
    right_click(&mut editor, 200.0, 20.0);
    assert!(editor.popup_host_node.is_none());
    editor.handle_popup_action("Delete Node Menu Item");
    assert!(
        typed.lock().unwrap().context_actions.is_empty(),
        "an empty-canvas menu action must not reach the node-menu hook"
    );
}

#[test]
fn keyboard_delete_respects_the_host_gate() {
    let (mut editor, typed) = editor_with_two_nodes();
    editor.select_all();
    typed.lock().unwrap().block_keyboard_delete = true;
    let result = editor.on_event(&Event::KeyDown {
        key: Key::Delete,
        modifiers: Modifiers::default(),
    });
    assert_eq!(result, EventResult::Ignored, "the key goes on to the host");
    assert_eq!(typed.lock().unwrap().nodes.len(), 2);
    assert_eq!(editor.selected_ids().len(), 2);

    typed.lock().unwrap().block_keyboard_delete = false;
    let result = editor.on_event(&Event::KeyDown {
        key: Key::Backspace,
        modifiers: Modifiers::default(),
    });
    assert_eq!(result, EventResult::Consumed);
    assert!(typed.lock().unwrap().nodes.is_empty());
}

#[test]
fn select_node_selects_only_that_node_and_reports_it() {
    let (mut editor, typed) = editor_with_two_nodes();
    editor.select_all();
    assert!(editor.select_node(NodeId(2), false));
    let ids: HashSet<NodeId> = [NodeId(2)].into_iter().collect();
    assert_eq!(editor.selected_ids(), &ids);
    assert_eq!(typed.lock().unwrap().last_selection, Some(NodeId(2)));
    assert_eq!(editor.canvas_offset, [0.0, 0.0], "no reveal asked");

    assert!(!editor.select_node(NodeId(9), true), "unknown node");
    assert_eq!(editor.selected_ids(), &ids);
}

#[test]
fn select_node_reveal_centres_an_offscreen_card_only() {
    let (mut editor, _typed) = editor_with_two_nodes();
    // Already wholly visible: the view stays.
    let layouts = editor.snapshot_layouts();
    let a = layouts.iter().find(|l| l.node_id == NodeId(1)).unwrap();
    let (top_left, size) = (a.top_left, a.size);
    assert!(editor.select_node(NodeId(1), true));
    assert_eq!(editor.canvas_offset, [0.0, 0.0]);

    // Pan it out of the pane, then reveal: its centre lands mid-pane.
    assert!(editor.set_view(1.0, [-1000.0, 0.0]));
    assert!(editor.select_node(NodeId(1), true));
    let cx = (top_left[0] + size[0] * 0.5) + editor.canvas_offset[0];
    let cy = (top_left[1] - size[1] * 0.5) + editor.canvas_offset[1];
    assert!((cx - 200.0).abs() < 1e-9, "x centre {cx}");
    assert!((cy - 150.0).abs() < 1e-9, "y centre {cy}");
}

#[test]
fn select_node_command_is_applied_on_layout() {
    let (mut editor, typed) = editor_with_two_nodes();
    let handle = NodeEditorHandle::new();
    editor = editor.with_command_handle(handle.clone());
    handle.push(NodeEditorCommand::SelectNode {
        id: NodeId(1),
        reveal: false,
    });
    editor.layout(Size::new(400.0, 300.0));
    let ids: HashSet<NodeId> = [NodeId(1)].into_iter().collect();
    assert_eq!(editor.selected_ids(), &ids);
    assert_eq!(typed.lock().unwrap().last_selection, Some(NodeId(1)));
}
