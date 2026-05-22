//! Hover behaviour of `InspectorPanel`.
//!
//! Two contracts the panel must honour for hover to be useful:
//!
//! 1. A MouseMove over a tree row must publish the corresponding widget's
//!    screen_bounds into the shared `hovered_bounds` cell AND advance the
//!    invalidation epoch.  The epoch advance is what lets `dispatch_event`
//!    mark retained ancestors (the floating Window the panel lives in)
//!    dirty so the row's own hover highlight re-rasterises.
//!
//! 2. `paint_global_overlay` must draw the Chrome-F12-style highlight on
//!    top of the rest of the UI when `hovered_bounds` is set.  Without
//!    this the host has to wire its own overlay painter, defeating the
//!    "drop-in inspector" contract.

use super::*;

use crate::event::Event;
use crate::geometry::{Point, Rect};
use crate::layout_props::Insets;
use crate::text::Font;
use crate::widget::InspectorNode;
use crate::widgets::inspector::InspectorPanel;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

fn make_nodes() -> Rc<RefCell<Vec<InspectorNode>>> {
    Rc::new(RefCell::new(vec![
        InspectorNode {
            type_name: "Root",
            screen_bounds: Rect::new(40.0, 30.0, 120.0, 80.0),
            depth: 0,
            margin: Insets::ZERO,
            padding: Insets::ZERO,
            h_anchor: crate::layout_props::HAnchor::FIT,
            v_anchor: crate::layout_props::VAnchor::FIT,
            path: vec![],
            properties: vec![],
        },
        InspectorNode {
            type_name: "Child",
            screen_bounds: Rect::new(50.0, 35.0, 60.0, 20.0),
            depth: 1,
            margin: Insets::ZERO,
            padding: Insets::ZERO,
            h_anchor: crate::layout_props::HAnchor::FIT,
            v_anchor: crate::layout_props::VAnchor::FIT,
            path: vec![],
            properties: vec![],
        },
    ]))
}

/// MouseMove over the top tree row publishes that node's screen_bounds
/// into the shared `hovered_bounds` cell — the same cell hosts read to
/// drive any custom overlay logic.
#[test]
fn hover_over_top_row_populates_hovered_bounds() {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered = Rc::new(RefCell::new(None));
    let nodes = make_nodes();
    let mut panel = InspectorPanel::new(Arc::clone(&font), Rc::clone(&nodes), Rc::clone(&hovered));
    panel.set_bounds(Rect::new(0.0, 0.0, 240.0, 400.0));
    panel.layout(Size::new(240.0, 400.0));

    // Row 0 sits at the top of the tree area.  With DEFAULT_PROPS_H=180 and
    // HEADER_H=30 in a 400-tall panel the tree area runs y ∈ [184, 370];
    // row height is 20, so row 0 occupies y ∈ [350, 370].  pos.y = 360 is
    // safely on row 0.
    let _ = panel.on_event(&Event::MouseMove {
        pos: Point::new(80.0, 360.0),
    });

    let observed = *hovered.borrow();
    let expected = nodes.borrow()[0].screen_bounds;
    assert_eq!(
        observed.map(|o| o.bounds),
        Some(expected),
        "MouseMove over row 0 must publish that node's screen_bounds into the hovered_bounds cell"
    );
}

/// A hover-row change must advance the invalidation epoch — without that
/// bump, `dispatch_event` does not mark the inspector's parent Window
/// dirty and the row's hover background (which the Window backbuffer
/// caches) never appears.
#[test]
fn hover_row_change_advances_invalidation_epoch() {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered = Rc::new(RefCell::new(None));
    let nodes = make_nodes();
    let mut panel = InspectorPanel::new(Arc::clone(&font), Rc::clone(&nodes), Rc::clone(&hovered));
    panel.set_bounds(Rect::new(0.0, 0.0, 240.0, 400.0));
    panel.layout(Size::new(240.0, 400.0));

    // Seed the panel's hovered state to "no row" so the next move is a
    // real change.
    let _ = panel.on_event(&Event::MouseMove {
        pos: Point::new(80.0, 10.0), // below tree area
    });

    crate::animation::clear_draw_request();
    let before = crate::animation::invalidation_epoch();
    let _ = panel.on_event(&Event::MouseMove {
        pos: Point::new(80.0, 360.0), // onto row 0
    });
    assert_ne!(
        crate::animation::invalidation_epoch(),
        before,
        "Hovering onto a new tree row must advance the invalidation epoch so the inspector's \
         parent Window backbuffer invalidates and the row's hover background re-rasterises"
    );
}

