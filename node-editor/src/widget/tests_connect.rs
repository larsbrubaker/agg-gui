//! Noodle-drag connection semantics on `NodeEditor` (see `connect`):
//! the model's accept rule and refusal note, drops on a card body, drags
//! backwards from an empty input, and picking a noodle up — at once (the
//! default) or kept until the drop (`with_deferred_noodle_pickup`).
//! MatterCAD's reference is `NoodleDragController`.

use super::tests_common::{fixture_with_typed_handle, mk_node, seed_nodes, Memory};
use super::*;
use crate::model::{NodeView, NoodleView, SocketView};
use agg_gui::{Modifiers, MouseButton, Point};

const LOOP: &str = "That would make a loop";

fn socket(name: &str) -> SocketView {
    SocketView {
        name: name.into(),
        socket_type: SocketTypeId(0),
        display_label: None,
    }
}

fn node(id: u64, pos: [f64; 2], inputs: &[&str], outputs: &[&str]) -> NodeView {
    let mut n = mk_node(id, &format!("N{id}"), pos);
    n.inputs = inputs.iter().map(|s| socket(s)).collect();
    n.outputs = outputs.iter().map(|s| socket(s)).collect();
    n
}

fn noodle(from: u64, from_socket: &str, to: u64, to_socket: &str) -> NoodleView {
    NoodleView {
        from_node: NodeId(from),
        from_socket: from_socket.into(),
        to_node: NodeId(to),
        to_socket: to_socket.into(),
    }
}

/// Source 1 (`out`) and 3 (`out`) on the left, target 2 (`a`, `b`) on the
/// right, with no noodles; the editor shows canvas space 1:1.
fn fixture(deferred: bool) -> (NodeEditor, Arc<Mutex<Memory>>) {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model).with_deferred_noodle_pickup(deferred);
    editor.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0));
    seed_nodes(
        &mut editor,
        &memory,
        vec![
            node(1, [20.0, 500.0], &[], &["out"]),
            node(2, [400.0, 500.0], &["a", "b"], &[]),
            node(3, [20.0, 300.0], &[], &["out"]),
        ],
    );
    editor.layout(Size::new(800.0, 600.0));
    (editor, memory)
}

/// Where `socket` on the `side` of `node` is drawn.
fn at(editor: &NodeEditor, node: u64, side: SocketSide, socket: &str) -> Point {
    let layouts = editor.snapshot_layouts();
    let s = layouts
        .iter()
        .find(|l| l.node_id == NodeId(node))
        .and_then(|l| l.sockets().find(|s| s.side == side && s.name == socket))
        .expect("socket is laid out");
    Point::new(s.center[0], s.center[1])
}

/// The middle of `node`'s card, away from its sockets.
fn body(editor: &NodeEditor, node: u64) -> Point {
    let layouts = editor.snapshot_layouts();
    let l = layouts.iter().find(|l| l.node_id == NodeId(node)).unwrap();
    Point::new(
        l.top_left[0] + l.size[0] / 2.0,
        l.top_left[1] - l.size[1] / 2.0,
    )
}

