//! Unit tests for selecting a set of cards from the host:
//! [`NodeEditor::select_nodes`] (`widget/commands.rs`). A host pasting or duplicating several cards
//! uses it to leave exactly the new cards selected, so these pin that the
//! result matches a user's Shift-click selection, that the editor's
//! Delete then acts on the set, and that reveal brings the set on screen.
//! Shares the `Memory` model fixture via [`super::tests_common`].

use super::tests_common::{fixture_with_typed_handle, mk_node, seed_nodes, Memory};
use super::*;
use agg_gui::{Modifiers, MouseButton, Point};

/// Three cards in a row, all on screen in the 400 x 300 pane (view at
/// offset 0, scale 1: local == canvas).
fn editor_with_three_nodes() -> (NodeEditor, Arc<Mutex<Memory>>) {
    let (shared, typed) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(shared);
    seed_nodes(
        &mut editor,
        &typed,
        vec![
            mk_node(1, "a", [10.0, 280.0]),
            mk_node(2, "b", [10.0, 160.0]),
            mk_node(3, "c", [210.0, 280.0]),
        ],
    );
    (editor, typed)
}

fn set(ids: &[u64]) -> HashSet<NodeId> {
    ids.iter().map(|&id| NodeId(id)).collect()
}

/// Click card `id`'s title bar with the left button, Shift held or not.
fn click_card(editor: &mut NodeEditor, id: u64, shift: bool) {
    let layouts = editor.snapshot_layouts();
    let l = layouts.iter().find(|l| l.node_id == NodeId(id)).unwrap();
    let pos = Point::new(l.top_left[0] + 10.0, l.top_left[1] - 8.0);
    let modifiers = Modifiers {
        shift,
        ..Modifiers::default()
    };
    editor.on_event(&Event::MouseDown {
        pos,
        button: MouseButton::Left,
        modifiers,
    });
    editor.on_event(&Event::MouseUp {
        pos,
        button: MouseButton::Left,
        modifiers,
    });
}

#[test]
fn select_nodes_selects_exactly_the_listed_cards() {
    let (mut editor, typed) = editor_with_three_nodes();
    editor.select_node(NodeId(2), false);

    assert_eq!(editor.select_nodes(&[NodeId(1), NodeId(3)], false), 2);
    assert_eq!(editor.selected_ids(), &set(&[1, 3]));
    assert_eq!(
        typed.lock().unwrap().last_selection,
        Some(NodeId(3)),
        "the last listed card is primary, as the last Shift-click makes it"
    );
    assert_eq!(editor.canvas_offset, [0.0, 0.0], "no reveal asked");
}

#[test]
fn select_nodes_matches_a_shift_click_selection() {
    let (mut clicked, clicked_model) = editor_with_three_nodes();
    click_card(&mut clicked, 1, false);
    click_card(&mut clicked, 3, true);
    assert_eq!(clicked.selected_ids(), &set(&[1, 3]), "fixture clicks");

    let (mut commanded, commanded_model) = editor_with_three_nodes();
    commanded.select_nodes(&[NodeId(1), NodeId(3)], false);
    assert_eq!(commanded.selected_ids(), clicked.selected_ids());
    assert_eq!(
        commanded_model.lock().unwrap().last_selection,
        clicked_model.lock().unwrap().last_selection
    );
}

#[test]
fn delete_after_select_nodes_removes_both_cards_in_one_group() {
    let (mut editor, typed) = editor_with_three_nodes();
    editor.select_nodes(&[NodeId(1), NodeId(3)], false);
    assert!(editor.delete_selection());

    let memory = typed.lock().unwrap();
    let left: Vec<NodeId> = memory.nodes.iter().map(|n| n.id).collect();
    assert_eq!(left, [NodeId(2)]);
    assert_eq!(memory.remove_groups.len(), 1, "one undo step");
    assert_eq!(memory.last_selection, None, "primary was deleted");
}

#[test]
fn select_nodes_with_one_card_matches_select_node() {
    let (mut by_list, list_model) = editor_with_three_nodes();
    by_list.select_all();
    by_list.set_view(1.0, [-1000.0, 0.0]);
    assert_eq!(by_list.select_nodes(&[NodeId(2)], true), 1);

    let (mut by_one, one_model) = editor_with_three_nodes();
    by_one.select_all();
    by_one.set_view(1.0, [-1000.0, 0.0]);
    assert!(by_one.select_node(NodeId(2), true));

    assert_eq!(by_list.selected_ids(), &set(&[2]));
    assert_eq!(by_list.selected_ids(), by_one.selected_ids());
    assert_eq!(by_list.canvas_offset, by_one.canvas_offset);
    assert_eq!(by_list.canvas_scale, by_one.canvas_scale);
    assert_eq!(
        list_model.lock().unwrap().last_selection,
        one_model.lock().unwrap().last_selection
    );
}

#[test]
fn select_nodes_ignores_unknown_and_repeated_ids() {
    let (mut editor, typed) = editor_with_three_nodes();
    let ids = [NodeId(9), NodeId(1), NodeId(1), NodeId(42), NodeId(2)];
    assert_eq!(editor.select_nodes(&ids, false), 2);
    assert_eq!(editor.selected_ids(), &set(&[1, 2]));
    assert_eq!(typed.lock().unwrap().last_selection, Some(NodeId(2)));
}

#[test]
fn select_nodes_with_nothing_known_clears_the_selection() {
    let (mut editor, typed) = editor_with_three_nodes();
    editor.select_nodes(&[NodeId(1), NodeId(2)], false);

    assert_eq!(editor.select_nodes(&[], false), 0);
    assert!(editor.selected_ids().is_empty());
    assert_eq!(typed.lock().unwrap().last_selection, None);

    editor.select_nodes(&[NodeId(3)], false);
    assert_eq!(editor.select_nodes(&[NodeId(77)], true), 0);
    assert!(
        editor.selected_ids().is_empty(),
        "only unknown ids: cleared"
    );
    assert_eq!(typed.lock().unwrap().last_selection, None);
}

#[test]
fn select_nodes_reveal_centres_an_offscreen_pair_only() {
    let (mut editor, _typed) = editor_with_three_nodes();
    // Both already wholly visible: the view stays.
    editor.select_nodes(&[NodeId(1), NodeId(2)], true);
    assert_eq!(editor.canvas_offset, [0.0, 0.0]);

    // Pan the pair out of the pane, then reveal: the centre of their
    // bounding box lands mid-pane, at the same zoom, and both cards are
    // wholly on screen.
    assert!(editor.set_view(1.0, [-1000.0, 400.0]));
    editor.select_nodes(&[NodeId(1), NodeId(2)], true);
    assert_eq!(editor.canvas_scale, 1.0, "reveal never zooms");
    let layouts = editor.snapshot_layouts();
    let o = editor.canvas_offset;
    let (mut left, mut right) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut top, mut bottom) = (f64::NEG_INFINITY, f64::INFINITY);
    for l in layouts.iter().filter(|l| l.node_id != NodeId(3)) {
        left = left.min(l.top_left[0] + o[0]);
        right = right.max(l.top_left[0] + l.size[0] + o[0]);
        top = top.max(l.top_left[1] + o[1]);
        bottom = bottom.min(l.top_left[1] - l.size[1] + o[1]);
    }
    assert!(((left + right) * 0.5 - 200.0).abs() < 1e-9, "x centre");
    assert!(((top + bottom) * 0.5 - 150.0).abs() < 1e-9, "y centre");
    assert!(left >= 0.0 && right <= 400.0 && bottom >= 0.0 && top <= 300.0);
}
