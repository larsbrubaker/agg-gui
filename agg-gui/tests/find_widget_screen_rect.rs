//! `find_widget_screen_rect` reports *absolute* window placement.
//!
//! `Widget::bounds` is parent-local, and plenty of leaf widgets reset
//! their own origin to (0, 0) inside `layout` (the parent positions them
//! by translating during paint / dispatch). A host that needs to line
//! window pixels up with a widget — screenshot and thumbnail crops,
//! native overlays — must therefore not read `bounds()` directly; this
//! helper walks the ancestor chain the way the inspector does.

use agg_gui::{DrawCtx, Event, EventResult, Rect, Size, TransAffine, Widget};

/// Leaf that behaves like `Viewport3dWidget`: `layout` pins its own
/// bounds origin to (0, 0), so its local rect says nothing about where
/// on the window it actually sits.
struct SelfZeroingPane {
    id: &'static str,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    visible: bool,
}

impl Widget for SelfZeroingPane {
    fn type_name(&self) -> &'static str {
        "SelfZeroingPane"
    }
    fn id(&self) -> Option<&str> {
        Some(self.id)
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
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn is_visible(&self) -> bool {
        self.visible
    }
}

/// Container that positions its children itself, optionally applying a
/// pan/zoom-style transform between itself and them.
struct Frame {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    child_scale: f64,
}

impl Widget for Frame {
    fn type_name(&self) -> &'static str {
        "Frame"
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
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _e: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn inspector_child_transform(&self) -> TransAffine {
        let mut t = TransAffine::new();
        t.scale_uniform(self.child_scale);
        t
    }
}

fn pane(id: &'static str, bounds: Rect) -> Box<dyn Widget> {
    Box::new(SelfZeroingPane {
        id,
        bounds,
        children: vec![],
        visible: true,
    })
}

#[test]
fn screen_rect_accumulates_ancestor_offsets() {
    // 800x600 window split into two 300-tall panes; the upper one sits
    // at y = 300 in the root's coordinate space.
    let mut root = Frame {
        bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
        children: vec![
            pane("lower", Rect::new(0.0, 0.0, 800.0, 300.0)),
            pane("upper", Rect::new(0.0, 300.0, 800.0, 300.0)),
        ],
        child_scale: 1.0,
    };
    // The pane self-zeroes during layout and the parent re-places it —
    // so `bounds()` alone reports an origin of (0, 0), which is exactly
    // what a caller must not treat as a window position.
    let self_zeroed = {
        let upper = &mut root.children_mut()[1];
        upper.layout(Size::new(800.0, 300.0));
        upper.bounds()
    };
    assert_eq!(self_zeroed, Rect::new(0.0, 0.0, 800.0, 300.0));
    root.children_mut()[1].set_bounds(Rect::new(0.0, 300.0, 800.0, 300.0));

    assert_eq!(
        agg_gui::find_widget_screen_rect(&root, "upper"),
        Some(Rect::new(0.0, 300.0, 800.0, 300.0))
    );
    assert_eq!(
        agg_gui::find_widget_screen_rect(&root, "lower"),
        Some(Rect::new(0.0, 0.0, 800.0, 300.0))
    );
    assert_eq!(agg_gui::find_widget_screen_rect(&root, "absent"), None);
}

#[test]
fn screen_rect_applies_inspector_child_transform() {
    // Same composition `collect_inspector_nodes` uses: the child's rect
    // passes through the parent's child transform, then the parent's own
    // origin.
    let inner = Frame {
        bounds: Rect::new(100.0, 50.0, 400.0, 300.0),
        children: vec![pane("leaf", Rect::new(5.0, 5.0, 8.0, 4.0))],
        child_scale: 2.0,
    };
    assert_eq!(
        agg_gui::find_widget_screen_rect(&inner, "leaf"),
        Some(Rect::new(110.0, 60.0, 16.0, 8.0))
    );
}

#[test]
fn screen_rect_is_none_for_hidden_widgets() {
    // A hidden widget covers no pixels, so a screenshot crop must not be
    // derived from it — and neither must one for a visible descendant of
    // a hidden ancestor, which is equally off-screen.
    let root = Frame {
        bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
        children: vec![
            Box::new(SelfZeroingPane {
                id: "hidden",
                bounds: Rect::new(0.0, 0.0, 800.0, 300.0),
                children: vec![pane("under-hidden", Rect::new(0.0, 0.0, 100.0, 100.0))],
                visible: false,
            }),
            pane("shown", Rect::new(0.0, 300.0, 800.0, 300.0)),
        ],
        child_scale: 1.0,
    };

    assert_eq!(agg_gui::find_widget_screen_rect(&root, "hidden"), None);
    assert_eq!(
        agg_gui::find_widget_screen_rect(&root, "under-hidden"),
        None
    );
    // The visible sibling is unaffected — the skip is per-subtree, not a
    // short-circuit of the whole walk.
    assert_eq!(
        agg_gui::find_widget_screen_rect(&root, "shown"),
        Some(Rect::new(0.0, 300.0, 800.0, 300.0))
    );
}

#[test]
fn screen_rect_matches_the_inspector_snapshot() {
    // The two queries must never disagree: the inspector overlay and a
    // screenshot crop have to frame the same pixels.
    let mut root = Frame {
        bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
        children: vec![Box::new(Frame {
            bounds: Rect::new(40.0, 25.0, 700.0, 500.0),
            children: vec![pane("target", Rect::new(0.0, 0.0, 100.0, 80.0))],
            child_scale: 1.0,
        })],
        child_scale: 1.0,
    };
    root.children_mut()[0].children_mut()[0].set_bounds(Rect::new(10.0, 200.0, 100.0, 80.0));

    let mut nodes = Vec::new();
    agg_gui::collect_inspector_nodes(&root, 0, agg_gui::Point::ORIGIN, &mut nodes);
    let from_inspector = nodes
        .iter()
        .find(|n| n.type_name == "SelfZeroingPane")
        .expect("pane in snapshot")
        .screen_bounds;
    assert_eq!(
        agg_gui::find_widget_screen_rect(&root, "target"),
        Some(from_inspector)
    );
}
