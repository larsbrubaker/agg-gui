//! Inline (frameless) value-editor tests for `NodeEditor`.
//!
//! Part A of the "no popup for number editing" work: clicking a number
//! or string row opens a chrome-less `TextField` positioned exactly over
//! the value pill (a frameless `agg_gui::Window`) rather than a floating
//! titled dialog. These tests lock the placement + commit/revert contract:
//!
//!   - the overlay lands on the pill's screen rect (at zoom 1 and zoomed);
//!   - Enter commits (numbers snap + clamp);
//!   - Escape reverts to the pre-edit value;
//!   - a click-away COMMITS (keeps the live value — DragValue's
//!     lose-focus-commits contract), for both number and string rows.
//!
//! All tests drive the no-sink (in-editor overlay) path so the frameless
//! window is `editor.overlay` and its bounds/behaviour are directly
//! inspectable. The atomartist host path (sink) reuses the same window;
//! only the coordinate space differs (app-absolute vs pane-local).

use super::tests_common::{fixture_with_typed_handle, install_test_font_once, mk_node, seed_nodes};
use super::*;
use crate::model::PropertyValue;
use agg_gui::{Event, Key, Modifiers, MouseButton, Point};

/// Focus the open inline field so it accepts keystrokes: the frameless
/// window forwards `FocusGained` to its content `TextField`, which gates
/// key handling on focus (and selects-all, so typing replaces the seed).
fn focus_inline_field(editor: &mut NodeEditor) {
    editor.on_event(&Event::FocusGained);
}

/// Type a run of characters into the focused inline field.
fn type_chars(editor: &mut NodeEditor, s: &str) {
    for c in s.chars() {
        editor.on_event(&Event::KeyDown {
            key: Key::Char(c),
            modifiers: Modifiers::default(),
        });
    }
}

fn open_editor(
    scale: f64,
    offset: [f64; 2],
) -> (
    NodeEditor,
    std::sync::Arc<std::sync::Mutex<super::tests_common::Memory>>,
) {
    install_test_font_once();
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    editor.canvas_scale = scale;
    editor.canvas_offset = offset;
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "N", [0.0, 0.0])]);
    (editor, memory)
}

/// A number row click drops the frameless editor exactly over the pill —
/// at zoom 1, canvas rect maps straight to editor-local bounds (Y-up: the
/// window's bottom-left `y` is the pill's bottom edge, `top_left_y - h`).
#[test]
fn inline_number_editor_lands_on_pill_rect() {
    let (mut editor, _memory) = open_editor(1.0, [0.0, 0.0]);
    // Pill: top-left (100, 200) canvas, 60×20.
    editor.open_number_editor(
        NodeId(1),
        "value".to_string(),
        5.0,
        Some(0.0),
        Some(100.0),
        None,
        0,
        [100.0, 200.0, 60.0, 20.0],
    );
    let b = editor.overlay.as_ref().expect("editor overlay present").bounds();
    assert_eq!(b, Rect::new(100.0, 180.0, 60.0, 20.0));
}

/// Placement honours the live pan/zoom: screen = canvas * scale + offset.
#[test]
fn inline_number_editor_lands_on_pill_rect_when_zoomed() {
    let (mut editor, _memory) = open_editor(2.0, [10.0, 5.0]);
    editor.open_number_editor(
        NodeId(1),
        "value".to_string(),
        5.0,
        Some(0.0),
        Some(100.0),
        None,
        0,
        [100.0, 200.0, 60.0, 20.0],
    );
    let b = editor.overlay.as_ref().expect("editor overlay present").bounds();
    // x = 100*2 + 10 = 210; bottom = (200-20)*2 + 5 = 365; w = 120; h = 40.
    assert_eq!(b, Rect::new(210.0, 365.0, 120.0, 40.0));
}

/// Enter commits the typed value after snap + clamp and tears the editor
/// down.
#[test]
fn inline_number_editor_enter_commits() {
    let (mut editor, memory) = open_editor(1.0, [0.0, 0.0]);
    editor.open_number_editor(
        NodeId(1),
        "value".to_string(),
        5.0,
        Some(0.0),
        Some(100.0),
        None,
        0,
        [100.0, 200.0, 60.0, 20.0],
    );
    focus_inline_field(&mut editor);
    type_chars(&mut editor, "42");
    editor.on_event(&Event::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::default(),
    });

    match memory.lock().unwrap().last_property.clone() {
        Some((id, name, PropertyValue::Number(v))) => {
            assert_eq!(id, NodeId(1));
            assert_eq!(name, "value");
            assert_eq!(v, 42.0, "Enter must commit the typed value");
        }
        other => panic!("expected a Number commit, got {:?}", other),
    }
    assert!(editor.overlay.is_none(), "Enter must close the inline editor");
}

