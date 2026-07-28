//! Unit tests for [`crate::debug_draw_report`] — the live-capture diagnostic
//! for the intermittent "continuous rendering never quiesces" runaway.
//!
//! Builds a tiny widget tree with one hot leaf and asserts the report names it
//! (with its child-index path), and that drained draw-trace provenance tags
//! surface with per-tag counts.

use crate::animation;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::widget::Widget;

/// Minimal test widget: a named node with children and an optional own
/// draw-need, mirroring the standard container `needs_draw` contract
/// (visibility gate + own state OR any child).
struct Node {
    name: &'static str,
    hot: bool,
    visible: bool,
    children: Vec<Box<dyn Widget>>,
    bounds: Rect,
}

impl Node {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            hot: false,
            visible: true,
            children: Vec::new(),
            bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
        }
    }
    fn hot(mut self) -> Self {
        self.hot = true;
        self
    }
    fn hidden(mut self) -> Self {
        self.visible = false;
        self
    }
    fn child(mut self, c: Node) -> Self {
        self.children.push(Box::new(c));
        self
    }
}

impl Widget for Node {
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
    fn layout(&mut self, available: Size) -> Size {
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn is_visible(&self) -> bool {
        self.visible
    }
    fn needs_draw(&self) -> bool {
        if !self.visible {
            return false;
        }
        self.hot || self.children.iter().any(|c| c.needs_draw())
    }
}

/// One hot leaf deep in the tree must be named in the report with its exact
/// child-index path and a `<- self` marker; its ancestors appear too (they
/// propagate the child's need) but WITHOUT the self marker.
#[test]
fn report_names_the_hot_widget() {
    animation::clear_draw_request();
    let _ = animation::drain_draw_trace();

    // root -> [0] Cold, [1] Middle -> [0] HotLeaf(hot)
    let root = Node::new("Root")
        .child(Node::new("Cold"))
        .child(Node::new("Middle").child(Node::new("HotLeaf").hot()));

    let report = crate::debug_draw_report(&root);

    assert!(
        report.contains("HotLeaf"),
        "report must name the hot widget:\n{report}"
    );
    // Path root -> child 1 -> child 0.
    assert!(
        report.contains("[1/0] HotLeaf  <- self"),
        "report must show the hot leaf's child-index path + self marker:\n{report}"
    );
    // The intervening container propagates the need (self-hot marker absent).
    assert!(
        report.contains("[1] Middle\n"),
        "propagating ancestor must appear without the self marker:\n{report}"
    );
    // The cold sibling must NOT appear.
    assert!(
        !report.contains("Cold"),
        "a widget not wanting a draw must be absent:\n{report}"
    );
}

/// A hidden subtree — even with a hot descendant — must be excluded, matching
/// the host loop's visibility gating.
#[test]
fn report_skips_hidden_subtrees() {
    animation::clear_draw_request();
    let _ = animation::drain_draw_trace();

    let root = Node::new("Root").child(
        Node::new("HiddenBranch")
            .hidden()
            .child(Node::new("HiddenHot").hot()),
    );

    let report = crate::debug_draw_report(&root);
    assert!(
        !report.contains("HiddenHot"),
        "a hot widget inside a hidden subtree must be excluded:\n{report}"
    );
    assert!(
        report.contains("widgets wanting draw: none"),
        "nothing visible wants a draw:\n{report}"
    );
}

/// Drained draw-trace provenance tags must surface in the report,
/// deduplicated with per-tag counts. Debug-build only (the trace compiles out
/// in release), so the assertion is gated on `debug_assertions`.
#[test]
#[cfg(debug_assertions)]
fn report_lists_trace_tags_with_counts() {
    animation::clear_draw_request();
    let _ = animation::drain_draw_trace();

    animation::request_draw_tagged("alpha");
    animation::request_draw_tagged("alpha");
    animation::request_draw_tagged("alpha");
    animation::request_draw_tagged("beta");

    let root = Node::new("Root");
    let report = crate::debug_draw_report(&root);

    assert!(
        report.contains("3 x alpha"),
        "the most frequent tag must show its count:\n{report}"
    );
    assert!(
        report.contains("1 x beta"),
        "each distinct tag must be listed with its count:\n{report}"
    );

    // Draining inside the report leaves the trace clean for the next capture.
    assert!(
        animation::drain_draw_trace().is_empty(),
        "debug_draw_report must drain the trace buffer"
    );
}
