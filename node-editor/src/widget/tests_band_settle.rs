//! Tests for ending a gesture that gets no release (`canvas_state.rs`,
//! `rubber_band.rs`): when the pointer capture is lost or the window is
//! deactivated mid-press, a rubber band is cancelled and every other drag
//! ends, so later hover changes nothing; a mode change or another button's
//! press during a band cancels it first; a band reports its primary
//! selection to a host that keeps no copy of it; and the noodle drop ring
//! reaches a socket by the same rule as a press.

use agg_gui::{Event, Modifiers, MouseButton, Point};

use super::tests_common::mk_node;
use super::tests_hosted::node_with_sockets;
use super::tests_rubber_band::{band, down, editor, empty, set, to};
use super::*;

/// Mid-band: the press held {3}, the band previews {1, 2}.
fn editor_mid_band() -> (NodeEditor, Arc<Mutex<super::tests_common::Memory>>) {
    let (mut editor, typed) = editor();
    editor.select_node(NodeId(3), false);
    down(&mut editor, empty(), Modifiers::default());
    to(&mut editor, Point::new(260.0, 250.0));
    assert_eq!(editor.selected_ids(), &set(&[1, 2]), "previewed");
    (editor, typed)
}

fn assert_settled(editor: &mut NodeEditor) {
    assert_eq!(editor.selected_ids(), &set(&[3]), "selection at the press");
    assert!(editor.rubber_band_local_rect().is_none(), "no rectangle");
    assert!(matches!(editor.interaction, CanvasState::Idle));
    // Plain hover afterwards (no button held) changes nothing.
    to(editor, Point::new(5.0, 100.0));
    to(editor, Point::new(390.0, 10.0));
    assert_eq!(editor.selected_ids(), &set(&[3]), "hover selects nothing");
}

#[test]
fn losing_the_pointer_capture_cancels_a_band() {
    let (mut editor, typed) = editor_mid_band();
    editor.on_event(&Event::MouseCaptureLost);
    assert_settled(&mut editor);
    assert_eq!(
        typed.lock().unwrap().selection_reports,
        vec![Some(NodeId(3))]
    );
}

#[test]
fn window_deactivation_cancels_a_band() {
    let (mut editor, _) = editor_mid_band();
    editor.on_event(&Event::WindowDeactivated);
    assert_settled(&mut editor);
}

#[test]
fn losing_the_pointer_capture_ends_a_click_on_empty_canvas() {
    let (mut editor, typed) = editor();
    editor.select_node(NodeId(3), false);
    down(&mut editor, empty(), Modifiers::default());
    editor.on_event(&Event::MouseCaptureLost);
    assert_settled(&mut editor);
    assert_eq!(
        typed.lock().unwrap().selection_reports,
        vec![Some(NodeId(3))]
    );
}

#[test]
fn losing_the_pointer_capture_ends_a_node_drag_and_a_pan() {
    let (mut editor, typed) = editor();
    // Card 3's title bar (top 120).
    let on_card = Point::new(30.0, 112.0);
    down(&mut editor, on_card, Modifiers::default());
    to(&mut editor, Point::new(60.0, 112.0));
    let moved = typed.lock().unwrap().nodes[2].position;
    assert_eq!(moved, [40.0, 120.0], "dragged live");
    editor.on_event(&Event::MouseCaptureLost);
    assert!(matches!(editor.interaction, CanvasState::Idle));
    to(&mut editor, Point::new(200.0, 50.0));
    assert_eq!(typed.lock().unwrap().nodes[2].position, moved, "stays put");

    editor.on_event(&Event::MouseDown {
        pos: Point::new(300.0, 200.0),
        button: MouseButton::Middle,
        modifiers: Modifiers::default(),
    });
    to(&mut editor, Point::new(310.0, 200.0));
    editor.on_event(&Event::MouseCaptureLost);
    assert!(matches!(editor.interaction, CanvasState::Idle));
    to(&mut editor, Point::new(350.0, 250.0));
    assert_eq!(editor.pan(), [10.0, 0.0], "the pan so far stays, no more");
}

