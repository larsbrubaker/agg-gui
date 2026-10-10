//! `TreeView` row names that don't fit beside the trailing secondary text
//! and fraction bar: the name label ellipsizes (end by default, or the mode
//! set with `TreeView::with_name_ellipsis`) instead of being clipped
//! mid-glyph, and the tree tips the full name while the pointer is over an
//! elided row — keyed by node so moving to another row re-arms the tooltip.

use std::sync::Arc;

use crate::event::Event;
use crate::geometry::{Point, Rect, Size};
use crate::text::{EllipsisMode, Font, ELLIPSIS};
use crate::widget::Widget;
use crate::widgets::tree_view::{NodeIcon, TreeView};
use crate::widgets::Label;

/// What [`EllipsisMode::End`] appends (agg-sharp's `EllipsisIfClipped`).
const END_DOTS: &str = "...";
const RH: f64 = 20.0;
const H: f64 = 200.0;
const LONG: &str = "demo_wasm_bg.wasm";
/// Wide enough for part of `LONG` beside "8.9 MB" and the fraction bar.
const NARROW: f64 = 240.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(super::TEST_FONT).expect("font"))
}

/// Root with a long-named child carrying "8.9 MB" and a fraction bar, and a
/// short-named child; laid out `w` wide.
fn tree(w: f64, mode: Option<EllipsisMode>) -> TreeView {
    let mut tree = TreeView::new(font()).with_row_height(RH);
    if let Some(m) = mode {
        tree = tree.with_name_ellipsis(m);
    }
    let root = tree.add_root("Root", NodeIcon::Folder);
    tree.expand(root);
    let long = tree.add_child(root, LONG, NodeIcon::File);
    tree.add_child(root, "a", NodeIcon::File);
    tree.set_node_secondary_text(long, Some("8.9 MB".into()));
    tree.set_node_fraction(long, Some(0.5));
    tree.layout(Size::new(w, H));
    tree.set_bounds(Rect::new(0.0, 0.0, w, H));
    tree
}

/// The name label of display row `i`.
fn name_label(tree: &TreeView, i: usize) -> &Label {
    tree.children()[i].children()[3]
        .as_any()
        .and_then(|a| a.downcast_ref::<Label>())
        .expect("name label")
}

fn hover_row(tree: &mut TreeView, i: usize) {
    let pos = Point::new(30.0, H - (i as f64 + 0.5) * RH);
    tree.on_event(&Event::MouseMove { pos });
}

#[test]
fn crowded_row_name_ends_with_an_ellipsis() {
    let tree = tree(NARROW, None);
    let label = name_label(&tree, 1);
    assert!(label.ellipsis_active(), "the long name must be elided");
    let shown = label.shown_text();
    assert!(
        shown.ends_with(END_DOTS),
        "end ellipsis by default: {shown}"
    );
    assert!(shown.starts_with("demo_"), "{shown}");
    assert!(!name_label(&tree, 2).ellipsis_active());
}

#[test]
fn name_ellipsis_mode_is_configurable() {
    let tree = tree(NARROW, Some(EllipsisMode::Middle));
    let shown = name_label(&tree, 1).shown_text();
    assert!(shown.contains(ELLIPSIS), "{shown}");
    assert!(shown.starts_with("demo"), "{shown}");
    assert!(
        shown.ends_with("wasm"),
        "middle mode keeps the end: {shown}"
    );
}

#[test]
fn elided_row_tips_its_full_name_keyed_by_node() {
    let mut tree = tree(NARROW, None);
    assert_eq!(tree.tooltip_text(), None, "no tip before hovering");
    hover_row(&mut tree, 1);
    assert_eq!(tree.tooltip_text(), Some(LONG));
    assert_eq!(tree.tooltip_key(), Some(1));
    // A row whose name fits shows no tip.
    hover_row(&mut tree, 2);
    assert_eq!(tree.tooltip_text(), None);
}

#[test]
fn wide_tree_neither_elides_nor_tips() {
    let mut tree = tree(600.0, None);
    assert!(!name_label(&tree, 1).ellipsis_active());
    assert_eq!(name_label(&tree, 1).shown_text(), LONG);
    hover_row(&mut tree, 1);
    assert_eq!(tree.tooltip_text(), None);
}

#[test]
fn tree_wide_tooltip_still_shows_off_elided_rows() {
    let mut tree = tree(NARROW, None).with_tooltip("files");
    hover_row(&mut tree, 2);
    assert_eq!(tree.tooltip_text(), Some("files"));
    hover_row(&mut tree, 1);
    assert_eq!(tree.tooltip_text(), Some(LONG), "the elided name wins");
}
