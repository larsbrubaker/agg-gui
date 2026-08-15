//! Unit tests for the host command seam (`widget/commands.rs`) — the
//! [`NodeEditorHandle`] queue plus the two operations it drives,
//! [`NodeEditor::delete_selection`] and [`NodeEditor::select_all`].
//!
//! These cover the widget in isolation; the end-to-end "Edit menu wired
//! to the canvas" behaviour lives in the consuming application's tests.
//! Shares the `Memory` model fixture with the other test modules via
//! [`super::tests_common`].

use super::tests_common::{fixture_with_typed_handle, mk_node, seed_nodes};
use super::*;

fn editor_with_three_nodes() -> (NodeEditor, Arc<Mutex<tests_common::Memory>>) {
    let (shared, typed) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(shared);
    seed_nodes(
        &mut editor,
        &typed,
        vec![
            mk_node(1, "a", [0.0, 0.0]),
            mk_node(2, "b", [200.0, 0.0]),
            mk_node(3, "c", [400.0, 0.0]),
        ],
    );
    (editor, typed)
}

#[test]
fn select_all_selects_every_node_in_the_model() {
    let (mut editor, _typed) = editor_with_three_nodes();
    assert!(editor.selected_ids().is_empty());

    assert!(editor.select_all(), "first call changes the selection");
    let ids: HashSet<NodeId> = [NodeId(1), NodeId(2), NodeId(3)].into_iter().collect();
    assert_eq!(editor.selected_ids(), &ids);

    // Idempotent: a second call has nothing to change and says so, so
    // hosts can skip a redraw.
    assert!(!editor.select_all());
}

#[test]
fn delete_selection_removes_selected_nodes_and_reports_whether_it_acted() {
    let (mut editor, typed) = editor_with_three_nodes();

    // Nothing selected → no-op, and the `false` return is what the key
    // handler turns into `EventResult::Ignored`.
    assert!(!editor.delete_selection());
    assert_eq!(typed.lock().unwrap().nodes.len(), 3);

    editor.select_all();
    assert!(editor.delete_selection());
    assert!(typed.lock().unwrap().nodes.is_empty());
    assert!(
        editor.selected_ids().is_empty(),
        "deleting must clear the now-dangling selection"
    );
}

#[test]
fn delete_selection_reaches_the_model_as_one_group() {
    // The group seam: a host with an undo stack hangs exactly one undo
    // step off a multi-node delete, so the widget must hand the whole
    // selection over in a single `remove_nodes` call.
    let (mut editor, typed) = editor_with_three_nodes();

    editor.select_all();
    assert!(editor.delete_selection());

    let memory = typed.lock().unwrap();
    assert_eq!(
        memory.remove_groups.len(),
        1,
        "a three-node delete must be one grouped call, not three"
    );
    let mut got = memory.remove_groups[0].clone();
    got.sort_by_key(|id| id.0);
    assert_eq!(got, vec![NodeId(1), NodeId(2), NodeId(3)]);
    assert!(memory.nodes.is_empty());
    drop(memory);
    assert!(editor.selected_ids().is_empty());
}

#[test]
fn delete_selection_clears_a_primary_that_it_removed() {
    // A host mirrors the primary selection in its own state; deleting
    // the node it points at must push `None` back through the hook, or
    // the mirror is left dangling on a node the model no longer has.
    let (mut editor, typed) = editor_with_three_nodes();
    typed.lock().unwrap().last_selection = Some(NodeId(2));

    editor.select_all();
    assert!(editor.delete_selection());

    assert_eq!(typed.lock().unwrap().last_selection, None);
}

#[test]
fn delete_selection_leaves_a_primary_it_did_not_remove() {
    // The converse: a primary outside the deleted set is still a valid
    // node, so the hook must stay quiet and the mirror must survive.
    let (mut editor, typed) = editor_with_three_nodes();
    typed.lock().unwrap().last_selection = Some(NodeId(3));

    editor.selected.insert(NodeId(1));
    assert!(editor.delete_selection());

    assert_eq!(typed.lock().unwrap().last_selection, Some(NodeId(3)));
}

#[test]
fn queued_commands_apply_on_the_next_layout() {
    let (shared, typed) = fixture_with_typed_handle();
    let handle = NodeEditorHandle::new();
    let mut editor = NodeEditor::new(shared).with_command_handle(handle.clone());
    seed_nodes(
        &mut editor,
        &typed,
        vec![mk_node(1, "a", [0.0, 0.0]), mk_node(2, "b", [200.0, 0.0])],
    );

    handle.push(NodeEditorCommand::SelectAll);
    assert!(handle.is_pending(), "queued until the editor lays out");
    assert!(
        editor.selected_ids().is_empty(),
        "pushing alone must not touch the widget"
    );

    editor.layout(Size::new(400.0, 300.0));
    assert!(!handle.is_pending(), "layout drains the queue");
    assert_eq!(editor.selected_ids().len(), 2);

    handle.push(NodeEditorCommand::DeleteSelection);
    editor.layout(Size::new(400.0, 300.0));
    assert!(typed.lock().unwrap().nodes.is_empty());
}

#[test]
fn commands_queued_in_one_batch_apply_in_order() {
    let (shared, typed) = fixture_with_typed_handle();
    let handle = NodeEditorHandle::new();
    let mut editor = NodeEditor::new(shared).with_command_handle(handle.clone());
    seed_nodes(
        &mut editor,
        &typed,
        vec![mk_node(1, "a", [0.0, 0.0]), mk_node(2, "b", [200.0, 0.0])],
    );

    // Select All then Delete Selected, both before a frame runs: the
    // delete must see the selection the select-all just made.
    handle.push(NodeEditorCommand::SelectAll);
    handle.push(NodeEditorCommand::DeleteSelection);
    editor.layout(Size::new(400.0, 300.0));

    assert!(typed.lock().unwrap().nodes.is_empty());
}

#[test]
fn an_editor_without_a_handle_ignores_the_queue() {
    // Regression guard for the additive contract: existing consumers
    // that never call `with_command_handle` must lay out unaffected.
    let (mut editor, typed) = editor_with_three_nodes();
    let orphan = NodeEditorHandle::new();
    orphan.push(NodeEditorCommand::DeleteSelection);

    editor.layout(Size::new(400.0, 300.0));

    assert_eq!(typed.lock().unwrap().nodes.len(), 3);
    assert!(orphan.is_pending(), "no editor drains an unattached handle");
}
