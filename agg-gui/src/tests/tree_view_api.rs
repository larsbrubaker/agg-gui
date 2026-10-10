//! `TreeView` public API and behaviour tests: click focus, programmatic
//! selection / cursor / expansion / scrolling, change events, lazy children,
//! node removal, trailing row content, glyph icons, and the performance
//! guarantees (cached rows match the reference walk, only on-screen rows get
//! widgets, a selection change rebuilds no row widget).
//!
//! Timing numbers live in `tree_view_perf.rs`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, Key, Modifiers, MouseButton};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::{paint_subtree, App, Widget};
use crate::widgets::tree_view::{NodeGlyph, NodeIcon, ScrollAlign, TreeView, TreeViewEvent};

const RH: f64 = 20.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(super::TEST_FONT).expect("font"))
}

/// Lay out at `w` x `h` and place the tree at the origin.
fn place(tree: &mut TreeView, w: f64, h: f64) {
    tree.layout(Size::new(w, h));
    tree.set_bounds(Rect::new(0.0, 0.0, w, h));
}

/// Root (expanded) with `n` file children; returns the tree.
fn flat_tree(n: usize) -> TreeView {
    let mut tree = TreeView::new(font()).with_row_height(RH);
    let root = tree.add_root("Root", NodeIcon::Folder);
    tree.expand(root);
    for i in 0..n {
        tree.add_child(root, format!("item {i}"), NodeIcon::File);
    }
    tree
}

fn key(tree: &mut TreeView, k: Key) {
    tree.on_event(&Event::KeyDown {
        key: k,
        modifiers: Modifiers::default(),
    });
}

fn click(tree: &mut TreeView, pos: Point) {
    let m = Modifiers::default();
    tree.on_event(&Event::MouseDown {
        pos,
        button: MouseButton::Left,
        modifiers: m,
    });
    tree.on_event(&Event::MouseUp {
        pos,
        button: MouseButton::Left,
        modifiers: m,
    });
}

/// Centre of display row `i` (top = 0) in a viewport `h` tall.
fn row_center(i: usize, h: f64) -> f64 {
    h - (i as f64 + 0.5) * RH
}

// ---------------------------------------------------------------------------
// 1. Click focus
// ---------------------------------------------------------------------------

/// A click on a row lands on the row's label; the tree must still take the
/// keyboard focus and select the row, so arrow keys work right after.
#[test]
fn click_on_row_focuses_tree_and_selects_row() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&events);
    let tree = flat_tree(5).on_tree_event(move |e| sink.borrow_mut().push(e.clone()));
    let mut app = App::new(Box::new(tree));
    app.layout(Size::new(300.0, 200.0));

    // Row 2 ("item 1"), well inside its label (Y-down screen coordinates).
    app.on_mouse_down(80.0, 2.5 * RH, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(80.0, 2.5 * RH, MouseButton::Left, Modifiers::default());
    assert_eq!(app.focused_widget_type_name(), Some("TreeView"));
    assert_eq!(
        events.borrow().last(),
        Some(&TreeViewEvent::SelectionChanged { cursor: Some(2) })
    );

    app.on_key_down(Key::ArrowDown, Modifiers::default());
    assert_eq!(
        events.borrow().last(),
        Some(&TreeViewEvent::SelectionChanged { cursor: Some(3) }),
        "the focused tree handles the arrow key"
    );
}

/// A double click reports `Activated` for the row (through the App, which
/// carries the click count).
#[test]
fn double_click_activates_row() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&events);
    let tree = flat_tree(3).on_tree_event(move |e| sink.borrow_mut().push(e.clone()));
    let mut app = App::new(Box::new(tree));
    app.layout(Size::new(300.0, 200.0));
    let m = Modifiers::default();
    app.on_mouse_down_clicks(80.0, 1.5 * RH, MouseButton::Left, m, 1);
    app.on_mouse_up(80.0, 1.5 * RH, MouseButton::Left, m);
    assert!(!events.borrow().contains(&TreeViewEvent::Activated(1)));
    app.on_mouse_down_clicks(80.0, 1.5 * RH, MouseButton::Left, m, 2);
    app.on_mouse_up(80.0, 1.5 * RH, MouseButton::Left, m);
    assert!(events.borrow().contains(&TreeViewEvent::Activated(1)));
}

