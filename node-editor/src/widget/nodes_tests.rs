//! Tests for [`crate::widget::nodes`] — extracted from `nodes.rs` to
//! keep that file under the project's 800-line cap.

use super::nodes::*;
use crate::draw::{layout_node, CanvasPalette, NODE_WIDTH};
use crate::model::{
    NodeGraphModel, NodeId, NodeView, PropertyValue, PropertyView, SocketTypeId, SocketView,
};
use agg_gui::Widget;

struct DummyModel;
impl NodeGraphModel for DummyModel {
    fn nodes(&self) -> Vec<NodeView> {
        vec![]
    }
    fn edges(&self) -> Vec<crate::model::EdgeView> {
        vec![]
    }
    fn node_types_by_category(&self) -> Vec<(String, Vec<crate::model::NodeTypeView>)> {
        vec![]
    }
    fn set_node_position(&mut self, _: NodeId, _: [f64; 2]) {}
    fn add_node(&mut self, _: &str, _: [f64; 2]) -> Option<NodeId> {
        None
    }
    fn remove_node(&mut self, _: NodeId) {}
    fn try_add_edge(
        &mut self,
        _: NodeId,
        _: &str,
        _: NodeId,
        _: &str,
    ) -> crate::model::EdgeResult {
        crate::model::EdgeResult::Rejected
    }
    fn set_property(&mut self, _: NodeId, _: &str, _: PropertyValue) {}
}

fn make_node() -> NodeView {
    NodeView {
        id: NodeId(42),
        type_id: "Extrude".into(),
        display_name: "Extrude".into(),
        category: "Operations 3D".into(),
        position: [10.0, 50.0],
        outputs: vec![SocketView {
            name: "Geometry".into(),
            socket_type: SocketTypeId(7),
            display_label: Some("Geometry".into()),
        }],
        inputs: vec![SocketView {
            name: "Paths".into(),
            socket_type: SocketTypeId(6),
            display_label: Some("Paths".into()),
        }],
        properties: vec![PropertyView {
            name: "height".into(),
            display_label: Some("Height".into()),
            current: PropertyValue::Number(5.0),
            min: Some(0.0),
            max: Some(40.0),
            bound_input: None,
            editor: None,
        }],
    }
}

#[test]
fn imported_node_width_matches_layout_default() {
    let layout = layout_node(&make_node());
    assert!((layout.size[0] - NODE_WIDTH).abs() < 1e-9);
}

#[test]
fn node_widget_carries_header_and_row_children() {
    let layout = layout_node(&make_node());
    let ctx = NodePaintContext::from_model(CanvasPalette::dark(), &DummyModel);
    let nw = NodeWidget::from_layout(&layout, false, ctx);
    assert!(!nw.children().is_empty());
    assert_eq!(nw.children()[0].type_name(), "NodeHeaderWidget");
    let row_count = layout.rows.len();
    assert_eq!(nw.children().len(), row_count + 1);
    for i in 1..=row_count {
        assert_eq!(nw.children()[i].type_name(), "NodeRowWidget");
    }
}

#[test]
fn input_row_contains_socket_and_label_subwidgets() {
    let layout = layout_node(&make_node());
    let ctx = NodePaintContext::from_model(CanvasPalette::dark(), &DummyModel);
    let nw = NodeWidget::from_layout(&layout, false, ctx);
    let row = nw
        .children()
        .iter()
        .filter(|c| c.type_name() == "NodeRowWidget")
        .find(|c| {
            c.properties()
                .iter()
                .any(|(k, v)| *k == "row" && v == "input:Paths")
        })
        .expect("expected an input row for Paths");
    let kinds: Vec<&'static str> = row.children().iter().map(|c| c.type_name()).collect();
    assert!(kinds.contains(&"SocketDotWidget"));
    assert!(kinds.contains(&"RowLabelWidget"));
}

