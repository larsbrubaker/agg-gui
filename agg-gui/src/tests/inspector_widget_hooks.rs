//! The inspector's edits on agg-gui's own widgets: the `Widget::try_set_*`
//! overrides (enabled on Button, Checkbox, RadioButton and ChevronWidget;
//! padding on FlexColumn, FlexRow and Container; visible on Window and
//! Conditional), each driven through `apply_widget_base_edit` as the host
//! drains the inspector's queue; hidden widgets listed (dimmed, never
//! outlined); and width / height / margin rows read and edited in the
//! widget's logical units under a scaling ancestor.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::event::{Event, Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor};
use crate::text::Font;
use crate::widget::{
    apply_widget_base_edit, collect_inspector_nodes, InspectorNode, Widget, WidgetBaseEdit,
    WidgetBaseField,
};
use crate::widgets::inspector::InspectorPanel;
use crate::widgets::primitives::SizedBox;
use crate::widgets::{
    Button, Checkbox, ChevronWidget, Conditional, Container, FlexColumn, FlexRow, RadioButton,
    Window,
};

use super::paint_recorder::PaintRecorder;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(super::TEST_FONT).expect("font"))
}

/// Apply `field` to `root`'s child 0, as the host drains an inspector edit.
fn edit_child(root: &mut dyn Widget, field: WidgetBaseField) -> bool {
    apply_widget_base_edit(
        root,
        &WidgetBaseEdit {
            path: vec![0],
            field,
        },
    )
}

/// A column holding `child`, so edits travel the path from the root.
fn holder(child: Box<dyn Widget>) -> FlexColumn {
    FlexColumn::new().add(child)
}

fn assert_enabled_edit(child: Box<dyn Widget>) {
    let mut root = holder(child);
    assert!(root.children()[0].is_enabled());
    assert!(edit_child(&mut root, WidgetBaseField::Enabled(false)));
    assert!(!root.children()[0].is_enabled());
    assert!(!root.children()[0].is_focusable());
    assert!(edit_child(&mut root, WidgetBaseField::Enabled(true)));
    assert!(root.children()[0].is_enabled());
}

#[test]
fn rust_only_enabled_edit_lands_on_button() {
    assert_enabled_edit(Box::new(Button::new("Go", font())));
}

#[test]
fn rust_only_enabled_edit_replaces_a_bound_enabled_closure() {
    let button = Button::new("Go", font()).with_enabled_fn(|| true);
    assert_enabled_edit(Box::new(button));
}

#[test]
fn rust_only_enabled_edit_lands_on_checkbox() {
    assert_enabled_edit(Box::new(Checkbox::new("Check", font(), false)));
}

#[test]
fn rust_only_enabled_edit_lands_on_radio_button() {
    assert_enabled_edit(Box::new(RadioButton::new("Pick", font())));
}

#[test]
fn rust_only_enabled_edit_lands_on_chevron() {
    let mut root = holder(Box::new(ChevronWidget::new(Rc::new(Cell::new(false)))));
    assert!(edit_child(&mut root, WidgetBaseField::Enabled(false)));
    assert!(!root.children()[0].is_enabled());
    assert!(!root.children()[0].hit_test(Point::new(1.0, 1.0)));
}

/// Lay `container` out at 200 x 200 and return its first child's bounds.
fn first_child_bounds(root: &mut dyn Widget) -> Rect {
    root.layout(Size::new(200.0, 200.0));
    root.children()[0].children()[0].bounds()
}

fn assert_padding_edit(container: Box<dyn Widget>) {
    let mut root = holder(container);
    let before = first_child_bounds(&mut root);
    assert!(edit_child(&mut root, WidgetBaseField::PaddingLeft(12.0)));
    assert_eq!(root.children()[0].padding().left, 12.0);
    let after = first_child_bounds(&mut root);
    assert_eq!(after.x, before.x + 12.0, "the next layout insets the child");
    // Other sides are kept, and a negative value stops at zero.
    assert!(edit_child(&mut root, WidgetBaseField::PaddingTop(-3.0)));
    let p = root.children()[0].padding();
    assert_eq!((p.left, p.top), (12.0, 0.0));
}

fn sized_child() -> Box<dyn Widget> {
    Box::new(SizedBox::fixed(20.0, 20.0))
}

#[test]
fn rust_only_padding_edit_lands_on_flex_column() {
    assert_padding_edit(Box::new(FlexColumn::new().add(sized_child())));
}

#[test]
fn rust_only_padding_edit_lands_on_flex_row() {
    assert_padding_edit(Box::new(FlexRow::new().add(sized_child())));
}

#[test]
fn rust_only_padding_edit_lands_on_container() {
    let container = Container::new().with_padding(0.0).add(sized_child());
    assert_padding_edit(Box::new(container));
}