// ---------------------------------------------------------------------------
// 2. Programmatic API and events
// ---------------------------------------------------------------------------

#[test]
fn select_single_moves_the_keyboard_cursor() {
    let mut tree = flat_tree(10);
    place(&mut tree, 300.0, 400.0);
    tree.select_single(4);
    assert_eq!(tree.selected_nodes(), vec![4]);
    assert_eq!(tree.cursor_node(), Some(4));
    assert!(
        tree.take_events().is_empty(),
        "programmatic calls emit nothing"
    );

    key(&mut tree, Key::ArrowDown);
    assert_eq!(
        tree.selected_nodes(),
        vec![5],
        "arrow continues from node 4"
    );
    assert_eq!(
        tree.take_events(),
        vec![TreeViewEvent::SelectionChanged { cursor: Some(5) }]
    );

    tree.set_cursor_node(Some(8));
    assert_eq!(
        tree.selected_nodes(),
        vec![5],
        "cursor moves without selecting"
    );
    key(&mut tree, Key::ArrowUp);
    assert_eq!(tree.selected_nodes(), vec![7]);
    tree.clear_selection();
    assert!(tree.selected_nodes().is_empty());
}

#[test]
fn expand_collapse_and_enter_report_events() {
    let mut tree = TreeView::new(font()).with_row_height(RH);
    let root = tree.add_root("Root", NodeIcon::Folder);
    let a = tree.add_child(root, "a", NodeIcon::Folder);
    tree.add_child(a, "a1", NodeIcon::File);
    place(&mut tree, 300.0, 200.0);

    // Click the root's expand arrow (x in [0, EXPAND_W)).
    click(&mut tree, Point::new(5.0, row_center(0, 200.0)));
    assert_eq!(
        tree.take_events(),
        vec![
            TreeViewEvent::Expanded(root),
            TreeViewEvent::SelectionChanged { cursor: Some(root) },
        ]
    );
    place(&mut tree, 300.0, 200.0);

    key(&mut tree, Key::ArrowDown); // cursor → a
    key(&mut tree, Key::ArrowRight); // expand a
    key(&mut tree, Key::ArrowLeft); // collapse a
    let ev = tree.take_events();
    assert_eq!(
        ev,
        vec![
            TreeViewEvent::SelectionChanged { cursor: Some(a) },
            TreeViewEvent::Expanded(a),
            TreeViewEvent::Collapsed(a),
        ]
    );

    key(&mut tree, Key::Enter);
    assert_eq!(
        tree.take_events(),
        vec![TreeViewEvent::Expanded(a), TreeViewEvent::Activated(a)]
    );
    let mut tree = tree.with_enter_toggles_expansion(false);
    key(&mut tree, Key::Enter);
    assert_eq!(tree.take_events(), vec![TreeViewEvent::Activated(a)]);
    assert!(tree.is_expanded(a), "Enter no longer toggles");
}

#[test]
fn expand_path_to_reveals_a_deep_node() {
    let mut tree = TreeView::new(font()).with_row_height(RH);
    let r = tree.add_root("r", NodeIcon::Folder);
    let a = tree.add_child(r, "a", NodeIcon::Folder);
    let b = tree.add_child(a, "b", NodeIcon::Folder);
    let c = tree.add_child(b, "c", NodeIcon::File);
    assert_eq!(tree.visible_row_of(c), None);
    tree.expand_path_to(c);
    assert_eq!(tree.visible_row_of(c), Some(3));
    assert!(!tree.is_expanded(c));
    tree.collapse(a);
    assert_eq!(tree.visible_row_of(c), None);
    tree.set_expanded(a, true);
    assert_eq!(tree.node_at_row(3), Some(c));
}