#[test]
fn output_row_dot_sits_on_right_side() {
    let layout = layout_node(&make_node());
    let ctx = NodePaintContext::from_model(CanvasPalette::dark(), &DummyModel);
    let nw = NodeWidget::from_layout(&layout, false, ctx);
    let row = nw
        .children()
        .iter()
        .filter(|c| c.type_name() == "NodeRowWidget")
        .find(|c| {
            c.properties()
                .iter()
                .any(|(k, v)| *k == "row" && v == "output:Geometry")
        })
        .expect("expected an output row for Geometry");
    let dot = row
        .children()
        .iter()
        .find(|c| c.type_name() == "SocketDotWidget")
        .expect("expected a socket dot in the output row");
    let centre_x = dot.bounds().x + dot.bounds().width * 0.5;
    assert!(
        (centre_x - NODE_WIDTH).abs() < 1e-6,
        "output dot centre should hug the right edge"
    );
}

#[test]
fn property_row_owns_value_editor() {
    let layout = layout_node(&make_node());
    let ctx = NodePaintContext::from_model(CanvasPalette::dark(), &DummyModel);
    let nw = NodeWidget::from_layout(&layout, false, ctx);
    let row = nw
        .children()
        .iter()
        .filter(|c| c.type_name() == "NodeRowWidget")
        .find(|c| {
            c.properties()
                .iter()
                .any(|(k, v)| *k == "row" && v == "prop:height")
        })
        .expect("expected a property row for height");
    let kinds: Vec<&'static str> = row.children().iter().map(|c| c.type_name()).collect();
    assert_eq!(kinds, vec!["ValueEditorWidget"]);
}

/// Doubling the scale must double every visible dimension of the
/// node and its children — bounds widths/heights AND the per-child
/// metrics (header height, row height, socket radius, font sizes are
/// indirectly verified through bounds).
#[test]
fn scaled_layout_doubles_every_dimension() {
    let layout = layout_node(&make_node());
    let ctx_1x = NodePaintContext::from_model_scaled(CanvasPalette::dark(), &DummyModel, 1.0);
    let nw_1x = NodeWidget::from_layout_transformed(&layout, false, ctx_1x, 1.0, [0.0, 0.0]);
    let ctx_2x = NodePaintContext::from_model_scaled(CanvasPalette::dark(), &DummyModel, 2.0);
    let nw_2x = NodeWidget::from_layout_transformed(&layout, false, ctx_2x, 2.0, [0.0, 0.0]);

    assert!(
        (nw_2x.bounds().width - 2.0 * nw_1x.bounds().width).abs() < 1e-6
            && (nw_2x.bounds().height - 2.0 * nw_1x.bounds().height).abs() < 1e-6,
        "NodeWidget bounds must scale with the canvas scale factor"
    );

    let header_1x = &nw_1x.children()[0];
    let header_2x = &nw_2x.children()[0];
    assert!(
        (header_2x.bounds().height - 2.0 * header_1x.bounds().height).abs() < 1e-6,
        "NodeHeaderWidget height must scale with the canvas scale factor"
    );

    let row_1x = nw_1x
        .children()
        .iter()
        .find(|c| c.type_name() == "NodeRowWidget")
        .expect("at least one row");
    let row_2x = nw_2x
        .children()
        .iter()
        .find(|c| c.type_name() == "NodeRowWidget")
        .expect("at least one row");
    assert!(
        (row_2x.bounds().height - 2.0 * row_1x.bounds().height).abs() < 1e-6,
        "NodeRowWidget height must scale with the canvas scale factor"
    );
}

/// Offset moves the bounds by the offset amount (in screen-space).
#[test]
fn offset_translates_node_bounds() {
    let layout = layout_node(&make_node());
    let ctx = NodePaintContext::from_model_scaled(CanvasPalette::dark(), &DummyModel, 1.0);
    let nw = NodeWidget::from_layout_transformed(&layout, false, ctx, 1.0, [25.0, 40.0]);
    // Node was at canvas (10, 50); with scale=1 and offset=(25, 40), the
    // bottom-left in screen-space is (10*1 + 25, (50 - h) * 1 + 40).
    assert!(
        (nw.bounds().x - (layout.top_left[0] + 25.0)).abs() < 1e-6,
        "got x={}",
        nw.bounds().x
    );
    let expected_y = layout.top_left[1] - layout.size[1] + 40.0;
    assert!(
        (nw.bounds().y - expected_y).abs() < 1e-6,
        "expected y={}, got {}",
        expected_y,
        nw.bounds().y
    );
}
