//! Tests for [`crate::draw`] — extracted to keep `draw.rs` under the
//! 800-line guardrail. Lives as a sibling `#[cfg(test)]` module rather
//! than nested in `draw.rs` so the production module stays small.

use crate::draw::{
    layout_node, layout_node_with_connections, NodeRow, SocketLayout, SocketSide, NODE_WIDTH,
    ROW_HEIGHT, TITLE_HEIGHT,
};
use crate::model::{NodeId, NodeView, PropertyValue, PropertyView, SocketTypeId, SocketView};

fn make_node() -> NodeView {
    NodeView {
        id: NodeId(1),
        type_id: "Test".into(),
        display_name: "Test".into(),
        category: "Test".into(),
        position: [100.0, 200.0],
        inputs: vec![SocketView {
            name: "a".into(),
            socket_type: SocketTypeId(7),
            display_label: None,
        }],
        outputs: vec![SocketView {
            name: "out".into(),
            socket_type: SocketTypeId(7),
            display_label: None,
        }],
        properties: vec![PropertyView {
            name: "v".into(),
            display_label: None,
            current: PropertyValue::Number(1.0),
            min: Some(0.0),
            max: Some(10.0),
            bound_input: None,
            editor: None,
        }],
    }
}

fn make_extrude_like() -> NodeView {
    NodeView {
        id: NodeId(2),
        type_id: "Extrude".into(),
        display_name: "Extrude".into(),
        category: "Operations 3D".into(),
        position: [0.0, 0.0],
        outputs: vec![SocketView {
            name: "Geometry".into(),
            socket_type: SocketTypeId(7),
            display_label: Some("Geometry".into()),
        }],
        inputs: vec![
            SocketView {
                name: "Paths".into(),
                socket_type: SocketTypeId(6),
                display_label: Some("Paths".into()),
            },
            SocketView {
                name: "Height".into(),
                socket_type: SocketTypeId(1),
                display_label: Some("Height".into()),
            },
        ],
        properties: vec![PropertyView {
            name: "height".into(),
            display_label: Some("Height".into()),
            current: PropertyValue::Number(5.0),
            min: Some(0.1),
            max: Some(40.0),
            bound_input: Some("Height".into()),
            editor: None,
        }],
    }
}

#[test]
fn output_row_appears_before_input_rows() {
    let info = layout_node(&make_extrude_like());
    assert!(matches!(info.rows[0], NodeRow::Output(_)));
    for row in &info.rows[1..] {
        assert!(!matches!(row, NodeRow::Output(_)));
    }
}

#[test]
fn input_row_carries_inline_editor_when_property_is_bound() {
    let info = layout_node(&make_extrude_like());
    let height_input = info.rows.iter().find_map(|r| match r {
        NodeRow::Input { socket, editor } if socket.name == "Height" => Some(editor),
        _ => None,
    });
    assert!(
        height_input.unwrap().is_some(),
        "Height input row should carry an inline editor when the bound property is present"
    );
}

#[test]
fn input_row_drops_editor_when_socket_is_connected() {
    let node = make_extrude_like();
    let info = layout_node_with_connections(&node, |name| name == "Height");
    let height_input = info.rows.iter().find_map(|r| match r {
        NodeRow::Input { socket, editor } if socket.name == "Height" => Some(editor),
        _ => None,
    });
    assert!(
        height_input.unwrap().is_none(),
        "connected input should drop its inline editor"
    );
}

#[test]
fn layout_places_input_left_output_right() {
    let info = layout_node(&make_node());
    assert_eq!(info.top_left, [100.0, 200.0]);
    let sockets: Vec<&SocketLayout> = info.sockets().collect();
    assert_eq!(sockets.len(), 2);
    let input = sockets.iter().find(|s| s.side == SocketSide::Input).unwrap();
    let output = sockets
        .iter()
        .find(|s| s.side == SocketSide::Output)
        .unwrap();
    assert!((input.center[0] - 100.0).abs() < 1e-9);
    assert!((output.center[0] - (100.0 + NODE_WIDTH)).abs() < 1e-9);
    // Outputs come first now → output y is at the top row, input
    // sits below it.
    let expected_out_y = 200.0 - TITLE_HEIGHT - 0.5 * ROW_HEIGHT;
    let expected_in_y = 200.0 - TITLE_HEIGHT - 1.5 * ROW_HEIGHT;
    assert!((output.center[1] - expected_out_y).abs() < 1e-9);
    assert!((input.center[1] - expected_in_y).abs() < 1e-9);
}

#[test]
fn body_and_header_contains() {
    let mut n = make_node();
    n.position = [0.0, 0.0];
    let info = layout_node(&n);
    assert!(info.body_contains([10.0, -10.0]));
    assert!(!info.body_contains([10.0, 10.0]));
    assert!(info.header_contains([10.0, -5.0]));
    assert!(!info.header_contains([10.0, -TITLE_HEIGHT - 5.0]));
}

#[test]
fn socket_hit_test() {
    let mut n = make_node();
    n.position = [0.0, 0.0];
    let info = layout_node(&n);
    let sockets: Vec<&SocketLayout> = info.sockets().collect();
    let in_center = sockets
        .iter()
        .find(|s| s.side == SocketSide::Input)
        .unwrap()
        .center;
    assert!(info.socket_at(in_center).is_some());
    assert!(info
        .socket_at([in_center[0] + 5.0, in_center[1] + 5.0])
        .is_some());
    assert!(info
        .socket_at([in_center[0] + 50.0, in_center[1]])
        .is_none());
}

#[test]
fn property_layout_for_unbound_property_uses_full_row() {
    let info = layout_node(&make_node());
    let prop_rows: Vec<&NodeRow> = info
        .rows
        .iter()
        .filter(|r| matches!(r, NodeRow::Property(_)))
        .collect();
    assert_eq!(prop_rows.len(), 1);
    if let NodeRow::Property(p) = prop_rows[0] {
        assert_eq!(p.name, "v");
        assert!((p.size[0] - (NODE_WIDTH - 2.0)).abs() < 1e-9);
    }
}
