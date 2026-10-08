//! Checks of the public widget-handle pieces GUI automation builds on:
//! [`WidgetAnchor`] (a path that follows its widget through reorders and
//! reports it gone once removed), [`walk_path`] and [`App::focused_path`].

use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::geometry::{Rect, Size};
use agg_gui::widget::walk_path;
use agg_gui::{App, Widget, WidgetAnchor};

/// A plain container or leaf with a fixed name.
struct Node {
    name: &'static str,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    focusable: bool,
}

fn node(name: &'static str, children: Vec<Box<dyn Widget>>) -> Box<dyn Widget> {
    Box::new(Node {
        name,
        bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
        children,
        focusable: false,
    })
}

impl Widget for Node {
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
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn is_focusable(&self) -> bool {
        self.focusable
    }
    fn id(&self) -> Option<&str> {
        Some(self.name)
    }
}

fn name_at(root: &dyn Widget, path: &[usize]) -> Option<String> {
    walk_path(root, path).and_then(|w| w.id().map(str::to_string))
}

#[test]
fn rust_only_an_anchor_follows_its_widget_through_a_reorder_and_reports_removal() {
    let mut root = node(
        "root",
        vec![
            node("a", vec![]),
            node("b", vec![node("b0", vec![]), node("b1", vec![])]),
        ],
    );
    assert_eq!(name_at(root.as_ref(), &[1, 1]).as_deref(), Some("b1"));
    assert!(WidgetAnchor::new(root.as_ref(), &[1, 2]).is_none());
    assert_eq!(
        walk_path(root.as_ref(), &[]).and_then(|w| w.id()),
        Some("root")
    );

    let mut anchor = WidgetAnchor::new(root.as_ref(), &[1, 1]).expect("b1");
    // Reorder at both levels: `b` moves first, `b1` moves first within it.
    root.children_mut().swap(0, 1);
    root.children_mut()[0].children_mut().swap(0, 1);
    assert_eq!(anchor.resolve(root.as_ref()), Some(vec![0, 0]));
    assert_eq!(anchor.refresh(root.as_ref()), Some(&[0usize, 0][..]));
    assert_eq!(anchor.path(), &[0, 0]);

    // Removing the widget detaches the anchor.
    let removed = root.children_mut()[0].children_mut().remove(0);
    assert_eq!(anchor.resolve(root.as_ref()), None);
    drop(removed);
}

#[test]
fn rust_only_focused_path_names_the_focused_widget() {
    let leaf = Box::new(Node {
        name: "field",
        bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
        children: Vec::new(),
        focusable: true,
    });
    let mut app = App::new(node(
        "root",
        vec![node("pad", vec![]), node("box", vec![leaf])],
    ));
    assert_eq!(app.focused_path(), None);
    app.focus_first();
    assert_eq!(app.focused_path(), Some(&[1usize, 0][..]));
    assert_eq!(
        name_at(app.root(), app.focused_path().unwrap_or_default()).as_deref(),
        Some("field")
    );
}