fn press(editor: &mut NodeEditor, p: Point) {
    editor.on_event(&agg_gui::Event::MouseDown {
        pos: p,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

fn drag_to(editor: &mut NodeEditor, p: Point) {
    editor.on_event(&agg_gui::Event::MouseMove { pos: p });
}

fn release(editor: &mut NodeEditor, p: Point) {
    editor.on_event(&agg_gui::Event::MouseUp {
        pos: p,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

/// Press at `from`, move to `to` and let go there.
fn drag(editor: &mut NodeEditor, from: Point, to: Point) {
    press(editor, from);
    drag_to(editor, to);
    release(editor, to);
}

fn noodles(memory: &Arc<Mutex<Memory>>) -> Vec<(u64, String, u64, String)> {
    memory
        .lock()
        .unwrap()
        .noodles
        .iter()
        .map(|n| {
            (
                n.from_node.0,
                n.from_socket.clone(),
                n.to_node.0,
                n.to_socket.clone(),
            )
        })
        .collect()
}

fn one(from: u64, from_socket: &str, to: u64, to_socket: &str) -> (u64, String, u64, String) {
    (from, from_socket.into(), to, to_socket.into())
}

#[test]
fn a_drop_on_an_accepting_socket_connects() {
    let (mut editor, memory) = fixture(false);
    let (from, to) = (
        at(&editor, 1, SocketSide::Output, "out"),
        at(&editor, 2, SocketSide::Input, "a"),
    );
    press(&mut editor, from);
    drag_to(&mut editor, to);
    assert_eq!(editor.noodle_refusal(), None);
    release(&mut editor, to);
    assert_eq!(noodles(&memory), vec![one(1, "out", 2, "a")]);
}

#[test]
fn a_refusing_socket_says_why_and_the_drop_changes_nothing() {
    let (mut editor, memory) = fixture(false);
    memory.lock().unwrap().refusals = vec![("a".into(), LOOP.into())];
    let (from, to) = (
        at(&editor, 1, SocketSide::Output, "out"),
        at(&editor, 2, SocketSide::Input, "a"),
    );
    press(&mut editor, from);
    drag_to(&mut editor, to);
    assert_eq!(editor.noodle_refusal(), Some(LOOP));
    // Off the socket again, the note goes.
    drag_to(&mut editor, Point::new(300.0, 100.0));
    assert_eq!(editor.noodle_refusal(), None);
    drag_to(&mut editor, to);
    release(&mut editor, to);
    assert!(noodles(&memory).is_empty());
    assert_eq!(editor.noodle_refusal(), None, "the note ends with the drag");
}

#[test]
fn a_silent_refusal_shows_no_note() {
    // An output onto another output: the default rule refuses without a
    // reason.
    let (mut editor, memory) = fixture(false);
    let (from, to) = (
        at(&editor, 1, SocketSide::Output, "out"),
        at(&editor, 3, SocketSide::Output, "out"),
    );
    press(&mut editor, from);
    drag_to(&mut editor, to);
    assert_eq!(editor.noodle_refusal(), None);
    release(&mut editor, to);
    assert!(noodles(&memory).is_empty());
}

#[test]
fn a_drag_backwards_from_an_empty_input_connects_to_an_output() {
    let (mut editor, memory) = fixture(false);
    let (from, to) = (
        at(&editor, 2, SocketSide::Input, "b"),
        at(&editor, 3, SocketSide::Output, "out"),
    );
    drag(&mut editor, from, to);
    assert_eq!(noodles(&memory), vec![one(3, "out", 2, "b")]);
}

#[test]
fn a_drop_on_a_card_body_connects_nothing_by_default() {
    let (mut editor, memory) = fixture(false);
    let from = at(&editor, 1, SocketSide::Output, "out");
    let target = body(&editor, 2);
    drag(&mut editor, from, target);
    assert!(noodles(&memory).is_empty());
}

#[test]
fn a_drop_on_a_card_body_takes_the_models_pick_among_accepting_sockets() {
    let (mut editor, memory) = fixture(false);
    {
        let mut m = memory.lock().unwrap();
        m.auto_pick = true;
        // `a` refuses, so the pick is the free `b`.
        m.refusals = vec![("a".into(), LOOP.into())];
    }
    let from = at(&editor, 1, SocketSide::Output, "out");
    let target = body(&editor, 2);
    drag(&mut editor, from, target);
    assert_eq!(noodles(&memory), vec![one(1, "out", 2, "b")]);
    // With `a` accepting again, the same type beats a free input: `a`
    // already has a noodle (the host replaces or appends) yet is picked.
    memory.lock().unwrap().refusals.clear();
    memory.lock().unwrap().noodles.clear();
    memory
        .lock()
        .unwrap()
        .noodles
        .push(noodle(3, "out", 2, "a"));
    editor.layout(Size::new(800.0, 600.0));
    drag(&mut editor, from, target);
    assert_eq!(
        noodles(&memory),
        vec![one(3, "out", 2, "a"), one(1, "out", 2, "a")],
        "the first accepting input of the same type is picked"
    );
}

#[test]
fn picking_a_noodle_up_removes_it_at_once_and_a_drop_on_empty_canvas_leaves_it_gone() {
    let (mut editor, memory) = fixture(false);
    memory
        .lock()
        .unwrap()
        .noodles
        .push(noodle(1, "out", 2, "a"));
    editor.layout(Size::new(800.0, 600.0));
    let from = at(&editor, 2, SocketSide::Input, "a");
    press(&mut editor, from);
    assert!(
        noodles(&memory).is_empty(),
        "the default pick-up removes it"
    );
    drag_to(&mut editor, Point::new(300.0, 100.0));
    release(&mut editor, Point::new(300.0, 100.0));
    assert!(noodles(&memory).is_empty());
    assert!(memory.lock().unwrap().moves.is_empty());
}

#[test]
fn a_deferred_pick_up_keeps_the_noodle_until_the_drop_deletes_it() {
    let (mut editor, memory) = fixture(true);
    memory
        .lock()
        .unwrap()
        .noodles
        .push(noodle(1, "out", 2, "a"));
    editor.layout(Size::new(800.0, 600.0));
    let from = at(&editor, 2, SocketSide::Input, "a");
    press(&mut editor, from);
    assert_eq!(noodles(&memory), vec![one(1, "out", 2, "a")]);
    assert!(editor.picked_up_noodle().is_some(), "not drawn in place");
    drag_to(&mut editor, Point::new(300.0, 100.0));
    release(&mut editor, Point::new(300.0, 100.0));
    assert!(noodles(&memory).is_empty());
    let moves = memory.lock().unwrap().moves.clone();
    assert_eq!(moves.len(), 1, "one move, one undo step");
    assert!(moves[0].1.is_none());
}

#[test]
fn a_deferred_pick_up_moved_to_another_input_is_one_move() {
    let (mut editor, memory) = fixture(true);
    memory
        .lock()
        .unwrap()
        .noodles
        .push(noodle(1, "out", 2, "a"));
    editor.layout(Size::new(800.0, 600.0));
    let (from, to) = (
        at(&editor, 2, SocketSide::Input, "a"),
        at(&editor, 2, SocketSide::Input, "b"),
    );
    drag(&mut editor, from, to);
    assert_eq!(noodles(&memory), vec![one(1, "out", 2, "b")]);
    assert_eq!(memory.lock().unwrap().moves.len(), 1);
}

#[test]
fn a_deferred_pick_up_put_back_or_dropped_on_a_refusal_changes_nothing() {
    let (mut editor, memory) = fixture(true);
    {
        let mut m = memory.lock().unwrap();
        m.noodles.push(noodle(1, "out", 2, "a"));
        // `a` refuses everything new — but putting the noodle back where
        // it came from is always allowed.
        m.refusals = vec![("a".into(), LOOP.into()), ("b".into(), LOOP.into())];
    }
    editor.layout(Size::new(800.0, 600.0));
    let (a, b) = (
        at(&editor, 2, SocketSide::Input, "a"),
        at(&editor, 2, SocketSide::Input, "b"),
    );
    press(&mut editor, a);
    drag_to(&mut editor, b);
    assert_eq!(editor.noodle_refusal(), Some(LOOP));
    drag_to(&mut editor, a);
    assert_eq!(editor.noodle_refusal(), None, "its own place accepts it");
    release(&mut editor, a);
    drag(&mut editor, a, b);
    assert_eq!(noodles(&memory), vec![one(1, "out", 2, "a")]);
    assert!(memory.lock().unwrap().moves.is_empty());
}

#[test]
fn a_multi_input_hands_over_the_noodle_landing_nearest_the_press() {
    let (mut editor, memory) = fixture(true);
    {
        let mut m = memory.lock().unwrap();
        m.multi_input = true;
        m.noodles.push(noodle(1, "out", 2, "a"));
        m.noodles.push(noodle(3, "out", 2, "a"));
    }
    editor.layout(Size::new(800.0, 600.0));
    let centre = at(&editor, 2, SocketSide::Input, "a");
    // The first noodle lands highest; press just below the centre.
    press(&mut editor, Point::new(centre.x, centre.y - 4.0));
    let picked = editor
        .picked_up_noodle()
        .cloned()
        .expect("a noodle is picked up");
    assert_eq!(picked.from_node, NodeId(3));
    release(&mut editor, Point::new(centre.x, centre.y - 4.0));
    press(&mut editor, Point::new(centre.x, centre.y + 4.0));
    let picked = editor
        .picked_up_noodle()
        .cloned()
        .expect("a noodle is picked up");
    assert_eq!(picked.from_node, NodeId(1));
    release(&mut editor, Point::new(centre.x, centre.y + 4.0));
    assert_eq!(memory.lock().unwrap().noodles.len(), 2, "both put back");
    assert!(memory.lock().unwrap().moves.is_empty());
}

#[test]
fn the_refusal_note_is_painted_over_the_editor() {
    let (mut editor, memory) = fixture(false);
    memory.lock().unwrap().refusals = vec![("a".into(), LOOP.into())];
    let (from, to) = (
        at(&editor, 1, SocketSide::Output, "out"),
        at(&editor, 2, SocketSide::Input, "a"),
    );
    press(&mut editor, from);
    drag_to(&mut editor, to);
    let mut r = crate::test_recorder::Recorder::default();
    editor.paint_canvas(&mut r);
    editor.finish_paint_canvas(&mut r);
    assert!(r.texts.iter().any(|t| t == LOOP), "texts: {:?}", r.texts);
    release(&mut editor, to);
    let mut r = crate::test_recorder::Recorder::default();
    editor.paint_canvas(&mut r);
    editor.finish_paint_canvas(&mut r);
    assert!(!r.texts.iter().any(|t| t == LOOP));
}

#[test]
fn a_deferred_pick_up_is_not_drawn_in_place() {
    let (mut editor, memory) = fixture(true);
    memory
        .lock()
        .unwrap()
        .noodles
        .push(noodle(1, "out", 2, "a"));
    editor.layout(Size::new(800.0, 600.0));
    let a = at(&editor, 2, SocketSide::Input, "a");
    let cubic_ends = |editor: &mut NodeEditor| {
        let mut r = crate::test_recorder::Recorder::default();
        editor.paint_canvas(&mut r);
        r.strokes()
            .filter_map(|s| match s.path.get(1) {
                Some(crate::test_recorder::Op::Cubic(_, _, end)) => Some(*end),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(cubic_ends(&mut editor).contains(&[a.x, a.y]));
    press(&mut editor, a);
    let away = Point::new(300.0, 100.0);
    drag_to(&mut editor, away);
    let ends = cubic_ends(&mut editor);
    assert!(!ends.contains(&[a.x, a.y]), "ends: {ends:?}");
    assert!(
        ends.contains(&[away.x, away.y]),
        "the noodle follows the pointer"
    );
}