#[test]
fn rust_only_visible_edit_lands_on_conditional() {
    let cell = Rc::new(Cell::new(true));
    let mut root = holder(Box::new(Conditional::new(Rc::clone(&cell), sized_child())));
    assert!(edit_child(&mut root, WidgetBaseField::Visible(false)));
    assert!(!cell.get(), "the shared cell the owner reads changes too");
    assert!(!root.children()[0].is_visible());
    assert!(edit_child(&mut root, WidgetBaseField::Visible(true)));
    assert!(root.children()[0].is_visible());
}

#[test]
fn rust_only_visible_edit_lands_on_window_and_its_cell() {
    let mut root = holder(Box::new(Window::new("W", font(), sized_child())));
    assert!(root.children()[0].is_visible());
    assert!(edit_child(&mut root, WidgetBaseField::Visible(false)));
    assert!(!root.children()[0].is_visible());

    let cell = Rc::new(Cell::new(false));
    let wired = Window::new("W", font(), sized_child()).with_visible_cell(Rc::clone(&cell));
    let mut root = holder(Box::new(wired));
    assert!(!root.children()[0].is_visible());
    assert!(edit_child(&mut root, WidgetBaseField::Visible(true)));
    assert!(cell.get(), "a wired window shows through its cell");
    assert!(root.children()[0].is_visible());
}

fn press(panel: &mut InspectorPanel, pos: Point) {
    let (button, modifiers) = (MouseButton::Left, Modifiers::default());
    panel.on_event(&Event::MouseDown {
        pos,
        button,
        modifiers,
    });
    panel.on_event(&Event::MouseUp {
        pos,
        button,
        modifiers,
    });
}

fn node(depth: usize, path: Vec<usize>, bounds: Rect, scale: f64) -> InspectorNode {
    InspectorNode {
        type_name: "Node",
        screen_bounds: bounds,
        margin: Insets::all(4.0 * scale),
        padding: Insets::ZERO,
        scale,
        h_anchor: HAnchor::FIT,
        v_anchor: VAnchor::FIT,
        depth,
        path,
        properties: vec![],
        visible: true,
        enabled: true,
        child_count: 0,
    }
}

/// The value text painted on the row labelled `label` (labels at x 12).
fn row_value(rec: &PaintRecorder, label: &str) -> Option<String> {
    let (_, _, y) = rec
        .texts
        .iter()
        .find(|(t, x, _)| t == label && *x == 12.0)?;
    rec.texts
        .iter()
        .find(|(t, x, ty)| ty == y && *x > 12.0 && t != label)
        .map(|(t, _, _)| t.clone())
}

#[test]
fn rust_only_size_and_margin_rows_use_logical_units_under_a_scaling_ancestor() {
    // A widget drawn at twice its size: 200 x 80 on screen, 100 x 40 logical.
    let nodes = Rc::new(RefCell::new(vec![
        node(0, vec![], Rect::new(0.0, 0.0, 400.0, 400.0), 1.0),
        node(1, vec![0], Rect::new(10.0, 10.0, 200.0, 80.0), 2.0),
    ]));
    let queue = Rc::new(RefCell::new(Vec::new()));
    let mut panel = InspectorPanel::new(font(), Rc::clone(&nodes), Rc::new(RefCell::new(None)))
        .with_base_edit_queue(Rc::clone(&queue));
    panel.set_bounds(Rect::new(0.0, 0.0, 250.0, 680.0));
    panel.layout(Size::new(250.0, 680.0));
    panel.select(Some(1));
    panel.set_properties_height(400.0);
    panel.layout(Size::new(250.0, 680.0));
    let mut rec = PaintRecorder::new();
    panel.paint(&mut rec);
    assert_eq!(row_value(&rec, "width").as_deref(), Some("100.0"));
    assert_eq!(row_value(&rec, "height").as_deref(), Some("40.0"));
    assert_eq!(row_value(&rec, "margin.left").as_deref(), Some("4.0"));

    let strip = panel.value_bounds("width").expect("an editable row");
    press(
        &mut panel,
        Point::new(strip.x + strip.width - 3.0, strip.y + strip.height * 0.5),
    );
    let edits = queue.borrow();
    assert_eq!(edits.len(), 1);
    // NumericStep of the logical 100: 5%.
    assert!(matches!(edits[0].field, WidgetBaseField::Width(w) if w == 105.0));
}

