//! Tests for numeric property-row interaction — the `DragValue`
//! contract on NumberDrag rows (threshold scrub, step snap, clamp,
//! click-to-edit) and the preserved immediate-scrub behaviour on Slider
//! rows.
//!
//! Source parity: mirrors the interaction contract of
//! `agg-gui/src/widgets/drag_value.rs`. NumberDrag rows can't mount a
//! real focusable `DragValue` in the canvas's retained back-buffer paint,
//! so the canvas dispatcher (`events.rs`) replicates the contract and
//! these tests pin it.

use super::tests_common::{fixture_with_typed_handle, install_test_font_once, mk_node};
use super::*;
use agg_gui::widgets::{EditorKind, NumberAttrs};
use agg_gui::{Modifiers, MouseButton, Point};
use crate::model::{NodeId, PropertyValue, PropertyView};

/// Build a node with a single numeric property whose editor is `kind`.
fn mk_number_node(
    id: u64,
    name: &str,
    pos: [f64; 2],
    value: f64,
    kind: Option<EditorKind>,
) -> crate::model::NodeView {
    let mut n = mk_node(id, name, pos);
    n.properties.push(PropertyView {
        name: "v".to_string(),
        display_label: None,
        current: PropertyValue::Number(value),
        min: None,
        max: None,
        bound_input: None,
        editor: None,
        editor_kind: kind,
    });
    n
}

/// Local (widget-space) point at the centre of `node`'s numeric row.
/// With the default identity pan/zoom this equals the canvas point, so a
/// press here routes into `on_mouse_down`'s numeric branch.
fn number_row_local(editor: &NodeEditor, node: NodeId) -> Point {
    let layouts = editor.snapshot_layouts();
    let l = layouts
        .iter()
        .find(|l| l.node_id == node)
        .expect("node must have a layout");
    for row in &l.rows {
        if let Some(p) = row.editor() {
            if matches!(p.current, PropertyValue::Number(_)) {
                let cx = p.top_left[0] + p.size[0] * 0.5;
                let cy = p.top_left[1] - p.size[1] * 0.5;
                return Point::new(
                    cx * editor.canvas_scale + editor.canvas_offset[0],
                    cy * editor.canvas_scale + editor.canvas_offset[1],
                );
            }
        }
    }
    panic!("node {node:?} has no numeric row");
}

fn seed(
    editor: &mut NodeEditor,
    memory: &std::sync::Arc<std::sync::Mutex<super::tests_common::Memory>>,
    nodes: Vec<crate::model::NodeView>,
) {
    memory.lock().unwrap().nodes = nodes;
    editor.layout(Size::new(400.0, 400.0));
}

fn last_number(memory: &std::sync::Arc<std::sync::Mutex<super::tests_common::Memory>>) -> Option<f64> {
    memory
        .lock()
        .unwrap()
        .last_property
        .as_ref()
        .and_then(|(_, _, v)| match v {
            PropertyValue::Number(n) => Some(*n),
            _ => None,
        })
}

/// Dragging a NumberDrag row past the threshold scrubs the value by the
/// full pixel delta measured from the press (no dead-zone jump).
#[test]
fn number_drag_scrubs_value_proportionally() {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 400.0));
    seed(
        &mut editor,
        &memory,
        vec![mk_number_node(
            1,
            "N",
            [40.0, 200.0],
            10.0,
            Some(EditorKind::NumberDrag(NumberAttrs::with_range(0.0, 100.0))),
        )],
    );

    let p = number_row_local(&editor, NodeId(1));
    editor.on_mouse_down(p, MouseButton::Left, Modifiers::default());
    editor.on_mouse_move(Point::new(p.x + 20.0, p.y));
    editor.on_mouse_up(Point::new(p.x + 20.0, p.y), MouseButton::Left, Modifiers::default());

    assert_eq!(
        last_number(&memory),
        Some(30.0),
        "20px drag from value 10 should land at 30 (1 unit/px)"
    );
}

/// A drag past the max clamps at the upper bound.
#[test]
fn number_drag_clamps_at_max() {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 400.0));
    seed(
        &mut editor,
        &memory,
        vec![mk_number_node(
            1,
            "N",
            [40.0, 200.0],
            10.0,
            Some(EditorKind::NumberDrag(NumberAttrs::with_range(0.0, 100.0))),
        )],
    );

    let p = number_row_local(&editor, NodeId(1));
    editor.on_mouse_down(p, MouseButton::Left, Modifiers::default());
    editor.on_mouse_move(Point::new(p.x + 500.0, p.y));

    assert_eq!(
        last_number(&memory),
        Some(100.0),
        "a drag past max must clamp to the upper bound"
    );

    // ...and past the min clamps to the lower bound.
    editor.on_mouse_move(Point::new(p.x - 500.0, p.y));
    assert_eq!(
        last_number(&memory),
        Some(0.0),
        "a drag past min must clamp to the lower bound"
    );
}

