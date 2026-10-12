//! Port of agg-sharp's `InspectorTests` (Tests/Agg.Tests/Agg.UI/InspectorTests.cs)
//! for the tests no earlier agg-gui test asserts: refresh and selection by
//! widget, the header, edits, value clicks, the split, the property rows and
//! box preview, arrow keys, and saved state.
//!
//! agg-gui's inspector differs in shape from the C# one it inspired: the host
//! collects an `InspectorNode` snapshot (`collect_inspector_nodes`) into a
//! shared cell and applies queued `WidgetBaseEdit`s itself, and the panel paints
//! its header and properties pane directly rather than as child widgets.  Each
//! test drives that real pipeline: a sample tree of `Fixed` widgets (the C#
//! `GuiWidget` page, box, leaf, hidden and disabled widgets), a real
//! `InspectorPanel`, and synthetic events in panel-local coordinates.  Where the
//! two differ the comment at the assertion says how.  Invisible widgets are left
//! out of agg-gui's snapshot, so the C# five nodes are four here (no "Hidden").

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{
    apply_widget_base_edit, collect_inspector_nodes, InspectorNode, Widget, WidgetBaseEdit,
    WidgetBaseField,
};
use crate::widgets::inspector::{next_h_anchor, next_v_anchor, InspectorPanel};
use crate::widgets::inspector_props::BOX_PREVIEW_FILL;

use super::paint_recorder::PaintRecorder;

/// Snapshot positions of the sample tree's widgets.
const PAGE: usize = 0;
const BOX: usize = 1;
const LEAF: usize = 2;

/// A fixed-size widget placed at its `WidgetBase::origin`: C#'s plain
/// `GuiWidget(width, height)` with `Position`, `Margin` and `Padding`.
struct Fixed {
    name: &'static str,
    bounds: Rect,
    size: Size,
    padding: Insets,
    visible: bool,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
}

impl Fixed {
    fn new(name: &'static str, w: f64, h: f64, x: f64, y: f64) -> Self {
        let mut base = WidgetBase::new()
            .with_h_anchor(HAnchor::ABSOLUTE)
            .with_v_anchor(VAnchor::ABSOLUTE);
        base.origin = Point::new(x, y);
        Self {
            name,
            bounds: Rect::new(x, y, w, h),
            size: Size::new(w, h),
            padding: Insets::ZERO,
            visible: true,
            base,
            children: Vec::new(),
        }
    }
}

