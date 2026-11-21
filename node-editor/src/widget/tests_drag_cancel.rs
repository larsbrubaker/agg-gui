//! Tests for Escape during a card drag (`canvas_state.rs`
//! `NodeEditor::cancel_drag`): every dragged card goes back exactly where
//! it was at the press, the snap guides go, the rest of the gesture moves
//! nothing, and the host's own Escape handler is not called; the host
//! hears the drag start at its press and end or cancel once. The resize
//! and value-scrub cancels are tested beside their gestures
//! (`tests_hosted_resize.rs`, `tests_value.rs`).

use agg_gui::{Key, Modifiers, MouseButton, Point};

use super::tests_rubber_band::{down, editor, set, to, up};
use super::*;

fn escape(editor: &mut NodeEditor) -> EventResult {
    editor.on_event(&Event::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::default(),
    })
}

fn position(memory: &Arc<Mutex<super::tests_common::Memory>>, id: u64) -> [f64; 2] {
    let m = memory.lock().unwrap();
    m.nodes
        .iter()
        .find(|n| n.id == NodeId(id))
        .map(|n| n.position)
        .expect("the node exists")
}

/// Card 1's title bar (its top-left is (10, 280), so local == canvas).
const TITLE: Point = Point { x: 50.0, y: 275.0 };

#[test]
fn escape_puts_a_dragged_card_back_and_the_release_moves_nothing() {
    let (mut editor, memory) = editor();
    agg_gui::snap::set_enabled(true);
    down(&mut editor, TITLE, Modifiers::default());
    // Straight down: card 1's left edge stays on card 3's, so the drag
    // shows a snap guide.
    to(&mut editor, Point::new(TITLE.x, TITLE.y - 50.0));
    assert_eq!(position(&memory, 1), [10.0, 230.0], "the drag moved it");
    assert!(
        !agg_gui::snap::guides_snapshot().is_empty(),
        "a guide shows"
    );

    assert!(escape(&mut editor).is_consumed());
    assert_eq!(position(&memory, 1), [10.0, 280.0], "back at its start");
    assert!(matches!(editor.interaction, CanvasState::Idle));
    assert!(agg_gui::snap::guides_snapshot().is_empty(), "guides gone");
    let writes = memory.lock().unwrap().position_writes.clone();
    assert_eq!(writes.last(), Some(&(NodeId(1), [10.0, 280.0])));

    // The rest of the gesture, until the button comes up, does nothing.
    to(&mut editor, Point::new(150.0, 150.0));
    up(&mut editor, Point::new(150.0, 150.0), Modifiers::default());
    assert_eq!(position(&memory, 1), [10.0, 280.0]);
    let m = memory.lock().unwrap();
    assert_eq!(m.position_writes, writes, "no write after the Escape");
    assert_eq!(m.escapes, 0, "the drag used the Escape");
    drop(m);
    // The press's selection stays, as in a file manager.
    assert_eq!(editor.selected_ids(), &set(&[1]));
    agg_gui::snap::set_enabled(false);
    agg_gui::snap::clear_guides();
}

#[test]
fn escape_puts_every_card_of_a_multi_card_drag_back() {
    let (mut editor, memory) = editor();
    editor.select_nodes(&[NodeId(1), NodeId(2)], false);
    down(&mut editor, TITLE, Modifiers::default());
    to(&mut editor, Point::new(TITLE.x + 30.0, TITLE.y - 40.0));
    assert_eq!(position(&memory, 1), [40.0, 240.0]);
    assert_eq!(position(&memory, 2), [280.0, 240.0]);

    assert!(escape(&mut editor).is_consumed());
    assert_eq!(position(&memory, 1), [10.0, 280.0]);
    assert_eq!(position(&memory, 2), [250.0, 280.0]);
    assert_eq!(position(&memory, 3), [10.0, 120.0], "never dragged");
    let writes = memory.lock().unwrap().position_writes.len();

    to(&mut editor, Point::new(200.0, 100.0));
    up(&mut editor, Point::new(200.0, 100.0), Modifiers::default());
    assert_eq!(position(&memory, 1), [10.0, 280.0]);
    assert_eq!(position(&memory, 2), [250.0, 280.0]);
    let m = memory.lock().unwrap();
    assert_eq!(m.position_writes.len(), writes, "no write after the Escape");
    assert_eq!(m.escapes, 0);
    drop(m);
    assert_eq!(editor.selected_ids(), &set(&[1, 2]));
}