/// Root with a visible child and a hidden one that holds a grandchild.
struct Parent {
    bounds: Rect,
    visible: bool,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Parent {
    fn type_name(&self) -> &'static str {
        if self.visible {
            "Shown"
        } else {
            "Hidden"
        }
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn is_visible(&self) -> bool {
        self.visible
    }
    fn layout(&mut self, available: Size) -> Size {
        available
    }
    fn paint(&mut self, _: &mut dyn crate::draw_ctx::DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> crate::event::EventResult {
        crate::event::EventResult::Ignored
    }
}

fn parent(visible: bool, children: Vec<Box<dyn Widget>>) -> Box<dyn Widget> {
    Box::new(Parent {
        bounds: Rect::new(0.0, 0.0, 50.0, 20.0),
        visible,
        children,
    })
}

#[test]
fn rust_only_hidden_rows_are_dimmed_and_never_outlined() {
    let root = parent(
        true,
        vec![
            parent(true, vec![]),
            parent(false, vec![parent(true, vec![])]),
        ],
    );
    let mut list = Vec::new();
    collect_inspector_nodes(root.as_ref(), 0, Point::ORIGIN, &mut list);
    assert_eq!(list.len(), 4);
    assert!(crate::widget::inspector_node_on_screen(&list, 1));
    assert!(!crate::widget::inspector_node_on_screen(&list, 2));
    assert!(
        !crate::widget::inspector_node_on_screen(&list, 3),
        "a visible widget under a hidden one occupies no pixels"
    );

    let nodes = Rc::new(RefCell::new(list));
    let hovered = Rc::new(RefCell::new(None));
    let mut panel = InspectorPanel::new(font(), Rc::clone(&nodes), Rc::clone(&hovered));
    panel.set_bounds(Rect::new(0.0, 0.0, 250.0, 600.0));
    panel.layout(Size::new(250.0, 600.0));
    let dimmed: Vec<bool> = panel.tree_view.nodes.iter().map(|n| n.dimmed).collect();
    // agg-sharp's InspectorTreeView dims by the widget's own flag.
    assert_eq!(dimmed, vec![false, false, true, false]);

    // Hovering the hidden row outlines nothing; the visible row outlines.
    let row_h = 20.0; // the inspector tree's row height
    let top = panel.tree_view.bounds().y + panel.tree_view.bounds().height;
    let row_y = |i: f64| top - row_h * (i + 0.5);
    panel.on_event(&Event::MouseMove {
        pos: Point::new(100.0, row_y(2.0)),
    });
    assert_eq!(panel.tree_view.hovered_node_idx(), Some(2));
    assert!(hovered.borrow().is_none(), "a hidden widget has no outline");
    panel.layout(Size::new(250.0, 600.0));
    assert!(hovered.borrow().is_none(), "nor after the next layout");
    panel.on_event(&Event::MouseMove {
        pos: Point::new(100.0, row_y(1.0)),
    });
    panel.layout(Size::new(250.0, 600.0));
    assert!(hovered.borrow().is_some());
}

/// Hidden widgets stay in the inspector snapshot, subtrees included, with
/// their `visible` flag false: agg-sharp's `InspectorModel.Collect` walks
/// every child whatever its `Visible`, so the inspector's `visible` row can
/// show a hidden widget again (`CollectSnapshotsEveryWidgetDepthFirstInRootCoordinates`
/// asserts the "Hidden" node).
#[test]
fn test_invisible_widget_listed_in_inspector() {
    use crate::draw_ctx::DrawCtx;
    use crate::event::{Event, EventResult};
    use crate::geometry::{Point, Rect, Size};
    use crate::widget::{collect_inspector_nodes, Widget};

    /// Minimal widget whose visibility can be toggled.
    struct ToggleWidget {
        bounds: Rect,
        visible: bool,
        children: Vec<Box<dyn Widget>>,
    }
    impl Widget for ToggleWidget {
        fn type_name(&self) -> &'static str {
            "ToggleWidget"
        }
        fn is_visible(&self) -> bool {
            self.visible
        }
        fn bounds(&self) -> Rect {
            self.bounds
        }
        fn set_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn children(&self) -> &[Box<dyn Widget>] {
            &self.children
        }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            &mut self.children
        }
        fn layout(&mut self, available: Size) -> Size {
            available
        }
        fn paint(&mut self, _: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }
    }

    let visible = ToggleWidget {
        bounds: Rect::new(0.0, 0.0, 100.0, 40.0),
        visible: true,
        children: Vec::new(),
    };
    let hidden = ToggleWidget {
        bounds: Rect::new(0.0, 50.0, 100.0, 40.0),
        visible: false,
        children: vec![Box::new(ToggleWidget {
            bounds: Rect::new(5.0, 5.0, 10.0, 10.0),
            visible: true,
            children: Vec::new(),
        })],
    };

    let mut nodes = Vec::new();
    collect_inspector_nodes(&visible, 0, Point::ORIGIN, &mut nodes);
    assert_eq!(nodes.len(), 1, "visible widget appears once");
    assert_eq!(nodes[0].type_name, "ToggleWidget");

    assert!(nodes[0].visible);

    nodes.clear();
    collect_inspector_nodes(&hidden, 0, Point::ORIGIN, &mut nodes);
    assert_eq!(nodes.len(), 2, "a hidden widget and its child are listed");
    assert!(!nodes[0].visible, "the row carries the widget's own flag");
    assert!(nodes[1].visible);
    assert_eq!(nodes[1].depth, 1);
    assert_eq!(nodes[1].screen_bounds, Rect::new(5.0, 55.0, 10.0, 10.0));
}
