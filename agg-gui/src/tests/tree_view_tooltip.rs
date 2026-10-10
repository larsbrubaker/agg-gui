//! Per-row tooltips on `TreeView` (`TreeView::set_node_tooltip`, asked for
//! by HDTreeMap): hovering a row shows its node's tooltip, keyed by node so
//! moving to another row re-arms the tip.  When the row's name is also
//! elided, the tip is the full name with the node's tooltip on the next line.

use std::sync::Arc;

use crate::event::Event;
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::tree_view::{NodeIcon, TreeView};

const RH: f64 = 20.0;
const H: f64 = 200.0;
const LONG: &str = "demo_wasm_bg.wasm";

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(super::TEST_FONT).expect("font"))
}

/// Root (row 0) with a long-named child carrying trailing text (row 1) and a
/// short-named child (row 2), plus the node indices of the two children.
fn tree() -> (TreeView, usize, usize) {
    let mut tree = TreeView::new(font()).with_row_height(RH);
    let root = tree.add_root("Root", NodeIcon::Folder);
    tree.expand(root);
    let long = tree.add_child(root, LONG, NodeIcon::File);
    let short = tree.add_child(root, "a", NodeIcon::File);
    tree.set_node_secondary_text(long, Some("8.9 MB".into()));
    tree.set_node_fraction(long, Some(0.5));
    (tree, long, short)
}

fn lay_out(tree: &mut TreeView, w: f64) {
    tree.layout(Size::new(w, H));
    tree.set_bounds(Rect::new(0.0, 0.0, w, H));
}

fn hover_row(tree: &mut TreeView, i: usize) {
    let pos = Point::new(30.0, H - (i as f64 + 0.5) * RH);
    tree.on_event(&Event::MouseMove { pos });
}

#[test]
fn hovered_row_shows_its_node_tooltip_keyed_by_node() {
    let (mut tree, long, short) = tree();
    tree.set_node_tooltip(short, Some("12 KB, modified today".into()));
    tree.set_node_tooltip(long, Some("big file".into()));
    lay_out(&mut tree, 600.0);

    hover_row(&mut tree, 2);
    assert_eq!(tree.tooltip_text(), Some("12 KB, modified today"));
    assert_eq!(tree.tooltip_key(), Some(short as u64));
    hover_row(&mut tree, 1);
    assert_eq!(tree.tooltip_text(), Some("big file"));
    assert_eq!(tree.tooltip_key(), Some(long as u64));
    // The root has no tooltip and its name fits.
    hover_row(&mut tree, 0);
    assert_eq!(tree.tooltip_text(), None);
}

#[test]
fn elided_row_tips_full_name_then_node_tooltip() {
    let (mut tree, long, _) = tree();
    tree.set_node_tooltip(long, Some("big file".into()));
    lay_out(&mut tree, 240.0);
    hover_row(&mut tree, 1);
    let want = format!("{LONG}\nbig file");
    assert_eq!(tree.tooltip_text(), Some(want.as_str()));
    assert_eq!(tree.tooltip_key(), Some(long as u64));
}

#[test]
fn setting_or_clearing_a_tooltip_updates_the_built_row() {
    let (mut tree, _, short) = tree();
    lay_out(&mut tree, 600.0);
    hover_row(&mut tree, 2);
    assert_eq!(tree.tooltip_text(), None);

    tree.set_node_tooltip(short, Some("now with a tip".into()));
    lay_out(&mut tree, 600.0);
    assert_eq!(tree.tooltip_text(), Some("now with a tip"));
    assert_eq!(tree.nodes[short].tooltip.as_deref(), Some("now with a tip"));

    tree.set_node_tooltip(short, None);
    lay_out(&mut tree, 600.0);
    assert_eq!(tree.tooltip_text(), None);
}

#[test]
fn node_tooltip_wins_over_the_tree_wide_one() {
    let (tree, _, short) = tree();
    let mut tree = tree.with_tooltip("files");
    tree.set_node_tooltip(short, Some("mine".into()));
    lay_out(&mut tree, 600.0);
    hover_row(&mut tree, 2);
    assert_eq!(tree.tooltip_text(), Some("mine"));
    hover_row(&mut tree, 0);
    assert_eq!(tree.tooltip_text(), Some("files"));
}