#[test]
fn escape_before_the_card_moved_writes_nothing() {
    let (mut editor, memory) = editor();
    down(&mut editor, TITLE, Modifiers::default());
    assert!(escape(&mut editor).is_consumed());
    up(&mut editor, TITLE, Modifiers::default());
    let m = memory.lock().unwrap();
    assert!(m.position_writes.is_empty(), "{:?}", m.position_writes);
    assert_eq!(m.escapes, 0);
}

#[test]
fn escape_with_no_drag_still_reaches_the_host() {
    let (mut editor, memory) = editor();
    escape(&mut editor);
    assert_eq!(memory.lock().unwrap().escapes, 1);

    // And again once a cancelled drag has ended.
    down(&mut editor, TITLE, Modifiers::default());
    to(&mut editor, Point::new(TITLE.x + 30.0, TITLE.y));
    escape(&mut editor);
    up(&mut editor, TITLE, Modifiers::default());
    escape(&mut editor);
    assert_eq!(memory.lock().unwrap().escapes, 2);
}

fn take_log(memory: &Arc<Mutex<super::tests_common::Memory>>) -> Vec<&'static str> {
    std::mem::take(&mut memory.lock().unwrap().hook_log)
}

/// A press on card 1's title bar `n` x 12 units in from its left end, so
/// no two presses are close enough to make a double-click.
fn title_press(memory: &Arc<Mutex<super::tests_common::Memory>>, n: u32) -> Point {
    let [x, top] = position(memory, 1);
    Point::new(x + 12.0 * f64::from(n) + 10.0, top - 5.0)
}

#[test]
fn a_card_drag_reports_its_start_and_its_cancel_or_end_in_order() {
    let (mut editor, memory) = editor();
    // Escape: the restore write comes before `on_drag_cancelled`, and the
    // release after it reports nothing.
    let p = title_press(&memory, 0);
    down(&mut editor, p, Modifiers::default());
    to(&mut editor, Point::new(p.x + 30.0, p.y));
    escape(&mut editor);
    up(&mut editor, p, Modifiers::default());
    assert_eq!(
        take_log(&memory),
        ["started", "position", "position", "cancelled"]
    );

    // A release ends it after its last live write.
    let p = title_press(&memory, 1);
    down(&mut editor, p, Modifiers::default());
    to(&mut editor, Point::new(p.x + 30.0, p.y));
    up(
        &mut editor,
        Point::new(p.x + 30.0, p.y),
        Modifiers::default(),
    );
    assert_eq!(take_log(&memory), ["started", "position", "ended"]);

    // A click is a drag that wrote nothing.
    let p = title_press(&memory, 2);
    down(&mut editor, p, Modifiers::default());
    up(&mut editor, p, Modifiers::default());
    assert_eq!(take_log(&memory), ["started", "ended"]);

    // A lost pointer capture ends it where it is.
    let p = title_press(&memory, 3);
    down(&mut editor, p, Modifiers::default());
    to(&mut editor, Point::new(p.x, p.y - 10.0));
    editor.on_event(&Event::MouseCaptureLost);
    assert_eq!(take_log(&memory), ["started", "position", "ended"]);

    // So does another button's press during the drag.
    let p = title_press(&memory, 4);
    down(&mut editor, p, Modifiers::default());
    editor.on_event(&Event::MouseDown {
        pos: p,
        button: MouseButton::Middle,
        modifiers: Modifiers::default(),
    });
    assert_eq!(take_log(&memory), ["started", "ended"]);
}
