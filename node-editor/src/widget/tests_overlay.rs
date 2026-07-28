//! Overlay-sink hand-off tests for `NodeEditor` — extracted from
//! `tests.rs` to keep that file under the project's 800-line cap.
//!
//! The sink is the channel that lets app shells (today: AtomArtist's
//! `build_app`) reparent the color-picker dialog from this editor's
//! pane up to a screen-level host so the user can drag it anywhere.
//! The branch in `open_color_picker` is critical: with a sink installed
//! the editor MUST NOT keep the dialog as `self.overlay`, otherwise the
//! dialog would render twice (once here, once at the screen-level host)
//! and double-handle every event.

use super::tests_common::{fixture_with_typed_handle, install_test_font_once, mk_node, seed_nodes};
use super::*;

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

/// The single-line text editor for a `PropertyValue::Text` row follows
/// the exact same sink hand-off contract as the colour picker.
#[test]
fn open_text_editor_hands_off_to_sink_when_installed() {
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

    editor.open_text_editor(
        NodeId(1),
        "label".to_string(),
        "hi".to_string(),
        [0.0, 0.0, 60.0, 20.0],
    );

    assert!(
        captured.borrow().is_some(),
        "sink must receive the constructed text-editor dialog when installed"
    );
    assert!(
        editor.overlay.is_none(),
        "with a sink installed the editor must NOT keep the dialog locally — that would paint it twice"
    );
    assert!(editor.overlay_close_flag.is_none());
}

/// Without a sink the text editor stays a local overlay, same as the
/// colour picker's legacy path.
#[test]
fn open_text_editor_uses_local_overlay_when_no_sink() {
    install_test_font_once();
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "N", [0.0, 0.0])]);

    editor.open_text_editor(
        NodeId(1),
        "label".to_string(),
        "hi".to_string(),
        [0.0, 0.0, 60.0, 20.0],
    );

    assert!(
        editor.overlay.is_some(),
        "without a sink the editor MUST keep the text-editor dialog as its own overlay"
    );
    assert!(editor.overlay_close_flag.is_some());
}

/// A cancel-style dismissal (Escape) reverts the text editor to the
/// pre-edit string. Live `on_change` writes committed a different value
/// during typing; the window's `on_close` must restore the original so
/// only Enter commits. Mirrors the colour picker's `on_cancel` revert.
#[test]
fn text_editor_escape_reverts_to_original_value() {
    use crate::model::PropertyValue;
    use agg_gui::{Event, Key, Modifiers};

    install_test_font_once();
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "N", [0.0, 0.0])]);

    editor.open_text_editor(
        NodeId(1),
        "label".to_string(),
        "original".to_string(),
        [0.0, 0.0, 60.0, 20.0],
    );
    // Stand in for the live keystroke path: on_change commits the
    // in-progress text on every edit.
    memory.lock().unwrap().set_property(
        NodeId(1),
        "label",
        PropertyValue::Text("typed-but-cancelled".to_string()),
    );

    // Escape routes into the modal overlay window and closes it with
    // CloseReason::Escape, firing the revert.
    editor.on_event(&Event::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::default(),
    });

    match memory.lock().unwrap().last_property.clone() {
        Some((id, name, PropertyValue::Text(s))) => {
            assert_eq!(id, NodeId(1));
            assert_eq!(name, "label");
            assert_eq!(s, "original", "Escape must restore the pre-edit string");
        }
        other => panic!(
            "expected a revert write of Text(\"original\"), got {:?}",
            other
        ),
    }
    assert!(
        editor.overlay.is_none(),
        "the cancel must tear the overlay down"
    );
}

// ── Reopen-after-dismiss regression (color picker) ───────────────────
//
// User report: click node A's colour row → picker opens; dismiss it via
// a window-chrome route (× / Escape / click-away); then click a DIFFERENT
// node's colour row → the picker did NOT open. Root cause: the plain
// `color_wheel_picker_dialog` wired no `on_close` and left `click_away`
// at the default `None`, so a chrome dismissal never set the overlay
// `close_flag`; `drain_overlay_close` never fired and the stale overlay
// swallowed the next colour-row click.

/// Build a node carrying a single `Color` property that opens the
/// colour-wheel picker on click (`EditorHint::Color`).
fn mk_color_node(id: u64, name: &str, pos: [f64; 2], rgba: [f32; 4]) -> crate::model::NodeView {
    use crate::model::{EditorHint, PropertyValue, PropertyView};
    let mut n = mk_node(id, name, pos);
    n.properties.push(PropertyView {
        name: "Color".to_string(),
        display_label: None,
        current: PropertyValue::Color(rgba),
        min: None,
        max: None,
        bound_input: None,
        editor: Some(EditorHint::Color),
        editor_kind: None,
    });
    n
}

