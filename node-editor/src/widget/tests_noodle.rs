//! Noodle-endpoint disambiguation + chevron/collapse tests for
//! `NodeEditor` — extracted from `tests.rs` to keep that file under
//! the project's 800-line cap. Uses `use super::*` so it still
//! reaches private fields/methods the way the inline tests did.

use super::tests_common::{fixture_with_typed_handle, mk_node, seed_nodes};
use super::*;
use crate::draw::{layout_node_with_connections, SocketSide};
use crate::model::{NodeView, NoodleView, SocketView};
use agg_gui::{Modifiers, MouseButton};

// ---------------------------------------------------------------------------
// resolve_noodle_endpoints — noodle endpoint side-disambiguation
// ---------------------------------------------------------------------------

/// Regression: when a target node has both an input and an output that
/// share a name (the AtomArtist `Output` node's adopted slot + mirror
/// output pattern), the inline name-only lookup the paint loop used
/// originally would resolve the noodle's `to` endpoint to whichever
/// row came first — outputs, since `layout_node_with_connections`
/// emits output rows ahead of input rows. The visual result was a
/// noodle landing on the wrong side of the node (see screenshot in the
/// bug report). The resolver now side-restricts the lookup; this test
/// pins both halves of that fix.
#[test]
fn resolve_noodle_endpoints_filters_by_socket_side_when_names_collide() {
    let producer = NodeView {
        id: NodeId(1),
        type_id: "Producer".into(),
        display_name: "Producer".into(),
        category: "test".into(),
        position: [0.0, 200.0],
        inputs: vec![],
        outputs: vec![SocketView {
            name: "Geometry".into(),
            socket_type: SocketTypeId(7),
            display_label: None,
        }],
        properties: vec![],
        error: None,
    };
    // The target node has both an INPUT and an OUTPUT called
    // "Geometry" — the same shape an AtomArtist `Output` node takes
    // once the user wires a node's `Geometry` output into its trailing
    // empty slot (the slot is renamed `Geometry`, and a mirror output
    // also named `Geometry` is appended).
    let ambiguous_target = NodeView {
        id: NodeId(2),
        type_id: "Output".into(),
        display_name: "Output".into(),
        category: "test".into(),
        position: [300.0, 200.0],
        inputs: vec![SocketView {
            name: "Geometry".into(),
            socket_type: SocketTypeId(7),
            display_label: Some("Extrude - Geometry".into()),
        }],
        outputs: vec![SocketView {
            name: "Geometry".into(),
            socket_type: SocketTypeId(7),
            display_label: None,
        }],
        properties: vec![],
        error: None,
    };

    let layouts = vec![
        layout_node_with_connections(&producer, |_| false),
        layout_node_with_connections(&ambiguous_target, |_| true),
    ];

    // Sanity: confirm the row order that triggered the original bug.
    // Outputs come before inputs in the sockets() iterator, so a
    // pre-fix `.find(|s| s.name == "Geometry")` on the target node
    // would have returned the *Output*-side socket.
    let pre_fix_first_hit = layouts[1]
        .sockets()
        .find(|s| s.name == "Geometry")
        .expect("test fixture should expose at least one matching socket");
    assert_eq!(
        pre_fix_first_hit.side,
        SocketSide::Output,
        "pre-fix lookup hits the Output side first — this is what the screenshot showed; \
         the resolver must NOT rely on naked-name lookup here",
    );

    // The fix: resolve_noodle_endpoints filters by side.
    let noodle = NoodleView {
        from_node: NodeId(1),
        from_socket: "Geometry".into(),
        to_node: NodeId(2),
        to_socket: "Geometry".into(),
    };
    let (from, to) =
        resolve_noodle_endpoints(&layouts, &noodle).expect("both endpoints must resolve");
    assert_eq!(
        from.side,
        SocketSide::Output,
        "source endpoint is an output"
    );
    assert_eq!(
        to.side,
        SocketSide::Input,
        "target endpoint must resolve to the Input-side socket — not the same-named Output",
    );
    // The label on the input row carries the human-readable form;
    // verify we got the *input* SocketLayout, not the bare mirror
    // output (which has no display_label).
    assert_eq!(to.display_label, "Extrude - Geometry");
}

/// `resolve_noodle_endpoints` returns `None` when one endpoint's node
/// is missing from the layout list — defensive guard so a stale noodle
/// (e.g. while the host's mutex is mid-update) doesn't panic the paint
/// loop.
#[test]
fn resolve_noodle_endpoints_returns_none_for_missing_node() {
    let producer = NodeView {
        id: NodeId(1),
        type_id: "Producer".into(),
        display_name: "Producer".into(),
        category: "test".into(),
        position: [0.0, 0.0],
        inputs: vec![],
        outputs: vec![SocketView {
            name: "out".into(),
            socket_type: SocketTypeId(0),
            display_label: None,
        }],
        properties: vec![],
        error: None,
    };
    let layouts = vec![layout_node_with_connections(&producer, |_| false)];
    let dangling = NoodleView {
        from_node: NodeId(1),
        from_socket: "out".into(),
        to_node: NodeId(42), // not in the layout list
        to_socket: "in".into(),
    };
    assert!(resolve_noodle_endpoints(&layouts, &dangling).is_none());
}