impl Widget for Fixed {
    fn type_name(&self) -> &'static str {
        self.name
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn padding(&self) -> Insets {
        self.padding
    }
    fn is_visible(&self) -> bool {
        self.visible
    }
    fn layout(&mut self, _available: Size) -> Size {
        for child in &mut self.children {
            let origin = child.widget_base().map(|b| b.origin).unwrap_or_default();
            let size = child.layout(self.size);
            child.set_bounds(Rect::new(origin.x, origin.y, size.width, size.height));
        }
        self.size
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// A page with a box (margin and padding) holding a leaf, a hidden widget and
/// a disabled one.
fn sample_tree() -> Fixed {
    let mut page = Fixed::new("Page", 600.0, 400.0, 0.0, 0.0);
    let mut boxed = Fixed::new("Box", 200.0, 100.0, 20.0, 30.0);
    boxed.base.margin = Insets::all(4.0);
    boxed.padding = Insets::all(6.0);
    boxed
        .children
        .push(Box::new(Fixed::new("Leaf", 60.0, 20.0, 10.0, 5.0)));
    page.children.push(Box::new(boxed));
    let mut hidden = Fixed::new("Hidden", 10.0, 10.0, 0.0, 0.0);
    hidden.visible = false;
    page.children.push(Box::new(hidden));
    page.children
        .push(Box::new(Fixed::new("Disabled", 20.0, 12.0, 0.0, 0.0)));
    page.layout(Size::new(600.0, 400.0));
    page
}

fn collect(page: &Fixed) -> Vec<InspectorNode> {
    let mut out = Vec::new();
    collect_inspector_nodes(page, 0, Point::ORIGIN, &mut out);
    out
}

/// The host's refresh: re-collect the snapshot and lay the panel out again.
fn refresh(page: &mut Fixed, nodes: &Rc<RefCell<Vec<InspectorNode>>>, panel: &mut InspectorPanel) {
    page.layout(Size::new(600.0, 400.0));
    *nodes.borrow_mut() = collect(page);
    let b = panel.bounds();
    panel.layout(Size::new(b.width, b.height));
}

fn new_panel(nodes: &Rc<RefCell<Vec<InspectorNode>>>) -> InspectorPanel {
    let font = Arc::new(Font::from_slice(super::TEST_FONT).expect("font"));
    InspectorPanel::new(font, Rc::clone(nodes), Rc::new(RefCell::new(None)))
}

fn place(panel: &mut InspectorPanel, w: f64, h: f64) {
    panel.set_bounds(Rect::new(0.0, 0.0, w, h));
    panel.layout(Size::new(w, h));
}

/// The sample page and a 250 x `h` panel over its snapshot, laid out.
fn page_with_panel(h: f64) -> (Fixed, Rc<RefCell<Vec<InspectorNode>>>, InspectorPanel) {
    let page = sample_tree();
    let nodes = Rc::new(RefCell::new(collect(&page)));
    let mut panel = new_panel(&nodes);
    place(&mut panel, 250.0, h);
    (page, nodes, panel)
}

fn press(panel: &mut InspectorPanel, pos: Point) {
    let modifiers = Modifiers::default();
    let button = MouseButton::Left;
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

fn mouse(panel: &mut InspectorPanel, event: &str, x: f64, y: f64) {
    let pos = Point::new(x, y);
    let modifiers = Modifiers::default();
    let button = MouseButton::Left;
    panel.on_event(&match event {
        "down" => Event::MouseDown {
            pos,
            button,
            modifiers,
        },
        "up" => Event::MouseUp {
            pos,
            button,
            modifiers,
        },
        _ => Event::MouseMove { pos },
    });
}

fn key(panel: &mut InspectorPanel, key: Key) {
    let modifiers = Modifiers::default();
    panel.on_event(&Event::KeyDown { key, modifiers });
}

fn paint(panel: &mut InspectorPanel) -> PaintRecorder {
    let mut rec = PaintRecorder::new();
    panel.paint(&mut rec);
    rec
}

/// Labels of the property rows, top to bottom (rows draw their label at x 12).
fn row_labels(rec: &PaintRecorder, panel: &InspectorPanel) -> Vec<String> {
    let pane_top = panel.split_line_y() - 2.0;
    rec.texts
        .iter()
        .filter(|(_, x, y)| *x == 12.0 && *y < pane_top)
        .map(|(t, _, _)| t.clone())
        .collect()
}

/// The value text painted on the row labelled `label`.
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
fn refresh_follows_the_live_tree_and_keeps_the_selection_by_widget() {
    let (mut page, nodes, mut panel) = page_with_panel(380.0);
    panel.set_expanded(BOX, false);
    panel.select(Some(LEAF));
    assert!(panel.is_expanded(BOX), "selecting reveals the row");

    // Nothing changed: the host's identical snapshot keeps the selection.
    // (C#'s `Refresh()` also reports whether anything changed; agg-gui's
    // host owns collection, so the panel has no such return value.)
    refresh(&mut page, &nodes, &mut panel);
    assert_eq!(panel.selected(), Some(LEAF));

    // A move is picked up and the selection follows the widget to its new node.
    page.children[0].widget_base_mut().unwrap().origin = Point::new(100.0, 30.0);
    refresh(&mut page, &nodes, &mut panel);
    assert_eq!(panel.selected(), Some(LEAF));
    assert_eq!(nodes.borrow()[LEAF].type_name, "Leaf");
    assert_eq!(nodes.borrow()[LEAF].screen_bounds.x, 110.0);

    page.children
        .push(Box::new(Fixed::new("Added", 5.0, 5.0, 0.0, 0.0)));
    refresh(&mut page, &nodes, &mut panel);
    assert_eq!(nodes.borrow().last().unwrap().type_name, "Added");
    assert_eq!(panel.selected(), Some(LEAF));

    // A removed widget takes its selection with it (the node now at its
    // position is another widget).
    page.children[0].children_mut().remove(0);
    refresh(&mut page, &nodes, &mut panel);
    assert_eq!(panel.selected(), None);
}

#[test]
fn header_is_a_tinted_bar_titled_widget_inspector_with_the_count_at_its_right() {
    use crate::framebuffer::Framebuffer;
    use crate::gfx_ctx::GfxCtx;

    let (_page, nodes, mut panel) = page_with_panel(380.0);
    let rec = paint(&mut panel);
    let w = 250.0;
    let title = rec.texts.iter().find(|(t, _, _)| t == "Widget Inspector");
    let (_, title_x, title_y) = title.expect("the header is titled Widget Inspector");
    let count_text = format!("{} widgets", nodes.borrow().len());
    let (_, count_x, count_y) = rec
        .texts
        .iter()
        .find(|(t, _, _)| *t == count_text)
        .expect("the header shows the widget count");
    // The recorder measures seven units a character.
    let count_right = count_x + 7.0 * count_text.chars().count() as f64;
    assert!(count_right > w - 11.0, "the count sits at the right");
    assert!(*title_x < 13.0);
    // Both sit in the header bar, the top 30 units (C#'s HeaderHeight).
    assert!(*title_y > 380.0 - 30.0 && *count_y > 380.0 - 30.0);

    // The bar is tinted with the header colour across the panel, all 30 high.
    let mut fb = Framebuffer::new(250, 380);
    let header = {
        let mut ctx = GfxCtx::new(&mut fb);
        let header = crate::widgets::inspector::c_header_bg(&ctx.visuals());
        panel.paint(&mut ctx);
        header
    };
    let pixel = |x: usize, y: usize| {
        let i = (y * 250 + x) * 4;
        let p = &fb.pixels()[i..i + 4];
        [
            p[0] as f64 / 255.0,
            p[1] as f64 / 255.0,
            p[2] as f64 / 255.0,
        ]
    };
    let near = |c: [f64; 3]| {
        (c[0] - header.r as f64).abs() < 0.02
            && (c[1] - header.g as f64).abs() < 0.02
            && (c[2] - header.b as f64).abs() < 0.02
    };
    for (x, y) in [(3, 378), (240, 378), (200, 352), (200, 377)] {
        assert!(
            near(pixel(x, y)),
            "header tint at ({x}, {y}): {:?}",
            pixel(x, y)
        );
    }
    assert!(
        !near(pixel(200, 340)),
        "below the bar is not the header tint"
    );
}

#[test]
fn edits_apply_every_field_and_stop_at_zero() {
    let mut page = sample_tree();
    let nodes = collect(&page);
    let box_path = nodes[BOX].path.clone();
    let leaf_path = nodes[LEAF].path.clone();
    let edit = |path: &Vec<usize>, field| WidgetBaseEdit {
        path: path.clone(),
        field,
    };

    assert!(apply_widget_base_edit(
        &mut page,
        &edit(&box_path, WidgetBaseField::MarginLeft(9.0))
    ));
    // C# steps padding.top to -3 here; agg-gui's padding belongs to each
    // container and is read-only in the inspector, so the zero floor is
    // checked on a margin side instead.  C#'s Visible and Width fields have
    // no agg-gui counterpart (no generic visibility or size setter).
    apply_widget_base_edit(
        &mut page,
        &edit(&box_path, WidgetBaseField::MarginBottom(-3.0)),
    );
    apply_widget_base_edit(
        &mut page,
        &edit(&box_path, WidgetBaseField::HAnchor(HAnchor::STRETCH)),
    );
    apply_widget_base_edit(
        &mut page,
        &edit(&box_path, WidgetBaseField::VAnchor(VAnchor::TOP)),
    );

    let base = page.children[0].widget_base().unwrap();
    assert_eq!(
        base.margin,
        Insets {
            left: 9.0,
            right: 4.0,
            top: 4.0,
            bottom: 0.0
        },
        "a size never goes below zero"
    );
    assert_eq!(base.h_anchor, HAnchor::STRETCH);
    assert_eq!(base.v_anchor, VAnchor::TOP);
    assert_eq!(next_h_anchor(HAnchor::RIGHT), HAnchor::ABSOLUTE);
    assert_eq!(next_v_anchor(VAnchor::ABSOLUTE), VAnchor::FIT);

    // A removed widget is left alone.
    page.children[0].children_mut().remove(0);
    let stale = edit(&leaf_path, WidgetBaseField::MarginLeft(1.0));
    assert!(
        !apply_widget_base_edit(&mut page, &stale),
        "a closed widget is left alone"
    );
}

#[test]
fn clicking_a_value_queues_an_edit_that_lands_on_idle() {
    let (mut page, nodes, panel) = page_with_panel(680.0);
    let queue = Rc::new(RefCell::new(Vec::new()));
    let mut panel = panel.with_base_edit_queue(Rc::clone(&queue));
    panel.select(Some(BOX));
    // every row shows once the split gives the pane more than its default
    panel.set_properties_height(400.0);
    place(&mut panel, 250.0, 680.0);
    paint(&mut panel);

    let click_value = |panel: &mut InspectorPanel, row: &str, right_half: bool| {
        let strip = panel.value_bounds(row).expect("an editable row");
        let x = if right_half {
            strip.x + strip.width - 3.0
        } else {
            strip.x + 3.0
        };
        press(panel, Point::new(x, strip.y + strip.height * 0.5));
    };
    let land = |page: &mut Fixed, panel: &mut InspectorPanel| {
        for e in queue.borrow_mut().drain(..) {
            apply_widget_base_edit(page, &e);
        }
        refresh(page, &nodes, panel);
        paint(panel)
    };

    // The right half of margin.left steps it up by agg-gui's inset step;
    // nothing changes until the host applies the queue.
    click_value(&mut panel, "margin.left", true);
    assert_eq!(queue.borrow().len(), 1);
    assert_eq!(page.children[0].margin().left, 4.0);
    let rec = land(&mut page, &mut panel);
    let inset_step = |v: f64| (v.abs() * 0.5 + 0.5).clamp(0.5, 4.0);
    assert_eq!(page.children[0].margin().left, 4.0 + inset_step(4.0));
    assert_eq!(
        row_value(&rec, "margin.left").as_deref(),
        Some("6.5"),
        "the pane re-collects after the edit lands"
    );

    // The left half steps down; an anchor picks the next in the cycle.
    // (C#'s visible row has no agg-gui counterpart.)
    click_value(&mut panel, "margin.top", false);
    click_value(&mut panel, "h_anchor", false);
    land(&mut page, &mut panel);
    assert_eq!(page.children[0].margin().top, 4.0 - inset_step(4.0));
    assert_eq!(
        page.children[0].h_anchor(),
        next_h_anchor(HAnchor::ABSOLUTE)
    );

    // A read-only row (x, and padding, as agg-gui's) queues nothing.
    let rec = paint(&mut panel);
    for row in ["x", "pad.right"] {
        assert!(panel.value_bounds(row).is_none());
        let (_, _, y) = rec
            .texts
            .iter()
            .find(|(t, x, _)| t == row && *x == 12.0)
            .unwrap();
        press(&mut panel, Point::new(250.0 - 3.0, y + 4.0));
    }
    assert_eq!(queue.borrow().len(), 0);
}

#[test]
fn dragging_the_split_resizes_the_panes_down_to_their_minimums() {
    let (_page, _nodes, mut panel) = page_with_panel(380.0);
    const DEFAULT: f64 = 180.0;
    const MIN_PANE: f64 = 60.0;
    let line = panel.split_line_y();
    assert_eq!(panel.properties_height(), DEFAULT);
    // agg-gui's 6-unit gap: the properties end 2 below the line and the tree starts 4 above it.
    assert_eq!(line, DEFAULT);
    assert_eq!(panel.tree_view.bounds().y, line + 4.0);
    let rec = paint(&mut panel);
    assert!(rec.fills.len() > 1);

    // A press 4 below the line is past the gap, in the properties pane, yet inside the 5-unit grab zone.
    mouse(&mut panel, "down", 100.0, line - 4.0);
    mouse(&mut panel, "move", 100.0, line - 4.0 - 10.0);
    assert!((panel.properties_height() - (DEFAULT - 4.0 - 10.0)).abs() < 0.001);
    mouse(&mut panel, "up", 100.0, line - 14.0);
    panel.set_properties_height(DEFAULT);
    place(&mut panel, 250.0, 380.0);

    // Grab 4 units above the line (past the gap into the tree) and drag up 40: the line moves 40.
    mouse(&mut panel, "down", 100.0, line + 4.0);
    mouse(&mut panel, "move", 100.0, line + 4.0 + 40.0);
    assert!((panel.properties_height() - (DEFAULT + 4.0 + 40.0)).abs() < 0.001);

    // Past either end each pane keeps 60.
    mouse(&mut panel, "move", 100.0, -500.0);
    assert_eq!(panel.properties_height(), MIN_PANE);
    mouse(&mut panel, "move", 100.0, 5000.0);
    assert_eq!(
        panel.properties_height(),
        panel.list_area_height() - MIN_PANE
    );
    mouse(&mut panel, "up", 100.0, 5000.0);
    place(&mut panel, 250.0, 380.0);
    assert_eq!(panel.tree_view.bounds().height, MIN_PANE - (6.0 - 2.0));

    // Released, moving the mouse no longer drags.
    panel.on_event(&Event::MouseMove {
        pos: Point::new(100.0, 100.0),
    });
    assert_eq!(
        panel.properties_height(),
        panel.list_area_height() - MIN_PANE
    );
}

#[test]
fn rows_keep_agg_gui_order_and_the_box_preview_shows_the_widget_shape() {
    let (_page, _nodes, mut panel) = page_with_panel(380.0);
    panel.select(Some(BOX));
    panel.set_properties_height(400.0);
    place(&mut panel, 250.0, 680.0);
    let rec = paint(&mut panel);
    let labels = row_labels(&rec, &panel);
    let expected = [
        "x",
        "y",
        "width",
        "height",
        "depth",
        "margin.left",
        "margin.right",
        "margin.top",
        "margin.bottom",
        "pad.left",
        "pad.right",
        "pad.top",
        "pad.bottom",
        "h_anchor",
        "v_anchor",
    ];
    // C# ends with visible, enabled and children; agg-gui follows the anchors
    // with each widget's own properties, led by its backbuffer flag.
    assert_eq!(&labels[..expected.len()], &expected[..]);
    assert_eq!(labels[expected.len()], "backbuffer");

    // Margins and anchors are editable; padding is read-only.
    assert!(panel.value_bounds("margin.top").is_some());
    assert!(panel.value_bounds("pad.top").is_none());
    assert!(panel.value_bounds("h_anchor").is_some());

    // A widget with no padding has no padding rows.
    panel.select(Some(LEAF));
    let rec = paint(&mut panel);
    assert!(!row_labels(&rec, &panel).iter().any(|l| l == "pad.left"));

    // The default pane has no room below the rows; a tall one draws the 2:1 box.
    panel.select(Some(BOX));
    panel.set_properties_height(180.0);
    place(&mut panel, 250.0, 380.0);
    paint(&mut panel);
    assert_eq!(panel.box_preview_bounds(), None);
    panel.set_properties_height(1000.0);
    place(&mut panel, 250.0, 380.0);
    paint(&mut panel);
    assert_eq!(
        panel.box_preview_bounds(),
        None,
        "the panel is too short for the room"
    );

    place(&mut panel, 250.0, 680.0);
    let rec = paint(&mut panel);
    let preview = panel.box_preview_bounds().expect("room for the preview");
    assert!((preview.width - preview.height * 2.0).abs() < 0.001);
    assert_eq!(
        rec.fills_with(BOX_PREVIEW_FILL),
        1,
        "the preview is tinted blue"
    );
}

#[test]
fn arrow_keys_move_the_selection_and_expand_as_agg_gui_tree_view() {
    let (_page, _nodes, mut panel) = page_with_panel(380.0);
    paint(&mut panel);

    // Clicking the Page row selects it and gives the tree the keys.
    let row = panel
        .tree_view
        .node_row_rect(PAGE)
        .expect("the Page row shows");
    let tree_y = panel.tree_view.bounds().y;
    press(
        &mut panel,
        Point::new(row.x + row.width * 0.5, tree_y + row.y + row.height * 0.5),
    );
    assert!(panel.is_focusable());
    assert!(panel.tree_has_focus());
    assert_eq!(panel.selected(), Some(PAGE));

    key(&mut panel, Key::ArrowDown);
    assert_eq!(panel.selected(), Some(BOX));
    key(&mut panel, Key::ArrowRight);
    assert_eq!(
        panel.selected(),
        Some(LEAF),
        "right on an expanded row moves down"
    );
    key(&mut panel, Key::ArrowLeft);
    assert_eq!(
        panel.selected(),
        Some(BOX),
        "left on a leaf moves to its parent"
    );
    key(&mut panel, Key::ArrowLeft);
    assert!(!panel.is_expanded(BOX), "left collapses an expanded row");
    key(&mut panel, Key::ArrowRight);
    assert!(panel.is_expanded(BOX), "right expands a collapsed row");
    assert_eq!(panel.selected(), Some(BOX));
    key(&mut panel, Key::Enter);
    assert!(!panel.is_expanded(BOX), "enter toggles");
    key(&mut panel, Key::ArrowUp);
    assert_eq!(panel.selected(), Some(PAGE));
    key(&mut panel, Key::ArrowUp);
    assert_eq!(
        panel.selected(),
        Some(PAGE),
        "the cursor stops at the first row"
    );
}

#[test]
fn saved_state_restores_expansion_selection_and_split_by_position() {
    let (_page, _nodes, mut panel) = page_with_panel(380.0);
    panel.select(Some(LEAF));
    panel.set_expanded(PAGE, false);
    panel.set_properties_height(150.0);
    let saved = panel.saved_state();
    // Everything but the collapsed page starts open (agg-gui's snapshot
    // leaves the hidden widget out, so C#'s first five are four here).
    assert_eq!(&saved.expanded[..4], &[false, true, true, true]);
    assert_eq!(saved.selected, Some(LEAF));

    // A new run: new widgets, same shape. The state waits for the first collection.
    let next_page = sample_tree();
    let next_nodes = Rc::new(RefCell::new(Vec::new()));
    let cell = Rc::new(RefCell::new(None));
    let mut next = new_panel(&next_nodes).with_snapshot_cell(Rc::clone(&cell));
    next.apply_saved_state(saved);
    assert_eq!(next.properties_height(), 150.0);
    *next_nodes.borrow_mut() = collect(&next_page);
    place(&mut next, 250.0, 380.0);
    assert_eq!(next.selected(), Some(LEAF));
    assert!(!next.is_expanded(PAGE));
    assert!(next.is_expanded(BOX));
    assert!(
        cell.borrow().is_some(),
        "an app saving on change hears about it"
    );
}