/// Escape reverts to the pre-edit value, undoing the live preview.
#[test]
fn inline_number_editor_escape_reverts() {
    let (mut editor, memory) = open_editor(1.0, [0.0, 0.0]);
    editor.open_number_editor(
        NodeId(1),
        "value".to_string(),
        5.0,
        Some(0.0),
        Some(100.0),
        None,
        0,
        [100.0, 200.0, 60.0, 20.0],
    );
    focus_inline_field(&mut editor);
    type_chars(&mut editor, "42"); // live on_change previews 42
    editor.on_event(&Event::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::default(),
    });

    match memory.lock().unwrap().last_property.clone() {
        Some((_, _, PropertyValue::Number(v))) => {
            assert_eq!(v, 5.0, "Escape must restore the pre-edit value");
        }
        other => panic!("expected a Number revert, got {:?}", other),
    }
    assert!(editor.overlay.is_none(), "Escape must close the inline editor");
}

/// Enter on unparseable text reverts to the pre-edit value (treated like
/// Escape), discarding the last valid live preview, then closes.
#[test]
fn inline_number_editor_enter_unparseable_reverts() {
    let (mut editor, memory) = open_editor(1.0, [0.0, 0.0]);
    editor.open_number_editor(
        NodeId(1),
        "value".to_string(),
        5.0,
        Some(0.0),
        Some(100.0),
        None,
        1,
        [100.0, 200.0, 60.0, 20.0],
    );
    focus_inline_field(&mut editor);
    // "12.3.4" is unparseable; the "12.3" prefix previews a valid 12.3.
    type_chars(&mut editor, "12.3.4");
    editor.on_event(&Event::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::default(),
    });

    match memory.lock().unwrap().last_property.clone() {
        Some((_, _, PropertyValue::Number(v))) => {
            assert_eq!(
                v, 5.0,
                "Enter on unparseable text must restore the pre-edit value"
            );
        }
        other => panic!("expected a Number revert, got {:?}", other),
    }
    assert!(
        editor.overlay.is_none(),
        "Enter must close the inline editor even when the text is unparseable"
    );
}

/// At heavy zoom-out the scaled pill is only a few px, so the overlay is
/// grown to a minimum readable size instead of being unusably tiny.
#[test]
fn inline_number_editor_clamps_to_minimum_size() {
    let (mut editor, _memory) = open_editor(0.2, [0.0, 0.0]);
    editor.open_number_editor(
        NodeId(1),
        "value".to_string(),
        5.0,
        Some(0.0),
        Some(100.0),
        None,
        0,
        [100.0, 200.0, 60.0, 20.0],
    );
    let b = editor.overlay.as_ref().expect("editor overlay present").bounds();
    assert!(
        b.width >= 60.0 && b.height >= 20.0,
        "overlay must meet the minimum readable size, got {:?}",
        b
    );
}

/// A press outside the pill COMMITS (keeps the live value) — DragValue's
/// lose-focus-commits contract, the opposite of Escape.
#[test]
fn inline_number_editor_click_away_commits() {
    let (mut editor, memory) = open_editor(1.0, [0.0, 0.0]);
    editor.open_number_editor(
        NodeId(1),
        "value".to_string(),
        5.0,
        Some(0.0),
        Some(100.0),
        None,
        0,
        [100.0, 200.0, 60.0, 20.0],
    );
    focus_inline_field(&mut editor);
    type_chars(&mut editor, "42");
    // Press well outside the pill bounds (100,180..160,200): a click-away.
    editor.on_event(&Event::MouseDown {
        pos: Point::new(5.0, 5.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });

    match memory.lock().unwrap().last_property.clone() {
        Some((_, _, PropertyValue::Number(v))) => {
            assert_eq!(v, 42.0, "click-away must keep (commit) the live value");
        }
        other => panic!("expected the committed Number, got {:?}", other),
    }
    assert!(
        editor.overlay.is_none(),
        "click-away must close the inline editor"
    );
}

/// The string inline editor shares the contract: a click-away commits the
/// live value (previously the titled text dialog reverted on click-away).
#[test]
fn inline_text_editor_click_away_commits() {
    install_test_font_once();
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "N", [0.0, 0.0])]);

    editor.open_text_editor(
        NodeId(1),
        "label".to_string(),
        "orig".to_string(),
        [100.0, 200.0, 60.0, 20.0],
    );
    focus_inline_field(&mut editor); // select-all
    type_chars(&mut editor, "new");
    editor.on_event(&Event::MouseDown {
        pos: Point::new(5.0, 5.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });

    match memory.lock().unwrap().last_property.clone() {
        Some((_, _, PropertyValue::Text(s))) => {
            assert_eq!(s, "new", "click-away must keep (commit) the typed string");
        }
        other => panic!("expected the committed Text, got {:?}", other),
    }
    assert!(editor.overlay.is_none());
}