#[test]
fn chevron_click_in_title_bar_toggles_collapsed_state() {
    // The chevron is a real `ChevronWidget` child of the node's
    // header. Clicking it should set the editor's shared
    // `pending_collapse` channel; the next `layout` pass drains it
    // and toggles the collapsed set.
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 800.0, 600.0));
    seed_nodes(&mut editor, &memory, vec![mk_node(1, "A", [50.0, 300.0])]);

    assert!(
        !editor.collapsed_nodes.contains(&NodeId(1)),
        "fresh node must start expanded"
    );

    // Find the ChevronWidget child of NodeHeaderWidget.  Tree:
    // NodeEditor → NodeWidget → NodeHeaderWidget (children[0]) →
    // ChevronWidget (children[0]).  Directly fire its on_event so
    // we exercise the real on_click closure that pumps the cell.
    let chevron = editor.children_mut()[0].children_mut()[0].children_mut()[0].as_mut();
    assert_eq!(chevron.type_name(), "ChevronWidget");
    let event = agg_gui::Event::MouseDown {
        pos: agg_gui::Point::new(8.0, 8.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    };
    let consumed = chevron.on_event(&event);
    assert_eq!(
        consumed,
        agg_gui::EventResult::Consumed,
        "ChevronWidget must consume left-clicks inside its bounds"
    );

    // NodeEditor drains the pending channel on the next layout pass.
    editor.layout(Size::new(800.0, 600.0));
    assert!(
        editor.collapsed_nodes.contains(&NodeId(1)),
        "chevron click must have toggled the collapse set via the drain"
    );

    // Second click toggles back.
    let chevron2 = editor.children_mut()[0].children_mut()[0].children_mut()[0].as_mut();
    let _ = chevron2.on_event(&event);
    editor.layout(Size::new(800.0, 600.0));
    assert!(
        !editor.collapsed_nodes.contains(&NodeId(1)),
        "second chevron click must restore expanded state"
    );
}

#[test]
fn collapsed_node_layout_is_title_height_only() {
    // A collapsed node carries no body rows — its layout height equals
    // TITLE_HEIGHT exactly so the framework lays out a single title-bar
    // strip plus the surrounding shadow halo.
    use crate::draw::{layout_node_with_state, TITLE_HEIGHT};
    let node = NodeView {
        id: NodeId(7),
        type_id: "T".into(),
        display_name: "Collapsed".into(),
        category: "test".into(),
        position: [0.0, 0.0],
        inputs: vec![SocketView {
            name: "in".into(),
            display_label: Some("In".into()),
            socket_type: crate::model::SocketTypeId(0),
        }],
        outputs: vec![SocketView {
            name: "out".into(),
            display_label: Some("Out".into()),
            socket_type: crate::model::SocketTypeId(0),
        }],
        properties: vec![],
        error: None,
    };
    let info = layout_node_with_state(&node, |_| false, true);
    assert!(info.collapsed);
    assert!(
        (info.size[1] - TITLE_HEIGHT).abs() < 1e-9,
        "collapsed layout height must be TITLE_HEIGHT exactly; got {}",
        info.size[1]
    );
    // Sockets still exist for noodle endpoint resolution, anchored at
    // the title-bar side-center.
    assert_eq!(info.rows.len(), 2);
    let center_y = -TITLE_HEIGHT * 0.5;
    for row in &info.rows {
        if let Some(s) = row.socket() {
            assert!(
                (s.center[1] - center_y).abs() < 1e-9,
                "socket Y must land at title-bar centre; got {}",
                s.center[1]
            );
        }
    }
}

// ---------------------------------------------------------------------------
// on_node_activated — double-click host hook (drill-into-component upstream)
// ---------------------------------------------------------------------------