#[test]
fn scroll_node_into_view_minimal_and_centred() {
    let mut tree = flat_tree(200); // 201 rows, 10 per viewport
    place(&mut tree, 300.0, 200.0);
    assert_eq!(tree.scroll_offset(), 0.0);

    // Minimal: row 50 ends up as the bottom row.
    tree.scroll_node_into_view(50, ScrollAlign::Minimal);
    place(&mut tree, 300.0, 200.0);
    assert_eq!(tree.scroll_offset(), 51.0 * RH - 200.0);
    let r = tree.node_row_rect(50).expect("visible");
    assert!(r.y >= -0.01 && r.y < RH, "bottom row, got {r:?}");
    // Already in view: no scroll.
    tree.scroll_node_into_view(45, ScrollAlign::Minimal);
    place(&mut tree, 300.0, 200.0);
    assert_eq!(tree.scroll_offset(), 51.0 * RH - 200.0);

    // Centre: row 120's middle sits at the viewport's middle.
    tree.scroll_node_into_view(120, ScrollAlign::Center);
    place(&mut tree, 300.0, 200.0);
    let r = tree.node_row_rect(120).expect("visible");
    assert!((r.y + RH * 0.5 - 100.0).abs() < 0.01, "centred, got {r:?}");

    // Clamped at the end; scroll_to_row by position.
    tree.scroll_to_row(200, ScrollAlign::Center);
    place(&mut tree, 300.0, 200.0);
    assert_eq!(tree.scroll_offset(), 201.0 * RH - 200.0);
    assert_eq!(tree.visible_row_count(), 201);
}

/// A scroll request made right after expanding (before any layout) uses the
/// rows as they will be, not as they were.
#[test]
fn scroll_into_view_after_expand_waits_for_layout() {
    let mut tree = TreeView::new(font()).with_row_height(RH);
    let r = tree.add_root("r", NodeIcon::Folder);
    let mut last = r;
    for i in 0..100 {
        last = tree.add_child(r, format!("c{i}"), NodeIcon::File);
    }
    place(&mut tree, 300.0, 200.0);
    tree.expand_path_to(last);
    tree.select_single(last);
    tree.scroll_node_into_view(last, ScrollAlign::Center);
    place(&mut tree, 300.0, 200.0);
    let rect = tree.node_row_rect(last).expect("visible");
    assert!(rect.y >= 0.0 && rect.y + RH <= 200.0, "in view: {rect:?}");
}

#[test]
fn lazy_children_load_on_expand() {
    let mut tree = TreeView::new(font()).with_row_height(RH);
    let r = tree.add_root("r", NodeIcon::Folder);
    tree.set_node_may_have_children(r, true);
    place(&mut tree, 300.0, 200.0);
    key(&mut tree, Key::ArrowDown); // cursor → r
    tree.take_events();
    key(&mut tree, Key::ArrowRight);
    assert_eq!(tree.take_events(), vec![TreeViewEvent::Expanded(r)]);
    // The owner populates on the event, then lays out.
    tree.add_child(r, "x", NodeIcon::File);
    tree.add_child(r, "y", NodeIcon::File);
    place(&mut tree, 300.0, 200.0);
    assert_eq!(tree.children().len(), 3);
}

#[test]
fn remove_children_and_remove_node_remap_ids() {
    let mut tree = TreeView::new(font()).with_row_height(RH);
    let r = tree.add_root("r", NodeIcon::Folder); // 0
    let a = tree.add_child(r, "a", NodeIcon::Folder); // 1
    tree.add_child(a, "a1", NodeIcon::File); // 2
    tree.add_child(a, "a2", NodeIcon::File); // 3
    let b = tree.add_child(r, "b", NodeIcon::File); // 4
    tree.expand(r);
    tree.expand(a);
    tree.select_single(b);
    place(&mut tree, 300.0, 200.0);
    assert_eq!(tree.visible_row_count(), 5);

    let map = tree.remove_children(a);
    assert_eq!(map, vec![Some(0), Some(1), None, None, Some(2)]);
    assert_eq!(tree.nodes.len(), 3);
    assert_eq!(tree.nodes[2].label, "b");
    assert_eq!(tree.nodes[2].parent, Some(0));
    assert_eq!(tree.cursor_node(), Some(2), "cursor follows its node");
    assert_eq!(tree.selected_nodes(), vec![2]);
    assert!(!tree.is_expanded(1), "emptied node is collapsed");
    place(&mut tree, 300.0, 200.0);
    assert_eq!(tree.children().len(), 3);

    // Re-populate after pruning: new children start at order 0.
    let c = tree.add_child(1, "a-new", NodeIcon::File);
    assert_eq!(tree.nodes[c].order, 0);

    let map = tree.remove_node(1);
    assert_eq!(map, vec![Some(0), None, Some(1), None]);
    assert_eq!(tree.visible_row_count(), 2);
}