/// When `hovered_bounds` is set, `paint_global_overlay` must emit at
/// least some pixels at the corresponding screen-space rect.  The
/// invariant: no overlay → no pixels; overlay set → pixels.
#[test]
fn paint_global_overlay_draws_when_hovered_bounds_set() {
    use crate::widget::paint_global_overlays;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered = Rc::new(RefCell::new(None));
    let nodes = make_nodes();
    let mut panel = InspectorPanel::new(Arc::clone(&font), Rc::clone(&nodes), Rc::clone(&hovered));
    panel.set_bounds(Rect::new(0.0, 0.0, 240.0, 400.0));
    panel.layout(Size::new(240.0, 400.0));

    // First paint with no hover.  Hovered bounds will later be placed
    // OUTSIDE the panel's own rect so any pixel change at the sample
    // point is unambiguously from the overlay.
    let pw = 400u32;
    let ph = 500u32;
    let mut fb_empty = Framebuffer::new(pw, ph);
    {
        let mut ctx = GfxCtx::new(&mut fb_empty);
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        paint_global_overlays(&mut panel as &mut dyn Widget, &mut ctx);
    }

    *hovered.borrow_mut() = Some(crate::widget::InspectorOverlay {
        bounds: Rect::new(250.0, 250.0, 80.0, 60.0),
        margin: Insets::ZERO,
        padding: Insets::ZERO,
    });

    let mut fb_overlay = Framebuffer::new(pw, ph);
    {
        let mut ctx = GfxCtx::new(&mut fb_overlay);
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        paint_global_overlays(&mut panel as &mut dyn Widget, &mut ctx);
    }

    // Framebuffer is Y-up (row 0 = bottom), so logical y maps to row y
    // directly.  Sample the centre of the overlay rect at logical
    // (290, 280).
    let sample_x = 290u32;
    let sample_y = 280u32;
    let idx = ((sample_y * pw + sample_x) * 4) as usize;
    let empty_px = &fb_empty.pixels()[idx..idx + 4];
    let overlay_px = &fb_overlay.pixels()[idx..idx + 4];

    assert_eq!(
        empty_px,
        &[255, 255, 255, 255],
        "With no hover the overlay region must stay the clear colour (white); got {:?}",
        empty_px
    );
    assert_ne!(
        overlay_px, empty_px,
        "With hovered_bounds set, paint_global_overlay must change pixels inside the overlay \
         rect.  Sample at logical ({sample_x},{sample_y}) — got {:?} vs empty {:?}",
        overlay_px, empty_px
    );
}

/// MouseMove over a row also marks that row as the tree's hovered node so
/// `TreeView::paint` knows where to draw the hover background.
#[test]
fn hover_row_marks_tree_row_hovered() {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered = Rc::new(RefCell::new(None));
    let nodes = make_nodes();
    let mut panel = InspectorPanel::new(Arc::clone(&font), Rc::clone(&nodes), Rc::clone(&hovered));
    panel.set_bounds(Rect::new(0.0, 0.0, 240.0, 400.0));
    panel.layout(Size::new(240.0, 400.0));

    let _ = panel.on_event(&Event::MouseMove {
        pos: Point::new(80.0, 360.0), // row 0
    });
    // Re-layout so any deferred row state propagates into the row widgets
    // — production runs layout every frame between event delivery and
    // paint.
    panel.layout(Size::new(240.0, 400.0));
    assert_eq!(
        panel.tree_view.hovered_node_idx(),
        Some(0),
        "After hovering row 0, TreeView::hovered_node_idx() must report 0"
    );
}