/// Fire a left-button `MouseDown` at `pos` through the widget's real
/// event pipeline (so overlay/popup gating and `on_mouse_down` both
/// run, exactly as in production).
fn mouse_down(editor: &mut NodeEditor, x: f64, y: f64) {
    editor.on_event(&agg_gui::Event::MouseDown {
        pos: agg_gui::Point::new(x, y),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

/// Fire a left-button `MouseUp` at `pos` — production double-clicks are
/// down/up/down/up, so the activation tests interleave these between the
/// two downs for fidelity.
fn mouse_up(editor: &mut NodeEditor, x: f64, y: f64) {
    editor.on_event(&agg_gui::Event::MouseUp {
        pos: agg_gui::Point::new(x, y),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

/// Seed a single node whose title bar sits at a known canvas rect so
/// the double-click tests can aim at it. With the default pan/zoom
/// (offset [0,0], scale 1.0) widget-local coords equal canvas coords,
/// so a click at `(60, 190)` lands inside the title bar of a node whose
/// top-left is `[50, 200]` (title spans y ∈ [200 - TITLE_HEIGHT, 200]).
fn seed_single_node(editor: &mut NodeEditor, memory: &Arc<Mutex<super::tests_common::Memory>>) {
    seed_nodes(editor, memory, vec![mk_node(1, "A", [50.0, 200.0])]);
}

#[test]
fn double_click_activation_handled_suppresses_collapse() {
    // When the host's `on_node_activated` returns true (it navigated
    // into a subgraph), the widget must NOT toggle collapse.
    let (model, memory) = fixture_with_typed_handle();
    memory.lock().unwrap().activation_handled = true;
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_single_node(&mut editor, &memory);

    // Two rapid clicks on the title bar = a double-click (down/up/
    // down/up, as the native shell delivers them).
    mouse_down(&mut editor, 60.0, 190.0);
    mouse_up(&mut editor, 60.0, 190.0);
    mouse_down(&mut editor, 60.0, 190.0);
    mouse_up(&mut editor, 60.0, 190.0);

    assert_eq!(
        memory.lock().unwrap().activated,
        vec![NodeId(1)],
        "activation must fire exactly once with the double-clicked node id"
    );
    assert!(
        !editor.collapsed_nodes.contains(&NodeId(1)),
        "a handled activation must suppress the default collapse toggle"
    );
}

#[test]
fn double_click_default_model_still_toggles_collapse() {
    // Default model returns false from on_node_activated → the widget
    // keeps its original collapse-on-double-click behaviour.
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_single_node(&mut editor, &memory);

    mouse_down(&mut editor, 60.0, 190.0);
    mouse_up(&mut editor, 60.0, 190.0);
    mouse_down(&mut editor, 60.0, 190.0);
    mouse_up(&mut editor, 60.0, 190.0);

    assert_eq!(
        memory.lock().unwrap().activated,
        vec![NodeId(1)],
        "the hook must still be consulted once even when it declines"
    );
    assert!(
        editor.collapsed_nodes.contains(&NodeId(1)),
        "an unhandled activation must fall through to the collapse toggle"
    );
}

#[test]
fn single_click_does_not_activate() {
    // A lone click on the title bar records the first click but must
    // not fire activation (no double-click) or collapse the node.
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_single_node(&mut editor, &memory);

    mouse_down(&mut editor, 60.0, 190.0);

    assert!(
        memory.lock().unwrap().activated.is_empty(),
        "a single click must not activate the node"
    );
    assert!(
        !editor.collapsed_nodes.contains(&NodeId(1)),
        "a single click must not collapse the node"
    );
}

#[test]
fn double_click_on_collapsed_node_still_activates() {
    // The primary drill-in case: a collapsed node's entire rect IS its
    // title bar, so double-clicking anywhere on it must fire the hook.
    let (model, memory) = fixture_with_typed_handle();
    memory.lock().unwrap().activation_handled = true;
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_single_node(&mut editor, &memory);

    // Collapse the node first, then re-lay so the title-only rect is live.
    editor.toggle_collapsed(NodeId(1));
    editor.layout(Size::new(400.0, 300.0));
    assert!(editor.collapsed_nodes.contains(&NodeId(1)));

    mouse_down(&mut editor, 60.0, 190.0);
    mouse_up(&mut editor, 60.0, 190.0);
    mouse_down(&mut editor, 60.0, 190.0);
    mouse_up(&mut editor, 60.0, 190.0);

    assert_eq!(
        memory.lock().unwrap().activated,
        vec![NodeId(1)],
        "double-clicking a collapsed node must still fire activation"
    );
    // Handled activation must not toggle the collapse back off.
    assert!(
        editor.collapsed_nodes.contains(&NodeId(1)),
        "a handled activation must leave the collapse state untouched"
    );
}

#[test]
fn two_downs_far_apart_do_not_activate() {
    // The drag-guard: the double-click detector requires the second down
    // within 6px of the first. Two title-bar clicks >6px apart are two
    // singles, not a double — no activation, no collapse.
    let (model, memory) = fixture_with_typed_handle();
    let mut editor = NodeEditor::new(model);
    editor.set_bounds(Rect::new(0.0, 0.0, 400.0, 300.0));
    seed_single_node(&mut editor, &memory);

    // Both points sit inside the title bar (x ∈ [50, 50+width],
    // y ∈ [174, 200]) but are 20px apart in x — beyond the 6px window.
    mouse_down(&mut editor, 60.0, 190.0);
    mouse_up(&mut editor, 60.0, 190.0);
    mouse_down(&mut editor, 80.0, 190.0);
    mouse_up(&mut editor, 80.0, 190.0);

    assert!(
        memory.lock().unwrap().activated.is_empty(),
        "clicks farther than the double-click window must not activate"
    );
    assert!(
        !editor.collapsed_nodes.contains(&NodeId(1)),
        "clicks farther than the double-click window must not collapse"
    );
}