#[test]
fn a_mode_change_during_a_band_cancels_it() {
    let (mut editor, typed) = editor_mid_band();
    editor.set_interaction_mode(InteractionMode::Pan);
    editor.set_interaction_mode(InteractionMode::Select);
    assert_settled(&mut editor);
    assert_eq!(
        typed.lock().unwrap().selection_reports,
        vec![Some(NodeId(3))]
    );
}

#[test]
fn a_middle_press_during_a_band_cancels_it_and_pans() {
    let (mut editor, _) = editor_mid_band();
    editor.on_event(&Event::MouseDown {
        pos: Point::new(260.0, 250.0),
        button: MouseButton::Middle,
        modifiers: Modifiers::default(),
    });
    assert_eq!(editor.selected_ids(), &set(&[3]));
    assert!(editor.rubber_band_local_rect().is_none());
    assert!(matches!(
        editor.interaction,
        CanvasState::PanningCanvas { .. }
    ));
}

#[test]
fn a_host_without_a_primary_copy_hears_none_when_a_band_clears_all() {
    let (mut editor, typed) = editor();
    typed.lock().unwrap().no_primary_mirror = true;
    editor.select_node(NodeId(1), false);
    band(
        &mut editor,
        empty(),
        Point::new(240.0, 290.0),
        Modifiers::default(),
    );
    assert!(editor.selected_ids().is_empty());
    assert_eq!(
        typed.lock().unwrap().selection_reports,
        vec![Some(NodeId(1)), None]
    );
    // And a band that keeps the reported primary reports nothing new.
    editor.select_node(NodeId(1), false);
    band(
        &mut editor,
        empty(),
        Point::new(100.0, 250.0),
        Modifiers::default(),
    );
    assert_eq!(editor.selected_ids(), &set(&[1]));
    assert_eq!(typed.lock().unwrap().selection_reports.len(), 3);
}

/// A socket centred exactly on the edge of a card drawn over it is pressed
/// from its uncovered half, and the drop ring reaches it from there too.
#[test]
fn the_drop_ring_reaches_a_socket_where_a_press_does() {
    let (shared, typed) = super::tests_common::fixture_with_typed_handle();
    let mut editor = NodeEditor::new(shared);
    // Card 2 (drawn above card 1: later id) ends where card 1 starts.
    super::tests_common::seed_nodes(
        &mut editor,
        &typed,
        vec![
            node_with_sockets(1, [250.0, 280.0]),
            node_with_sockets(2, [50.0, 280.0]),
            mk_node(3, "src", [10.0, 60.0]),
        ],
    );
    let layouts = editor.snapshot_layouts();
    let card2 = layouts.iter().find(|l| l.node_id == NodeId(2)).unwrap();
    assert_eq!(card2.top_left[0] + card2.size[0], 250.0, "abutting");
    let input = layouts
        .iter()
        .find(|l| l.node_id == NodeId(1))
        .and_then(|l| l.sockets().find(|s| s.name == "Width"))
        .unwrap()
        .clone();
    assert_eq!(input.center[0], 250.0, "centred on card 2's edge");
    assert!(card2.body_contains(input.center));
    let cursor = [input.center[0] + 2.0, input.center[1]];
    let pressed = editor.hit_socket(&layouts, cursor);
    assert_eq!(
        pressed.map(|(n, s)| (n, s.name)),
        Some((NodeId(1), "Width".into()))
    );
    let fixed = crate::connection::SocketRef {
        node: NodeId(3),
        side: crate::draw::SocketSide::Output,
        socket: "Result".into(),
        socket_type: SocketTypeId(0),
    };
    let model = editor.model.lock().unwrap();
    let ring = super::connect::find_target_near(&layouts, &*model, cursor, &fixed, None);
    assert_eq!(ring.map(|s| s.name.as_str()), Some("Width"));
}