/// A widget that exposes a non-identity `inspector_child_transform`
/// must have its descendants' `screen_bounds` reflect that transform
/// in the inspector snapshot.  Without this the F12-style hover
/// highlight lands at the un-transformed canvas position when the
/// widget sits inside a panning/zooming container (e.g. NodeEditor).
#[test]
fn collect_inspector_nodes_applies_child_transform() {
    use crate::draw_ctx::DrawCtx;
    use crate::event::EventResult;
    use crate::geometry::Size;
    use crate::layout_props::WidgetBase;
    use crate::widget::{collect_inspector_nodes, Widget};
    use crate::TransAffine;

    /// Parent widget whose children paint inside a scaled+translated
    /// space — mimics NodeEditor's pan/zoom behaviour minimally.
    struct ScalingParent {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        scale: f64,
        offset: [f64; 2],
        base: WidgetBase,
    }
    impl Widget for ScalingParent {
        fn type_name(&self) -> &'static str {
            "ScalingParent"
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
        fn layout(&mut self, available: Size) -> Size {
            available
        }
        fn paint(&mut self, _: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }
        fn inspector_child_transform(&self) -> TransAffine {
            let mut t = TransAffine::new();
            t.scale_uniform(self.scale);
            t.translate(self.offset[0], self.offset[1]);
            t
        }
    }
    struct Leaf {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        base: WidgetBase,
    }
    impl Widget for Leaf {
        fn type_name(&self) -> &'static str {
            "Leaf"
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
        fn layout(&mut self, _: Size) -> Size {
            Size::new(self.bounds.width, self.bounds.height)
        }
        fn paint(&mut self, _: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }
    }

    // ScalingParent at screen (100, 50), with child_transform scale=2,
    // offset=(10, 20).  Leaf at parent-local (5, 5) with size 8x4.
    // Expected leaf screen bounds:
    //   x = 100 + (10 + 5*2) = 120
    //   y = 50 + (20 + 5*2) = 80
    //   w = 8 * 2 = 16
    //   h = 4 * 2 = 8
    let leaf = Leaf {
        bounds: Rect::new(5.0, 5.0, 8.0, 4.0),
        children: vec![],
        base: WidgetBase::new(),
    };
    let parent = ScalingParent {
        bounds: Rect::new(100.0, 50.0, 400.0, 300.0),
        children: vec![Box::new(leaf)],
        scale: 2.0,
        offset: [10.0, 20.0],
        base: WidgetBase::new(),
    };

    let mut nodes = Vec::new();
    collect_inspector_nodes(&parent, 0, Point::ORIGIN, &mut nodes);
    assert_eq!(nodes.len(), 2, "expected ScalingParent + Leaf");
    let leaf_node = nodes
        .iter()
        .find(|n| n.type_name == "Leaf")
        .expect("Leaf inspector node missing");
    let b = leaf_node.screen_bounds;
    assert!(
        (b.x - 120.0).abs() < 1e-6
            && (b.y - 80.0).abs() < 1e-6
            && (b.width - 16.0).abs() < 1e-6
            && (b.height - 8.0).abs() < 1e-6,
        "Leaf screen_bounds must reflect ScalingParent's inspector_child_transform; got {:?}",
        b
    );
}

/// Moving the cursor off the tree area clears the tree's hover state too
/// — otherwise the previously-hovered row keeps its background tinted
/// inside the parent Window's cached backbuffer.
#[test]
fn moving_off_tree_clears_tree_hover() {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered = Rc::new(RefCell::new(None));
    let nodes = make_nodes();
    let mut panel = InspectorPanel::new(Arc::clone(&font), Rc::clone(&nodes), Rc::clone(&hovered));
    panel.set_bounds(Rect::new(0.0, 0.0, 240.0, 400.0));
    panel.layout(Size::new(240.0, 400.0));

    let _ = panel.on_event(&Event::MouseMove {
        pos: Point::new(80.0, 360.0), // row 0
    });
    panel.layout(Size::new(240.0, 400.0));
    assert_eq!(panel.tree_view.hovered_node_idx(), Some(0));

    let _ = panel.on_event(&Event::MouseMove {
        pos: Point::new(80.0, 10.0), // below tree area (props pane)
    });
    panel.layout(Size::new(240.0, 400.0));
    assert_eq!(
        panel.tree_view.hovered_node_idx(),
        None,
        "Mouse moving off the tree area must clear the tree's hover state"
    );
    assert!(
        hovered.borrow().is_none(),
        "...and the shared hovered_bounds cell must be cleared too"
    );
}