/// `add_child` numbers siblings 0, 1, 2 … even with direct `nodes` edits
/// in between (the count is rebuilt when the length changed).
#[test]
fn add_child_order_counts_siblings() {
    let mut tree = TreeView::new(font());
    let r = tree.add_root("r", NodeIcon::Folder);
    let a = tree.add_child(r, "a", NodeIcon::File);
    tree.nodes.push(crate::widgets::tree_view::TreeNode::new(
        "direct",
        NodeIcon::File,
        Some(r),
        1,
    ));
    let b = tree.add_child(r, "b", NodeIcon::File);
    let r2 = tree.add_root("r2", NodeIcon::Folder);
    assert_eq!(tree.nodes[a].order, 0);
    assert_eq!(tree.nodes[b].order, 2);
    assert_eq!(tree.nodes[r2].order, 1);
    tree.nodes.clear();
    let x = tree.add_root("x", NodeIcon::File);
    assert_eq!(tree.nodes[x].order, 0);
}

// ---------------------------------------------------------------------------
// 3. Trailing content and 4. glyph icons
// ---------------------------------------------------------------------------

#[test]
fn secondary_text_and_fraction_sit_at_the_trailing_edge() {
    let mut tree = flat_tree(1);
    tree.set_node_secondary_text(1, Some("12.3 GB".into()));
    tree.set_node_fraction(1, Some(0.5));
    place(&mut tree, 300.0, 200.0);
    let row = &tree.children()[1];
    let parts = row.children();
    let names: Vec<_> = parts.iter().map(|c| c.type_name()).collect();
    assert_eq!(&names[3..], &["Label", "Label", "FractionBar"]);
    let (label, secondary, bar) = (parts[3].bounds(), parts[4].bounds(), parts[5].bounds());
    let row_w = row.bounds().width;
    assert!(
        (bar.x + bar.width - row_w).abs() < 0.5,
        "bar at the right edge"
    );
    assert!(
        secondary.x + secondary.width <= bar.x,
        "text left of the bar"
    );
    assert!(secondary.width > 10.0);
    assert!(
        label.x + label.width <= secondary.x,
        "label clear of the text"
    );
}

/// Count pixels close to `c` in `fb`.
fn count_color(fb: &Framebuffer, c: Color) -> usize {
    let (r, g, b) = (
        (c.r * 255.0) as i32,
        (c.g * 255.0) as i32,
        (c.b * 255.0) as i32,
    );
    fb.pixels()
        .chunks_exact(4)
        .filter(|p| {
            (p[0] as i32 - r).abs() < 40
                && (p[1] as i32 - g).abs() < 40
                && (p[2] as i32 - b).abs() < 40
        })
        .count()
}

/// A glyph icon is drawn as text with the icon font, in its colour, at the
/// current scale: twice the scale covers about four times the pixels
/// (an image raster would be fixed).
#[test]
fn glyph_icon_draws_in_its_colour_at_any_scale() {
    let fa = Arc::new(Font::from_slice(crate::fonts::FONT_AWESOME_4_7).expect("fa"));
    let green = Color::rgb(0.1, 0.8, 0.1);
    let render = |scale: f64| {
        let mut tree = flat_tree(1).with_icon_font(Arc::clone(&fa));
        tree.set_node_icon_glyph(1, Some(NodeGlyph::new('\u{f07b}', green)));
        place(&mut tree, 200.0, 60.0);
        let px = (200.0 * scale) as u32;
        let mut fb = Framebuffer::new(px, (60.0 * scale) as u32);
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.scale(scale, scale);
        paint_subtree(&mut tree, &mut ctx);
        drop(ctx);
        count_color(&fb, green)
    };
    let one = render(1.0);
    let two = render(2.0);
    assert!(one > 20, "glyph painted in its colour ({one} px)");
    let ratio = two as f64 / one as f64;
    assert!(
        (3.0..5.0).contains(&ratio),
        "scales with DPI: {one} → {two}"
    );
}

