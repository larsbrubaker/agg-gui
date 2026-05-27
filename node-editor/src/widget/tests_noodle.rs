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