/// Step snapping rounds the scrubbed value to the nearest multiple.
#[test]
fn number_drag_snaps_to_step() {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 400.0));
    seed(
        &mut editor,
        &memory,
        vec![mk_number_node(
            1,
            "N",
            [40.0, 200.0],
            0.0,
            Some(EditorKind::NumberDrag(
                NumberAttrs::with_range(0.0, 100.0).with_step(5.0),
            )),
        )],
    );

    let p = number_row_local(&editor, NodeId(1));
    editor.on_mouse_down(p, MouseButton::Left, Modifiers::default());
    // 12px from 0 → raw 12 → nearest multiple of 5 is 10.
    editor.on_mouse_move(Point::new(p.x + 12.0, p.y));

    assert_eq!(
        last_number(&memory),
        Some(10.0),
        "step=5 must snap raw 12 to 10"
    );
}

/// A press that never crosses the 3px threshold is a click — it opens
/// the inline keyboard editor and leaves the value untouched.
#[test]
fn number_drag_click_opens_inline_editor() {
    install_test_font_once();
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 400.0));
    seed(
        &mut editor,
        &memory,
        vec![mk_number_node(
            1,
            "N",
            [40.0, 200.0],
            10.0,
            Some(EditorKind::NumberDrag(NumberAttrs::with_range(0.0, 100.0))),
        )],
    );

    let p = number_row_local(&editor, NodeId(1));
    editor.on_mouse_down(p, MouseButton::Left, Modifiers::default());
    // A jitter within the threshold must not be treated as a drag.
    editor.on_mouse_move(Point::new(p.x + 2.0, p.y));
    editor.on_mouse_up(Point::new(p.x + 2.0, p.y), MouseButton::Left, Modifiers::default());

    assert!(
        editor.overlay.is_some(),
        "a sub-threshold click on a NumberDrag row must open the inline editor"
    );
    assert!(
        last_number(&memory).is_none(),
        "a click (no drag) must not scrub the value"
    );
}

/// Slider rows keep NodeDesigner parity: they scrub from the first pixel
/// (no threshold) and a plain click never opens the inline editor.
#[test]
fn slider_row_scrubs_immediately_and_never_edits() {
    install_test_font_once();
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 400.0));
    seed(
        &mut editor,
        &memory,
        vec![mk_number_node(
            1,
            "N",
            [40.0, 200.0],
            10.0,
            Some(EditorKind::Slider(NumberAttrs::with_range(0.0, 100.0))),
        )],
    );

    let p = number_row_local(&editor, NodeId(1));
    editor.on_mouse_down(p, MouseButton::Left, Modifiers::default());
    // 1px — below the NumberDrag threshold, but a slider scrubs anyway.
    editor.on_mouse_move(Point::new(p.x + 1.0, p.y));
    assert_eq!(
        last_number(&memory),
        Some(11.0),
        "a slider must scrub from the first pixel of motion"
    );

    editor.on_mouse_up(Point::new(p.x + 1.0, p.y), MouseButton::Left, Modifiers::default());
    assert!(
        editor.overlay.is_none(),
        "a slider row must never open the inline keyboard editor"
    );
}

/// A Slider row that carries a `step` attr must still scrub continuously
/// — step snapping is a NumberDrag-only behaviour. Guards the gate at
/// `DraggingProperty` construction (`step` is forced to `None` for
/// sliders) so a slider drag is never quantised.
#[test]
fn slider_row_with_step_attr_does_not_snap() {
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 400.0));
    seed(
        &mut editor,
        &memory,
        vec![mk_number_node(
            1,
            "N",
            [40.0, 200.0],
            0.0,
            Some(EditorKind::Slider(
                NumberAttrs::with_range(0.0, 100.0).with_step(5.0),
            )),
        )],
    );

    let p = number_row_local(&editor, NodeId(1));
    editor.on_mouse_down(p, MouseButton::Left, Modifiers::default());
    // 12px from 0 — a NumberDrag with step=5 would snap to 10, but a
    // slider must report the continuous value 12.
    editor.on_mouse_move(Point::new(p.x + 12.0, p.y));

    assert_eq!(
        last_number(&memory),
        Some(12.0),
        "a slider must scrub continuously even when a step attr is set"
    );
}

/// A numeric row with no explicit editor kind defaults to the NumberDrag
/// contract (threshold + click-to-edit), not the slider one.
#[test]
fn number_row_without_kind_defaults_to_drag_contract() {
    install_test_font_once();
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 400.0));
    seed(
        &mut editor,
        &memory,
        vec![mk_number_node(1, "N", [40.0, 200.0], 10.0, None)],
    );

    let p = number_row_local(&editor, NodeId(1));
    editor.on_mouse_down(p, MouseButton::Left, Modifiers::default());
    editor.on_mouse_up(p, MouseButton::Left, Modifiers::default());

    assert!(
        editor.overlay.is_some(),
        "a kind-less numeric row must follow the click-to-edit default"
    );
}