/// Local (widget-space) point at the centre of `node`'s colour row —
/// the inverse of `NodeEditor::local_to_canvas`, so dispatching a
/// `MouseDown` here routes into the real `on_mouse_down` colour branch.
fn color_row_local(editor: &NodeEditor, node: NodeId) -> agg_gui::Point {
    use crate::model::PropertyValue;
    let layouts = editor.snapshot_layouts();
    let l = layouts
        .iter()
        .find(|l| l.node_id == node)
        .expect("node must have a layout");
    for row in &l.rows {
        if let Some(p) = row.editor() {
            if matches!(p.current, PropertyValue::Color(_)) {
                let cx = p.top_left[0] + p.size[0] * 0.5;
                let cy = p.top_left[1] - p.size[1] * 0.5;
                return agg_gui::Point::new(
                    cx * editor.canvas_scale + editor.canvas_offset[0],
                    cy * editor.canvas_scale + editor.canvas_offset[1],
                );
            }
        }
    }
    panic!("node {node:?} has no colour row");
}

/// Escape must tear the colour-picker overlay down (window-chrome route),
/// leaving the canvas free to open a picker for the next node.
#[test]
fn color_picker_escape_releases_overlay_and_reopens_for_other_node() {
    use agg_gui::{Event, Key, Modifiers, MouseButton};

    install_test_font_once();
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(
        &mut editor,
        &memory,
        vec![
            mk_color_node(1, "A", [40.0, 120.0], [0.4, 0.6, 0.8, 1.0]),
            mk_color_node(2, "Torus", [40.0, 250.0], [0.9, 0.2, 0.1, 1.0]),
        ],
    );

    // Open A's picker via a real colour-row click.
    let a = color_row_local(&editor, NodeId(1));
    editor.on_event(&Event::MouseDown {
        pos: a,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(
        editor.overlay.is_some(),
        "clicking A's colour row opens the picker"
    );

    // Dismiss via Escape (window chrome).
    editor.on_event(&Event::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::default(),
    });
    assert!(
        editor.overlay.is_none(),
        "Escape must release the overlay so the next colour row can open"
    );

    // Now click node B (the torus) — the picker must open for it.
    let b = color_row_local(&editor, NodeId(2));
    editor.on_event(&Event::MouseDown {
        pos: b,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(
        editor.overlay.is_some(),
        "clicking the second node's colour row must open the picker for it"
    );
}

/// A click-away dismissal must also release the colour-picker overlay.
#[test]
fn color_picker_click_away_releases_overlay_and_reopens_for_other_node() {
    use agg_gui::{Event, Modifiers, MouseButton};

    install_test_font_once();
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(
        &mut editor,
        &memory,
        vec![
            mk_color_node(1, "A", [40.0, 120.0], [0.4, 0.6, 0.8, 1.0]),
            mk_color_node(2, "Torus", [40.0, 250.0], [0.9, 0.2, 0.1, 1.0]),
        ],
    );

    let a = color_row_local(&editor, NodeId(1));
    editor.on_event(&Event::MouseDown {
        pos: a,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(
        editor.overlay.is_some(),
        "clicking A's colour row opens the picker"
    );

    // Press far outside the dialog (initial window bounds start at
    // (60, 60)); with click-away enabled this dismisses the picker.
    editor.on_event(&Event::MouseDown {
        pos: agg_gui::Point::new(5.0, 5.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(
        editor.overlay.is_none(),
        "a click-away must release the overlay so the next colour row can open"
    );

    let b = color_row_local(&editor, NodeId(2));
    editor.on_event(&Event::MouseDown {
        pos: b,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(
        editor.overlay.is_some(),
        "clicking the second node's colour row must open the picker for it"
    );
}

/// The text editor already wires `on_close` + click-away, so it must not
/// regress: closing A's editor and clicking B's colour row… actually a
/// text row — reopens fine. Guards the mirrored pattern.
#[test]
fn text_editor_escape_releases_overlay_and_reopens_for_other_node() {
    use agg_gui::{Event, Key, Modifiers};

    install_test_font_once();
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "N", [0.0, 0.0])]);

    editor.open_text_editor(
        NodeId(1),
        "label".to_string(),
        "a".to_string(),
        [0.0, 0.0, 60.0, 20.0],
    );
    assert!(editor.overlay.is_some());
    editor.on_event(&Event::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::default(),
    });
    assert!(
        editor.overlay.is_none(),
        "Escape must release the text-editor overlay"
    );

    // Reopen for a different node — must install a fresh overlay.
    editor.open_text_editor(
        NodeId(2),
        "label".to_string(),
        "b".to_string(),
        [0.0, 0.0, 60.0, 20.0],
    );
    assert!(
        editor.overlay.is_some(),
        "the text editor must reopen for a second node after dismissal"
    );
}
