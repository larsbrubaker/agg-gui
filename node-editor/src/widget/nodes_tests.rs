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
    fn noodles(&self) -> Vec<crate::model::NoodleView> {
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
    fn try_add_noodle(
        &mut self,
        _: NodeId,
        _: &str,
        _: NodeId,
        _: &str,
    ) -> crate::model::NoodleResult {
        crate::model::NoodleResult::Rejected
    }
    fn remove_noodle(&mut self, _: NodeId, _: &str, _: NodeId, _: &str) -> bool {
        false
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
            editor_kind: None,
        }],
        error: None,
        warning: None,
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
    let nw_1x = NodeWidget::from_layout_transformed(
        &layout,
        false,
        ctx_1x,
        1.0,
        [0.0, 0.0],
        std::rc::Rc::new(std::cell::Cell::new(None)),
    );
    let ctx_2x = NodePaintContext::from_model_scaled(CanvasPalette::dark(), &DummyModel, 2.0);
    let nw_2x = NodeWidget::from_layout_transformed(
        &layout,
        false,
        ctx_2x,
        2.0,
        [0.0, 0.0],
        std::rc::Rc::new(std::cell::Cell::new(None)),
    );

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
    let nw = NodeWidget::from_layout_transformed(
        &layout,
        false,
        ctx,
        1.0,
        [25.0, 40.0],
        std::rc::Rc::new(std::cell::Cell::new(None)),
    );
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

/// A node whose rows are not all the same height must build its widget
/// tree from the **layout's** row rects rather than from `row_index *
/// ROW_HEIGHT`.
///
/// The two have to agree: the widget tree is what paints (and what the
/// inspector reports), while `NodeLayoutInfo`'s rects are what the
/// canvas hit-tests. A read-only string row is two rows tall
/// (`row_height_for_property`), so with a fixed row pitch every row
/// below one painted a row-height away from where its clicks resolve —
/// a toggle under a hint row committed the hint's row instead of its
/// own.
/// A read-only string property, which the layout gives two rows worth of
/// height.
fn read_only_row(name: &str, bound_input: Option<&str>) -> PropertyView {
    PropertyView {
        name: name.into(),
        display_label: Some(String::new()),
        current: PropertyValue::Other {
            display: "a hint message the renderer wraps across two lines".into(),
        },
        min: None,
        max: None,
        bound_input: bound_input.map(|s| s.to_string()),
        editor: None,
        editor_kind: Some(agg_gui::widgets::EditorKind::StringReadOnly),
    }
}

fn toggle_row(name: &str) -> PropertyView {
    PropertyView {
        name: name.into(),
        display_label: Some("Toggle".into()),
        current: PropertyValue::Bool(false),
        min: None,
        max: None,
        bound_input: None,
        editor: None,
        editor_kind: Some(agg_gui::widgets::EditorKind::Toggle),
    }
}

/// Assert that the widget tree stacks its rows exactly where the layout
/// says they are — every row, at `scale`.
///
/// `NodeRow::height()` is the single pitch both sides use, so this is
/// the invariant that keeps what the user sees and what the canvas
/// hit-tests from drifting apart.
fn assert_row_stack_matches_layout(node: &NodeView, scale: f64, connected: &[&str]) {
    let layout = crate::draw::layout_node_with_state(
        node,
        |name| connected.contains(&name),
        /* collapsed */ false,
    );
    let ctx = NodePaintContext::from_model_scaled(CanvasPalette::dark(), &DummyModel, scale);
    let nw = NodeWidget::from_layout_transformed(
        &layout,
        false,
        ctx,
        scale,
        [0.0, 0.0],
        std::rc::Rc::new(std::cell::Cell::new(None)),
    );
    let widget_rows: Vec<&Box<dyn Widget>> = nw
        .children()
        .iter()
        .filter(|c| c.type_name() == "NodeRowWidget")
        .collect();
    assert_eq!(widget_rows.len(), layout.rows.len(), "row count");

    let screen_h = layout.size[1] * scale;
    let mut offset = 0.0_f64;
    for (i, row) in layout.rows.iter().enumerate() {
        let w = &widget_rows[i];
        let want_top = screen_h - crate::draw::TITLE_HEIGHT * scale - offset * scale;
        let got_top = w.bounds().y + w.bounds().height;
        assert!(
            (got_top - want_top).abs() < 1e-6,
            "row {i} paints with its top at {got_top}, laid out at {want_top}"
        );
        assert!(
            (w.bounds().height - row.height() * scale).abs() < 1e-6,
            "row {i} paints {} tall, laid out {} tall",
            w.bounds().height,
            row.height() * scale
        );
        // The editor, when there is one, fills its row rather than a
        // fixed row's worth — including at scale.
        if let Some(editor) = w
            .children()
            .iter()
            .find(|c| c.type_name() == "ValueEditorWidget")
        {
            let inset = 2.0 * scale;
            assert!(
                (editor.bounds().height - (row.height() * scale - inset)).abs() < 1e-6,
                "row {i}'s editor is {} tall inside a {} row",
                editor.bounds().height,
                w.bounds().height
            );
        }
        offset += row.height();
    }
}

/// The rows below a tall one must line up with the hit layout.
///
/// The widget tree is what paints (and what the inspector reports),
/// while `NodeLayoutInfo`'s rects are what the canvas hit-tests. A
/// read-only string row is two rows tall (`row_height_for_property`), so
/// with a fixed row pitch every row below one painted a row-height away
/// from where its clicks resolve — a toggle under a hint row committed
/// the hint's row instead of its own.
#[test]
fn rows_below_a_tall_row_line_up_with_the_hit_layout() {
    let mut node = make_node();
    node.properties = vec![read_only_row("hint", None), toggle_row("toggle")];
    assert_row_stack_matches_layout(&node, 1.0, &[]);
}

/// The same for a tall row bound to an **input socket**, connected or
/// not. The layout reserves the taller slot either way (the editor is
/// dropped when the socket is wired, the row is not), so the row has to
/// carry its own height rather than the socket branch assuming one.
#[test]
fn a_bound_read_only_row_stacks_by_its_own_height() {
    let mut node = make_node();
    node.properties = vec![read_only_row("hint", Some("Paths")), toggle_row("toggle")];
    for connected in [&[][..], &["Paths"][..]] {
        assert_row_stack_matches_layout(&node, 1.0, connected);
        // The slot itself is the taller one either way — the row keeps
        // the height its property asked for even when the editor is
        // dropped because the socket is wired.
        let layout =
            crate::draw::layout_node_with_state(&node, |name| connected.contains(&name), false);
        let hint = layout
            .rows
            .iter()
            .find(|r| r.socket().map(|s| s.name == "Paths").unwrap_or(false))
            .expect("the bound row exists");
        assert!(
            hint.height() > crate::draw::ROW_HEIGHT,
            "a read-only bound row must reserve more than one row (connected: {connected:?})"
        );
    }
}

/// …and the whole stack scales. A row metric left in logical units
/// while its neighbours are pre-scaled only shows up when the canvas is
/// zoomed.
#[test]
fn the_row_stack_holds_at_double_scale() {
    let mut node = make_node();
    node.properties = vec![read_only_row("hint", Some("Paths")), toggle_row("toggle")];
    assert_row_stack_matches_layout(&node, 2.0, &[]);
}