// ---------------------------------------------------------------------------
// 5. Performance guarantees
// ---------------------------------------------------------------------------

#[test]
fn only_rows_in_the_viewport_get_widgets() {
    let mut tree = flat_tree(10_000);
    place(&mut tree, 300.0, 200.0);
    assert!(
        tree.children().len() <= 11,
        "{} widgets",
        tree.children().len()
    );
    // Scrolled: the widgets are the rows now on screen.
    tree.set_scroll_offset(5_000.0 * RH);
    place(&mut tree, 300.0, 200.0);
    let first = tree.children()[0].bounds();
    assert!(first.y + first.height > 199.0 && first.y < 200.0);
    let row = tree.children()[0]
        .as_any()
        .and_then(|a| a.downcast_ref::<crate::widgets::tree_view::TreeRow>())
        .map(|r| r.node_idx);
    assert_eq!(row, tree.node_at_row(5_000));
    // Hit testing follows the scroll.
    click(&mut tree, Point::new(100.0, row_center(0, 200.0)));
    assert_eq!(tree.selected_nodes(), vec![row.unwrap()]);
}

/// Address of each row widget, to tell reused widgets from rebuilt ones.
fn widget_ids(tree: &TreeView) -> Vec<usize> {
    tree.children()
        .iter()
        .map(|w| w.as_ref() as *const dyn Widget as *const () as usize)
        .collect()
}

#[test]
fn selection_hover_and_focus_changes_rebuild_no_row_widget() {
    let mut tree = flat_tree(30);
    place(&mut tree, 300.0, 200.0);
    let before = widget_ids(&tree);
    tree.on_event(&Event::FocusGained);
    click(&mut tree, Point::new(100.0, row_center(3, 200.0)));
    key(&mut tree, Key::ArrowDown);
    tree.on_event(&Event::MouseMove {
        pos: Point::new(100.0, row_center(5, 200.0)),
    });
    tree.select_single(2);
    place(&mut tree, 300.0, 200.0);
    assert_eq!(widget_ids(&tree), before);
    // A label change rebuilds just that row.
    tree.nodes[2].label = "renamed".into();
    place(&mut tree, 300.0, 200.0);
    let after = widget_ids(&tree);
    let rebuilt = before.iter().zip(&after).filter(|(a, b)| a != b).count();
    assert_eq!(rebuilt, 1);
}

/// The cached rows equal the reference walk after a long series of direct
/// `nodes` edits (expansion, reparenting, reordering, pushes).
#[test]
fn cached_rows_match_reference_after_direct_edits() {
    use crate::widgets::tree_view::TreeNode;
    let mut tree = TreeView::new(font()).with_row_height(RH);
    let mut seed = 0x2545_f491_u64;
    let mut rand = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    for i in 0..200 {
        let parent = if i < 5 { None } else { Some(rand(i)) };
        tree.nodes.push(TreeNode::new(
            format!("n{i}"),
            NodeIcon::Folder,
            parent,
            rand(10) as u32,
        ));
    }
    for step in 0..300 {
        let i = rand(tree.nodes.len());
        match step % 4 {
            0 => tree.nodes[i].is_expanded = !tree.nodes[i].is_expanded,
            1 => tree.nodes[i].order = rand(10) as u32,
            2 if i > 5 => tree.nodes[i].parent = Some(rand(i)),
            _ => {
                tree.add_child(i, "new", NodeIcon::File);
            }
        }
        if step % 7 == 0 {
            tree.nodes[i].may_have_children = !tree.nodes[i].may_have_children;
        }
        let expected: Vec<_> = crate::widgets::tree_view::reference_rows(&tree.nodes);
        let count = tree.visible_row_count();
        let got: Vec<_> = (0..count)
            .map(|r| tree.node_at_row(r).expect("row"))
            .collect();
        assert_eq!(got, expected, "step {step}");
    }
}
