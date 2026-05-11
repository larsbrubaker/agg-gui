use super::*;

/// InspectorPanel must build the TreeView with the correct nodes:
/// - Two InspectorNodes (Root at depth 0, Child at depth 1) must produce two
///   TreeView nodes where Child's parent is Root's index.
/// - InspectorPanel exposes no children (TreeView is managed directly).
/// - The TreeView bounds must sit inside the tree area (above split, below header).
#[test]
fn test_inspector_row0_at_top() {
    use crate::geometry::Rect;
    use crate::text::Font;
    use crate::widget::{InspectorNode, Widget};
    use crate::widgets::inspector::InspectorPanel;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered_bounds = Rc::new(RefCell::new(None));
    let nodes: Rc<RefCell<Vec<InspectorNode>>> = Rc::new(RefCell::new(vec![
        InspectorNode {
            type_name: "Root",
            screen_bounds: Rect::new(0.0, 0.0, 100.0, 50.0),
            depth: 0,
            margin: crate::layout_props::Insets::ZERO,
            padding: crate::layout_props::Insets::ZERO,
            h_anchor: crate::layout_props::HAnchor::FIT,
            v_anchor: crate::layout_props::VAnchor::FIT,
            path: vec![],
            properties: vec![],
        },
        InspectorNode {
            type_name: "Child",
            screen_bounds: Rect::new(0.0, 0.0, 50.0, 20.0),
            depth: 1,
            margin: crate::layout_props::Insets::ZERO,
            padding: crate::layout_props::Insets::ZERO,
            h_anchor: crate::layout_props::HAnchor::FIT,
            v_anchor: crate::layout_props::VAnchor::FIT,
            path: vec![],
            properties: vec![],
        },
    ]));

    let mut panel = InspectorPanel::new(
        Arc::clone(&font),
        Rc::clone(&nodes),
        Rc::clone(&hovered_bounds),
    );
    panel.layout(crate::Size::new(200.0, 300.0));
    panel.set_bounds(Rect::new(0.0, 0.0, 200.0, 300.0));

    // InspectorPanel exposes one InternalPresenceNode child so it appears
    // expandable in the inspector (not a leaf node).
    assert_eq!(
        panel.children().len(),
        1,
        "InspectorPanel must have one presence child"
    );
    assert_eq!(
        panel.children()[0].type_name(),
        "TreeView",
        "The presence child must report type_name 'TreeView'"
    );

    // The TreeView should have exactly 2 nodes (one per InspectorNode).
    assert_eq!(
        panel.tree_view.nodes.len(),
        2,
        "tree_view must have 2 nodes"
    );

    // Root node has no parent; Child node's parent is Root (index 0).
    assert!(
        panel.tree_view.nodes[0].parent.is_none(),
        "Root must have no parent"
    );
    assert_eq!(
        panel.tree_view.nodes[1].parent,
        Some(0),
        "Child must have Root (0) as parent"
    );

    // The TreeView bounds must be positioned inside the tree area.
    // tree_area top = list_area_h = 300 - 30 = 270 (just below header).
    // tree_area bottom = split_y + 4; split_y ≥ MIN_PROPS_H = 60, so ≥ 64.
    let tv_bounds = panel.tree_view.bounds();
    assert!(tv_bounds.height > 0.0, "TreeView must have positive height");
    assert!(
        tv_bounds.y >= 60.0,
        "TreeView bottom must be above split handle"
    );
    assert!(
        tv_bounds.y + tv_bounds.height <= 270.0 + 1.0,
        "TreeView top must not exceed list_area_h (270); got {}",
        tv_bounds.y + tv_bounds.height
    );
}

/// InspectorPanel must populate tree_view.nodes from the InspectorNode list,
/// building a correct parent-child structure from the depth information.
#[test]
fn test_inspector_tree_populates_from_nodes() {
    use crate::geometry::Rect;
    use crate::text::Font;
    use crate::widget::{InspectorNode, Widget};
    use crate::widgets::inspector::InspectorPanel;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered_bounds = Rc::new(RefCell::new(None));
    let nodes: Rc<RefCell<Vec<InspectorNode>>> = Rc::new(RefCell::new(vec![
        InspectorNode {
            type_name: "Root",
            screen_bounds: Rect::new(0.0, 0.0, 100.0, 50.0),
            depth: 0,
            margin: crate::layout_props::Insets::ZERO,
            padding: crate::layout_props::Insets::ZERO,
            h_anchor: crate::layout_props::HAnchor::FIT,
            v_anchor: crate::layout_props::VAnchor::FIT,
            path: vec![],
            properties: vec![],
        },
        InspectorNode {
            type_name: "Child",
            screen_bounds: Rect::new(0.0, 0.0, 50.0, 20.0),
            depth: 1,
            margin: crate::layout_props::Insets::ZERO,
            padding: crate::layout_props::Insets::ZERO,
            h_anchor: crate::layout_props::HAnchor::FIT,
            v_anchor: crate::layout_props::VAnchor::FIT,
            path: vec![],
            properties: vec![],
        },
        InspectorNode {
            type_name: "Sibling",
            screen_bounds: Rect::new(0.0, 0.0, 50.0, 20.0),
            depth: 0,
            margin: crate::layout_props::Insets::ZERO,
            padding: crate::layout_props::Insets::ZERO,
            h_anchor: crate::layout_props::HAnchor::FIT,
            v_anchor: crate::layout_props::VAnchor::FIT,
            path: vec![],
            properties: vec![],
        },
    ]));

    let mut panel = InspectorPanel::new(Arc::clone(&font), Rc::clone(&nodes), hovered_bounds);
    panel.layout(crate::Size::new(200.0, 400.0));

    // Panel exposes TreeView via tree_view field.
    assert_eq!(panel.tree_view.nodes.len(), 3, "must have 3 tree nodes");

    // Root is a root-level node (no parent).
    assert!(
        panel.tree_view.nodes[0].parent.is_none(),
        "node 0 must be root-level"
    );

    // Child has Root as parent.
    assert_eq!(
        panel.tree_view.nodes[1].parent,
        Some(0),
        "node 1 must be child of node 0"
    );

    // Sibling is another root-level node.
    assert!(
        panel.tree_view.nodes[2].parent.is_none(),
        "node 2 must be root-level"
    );

    // InspectorPanel.children() exposes one InternalPresenceNode so it is
    // non-leaf in the inspector tree; the proxy reports type_name "TreeView".
    assert_eq!(
        panel.children().len(),
        1,
        "InspectorPanel must have one presence child"
    );
    assert_eq!(
        panel.children()[0].type_name(),
        "TreeView",
        "Presence child must report type_name 'TreeView'"
    );
}

/// All nodes must be expanded by default so the full tree is visible on first show.
#[test]
fn test_inspector_tree_default_expanded() {
    use crate::geometry::Rect;
    use crate::text::Font;
    use crate::widget::{InspectorNode, Widget};
    use crate::widgets::inspector::InspectorPanel;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered_bounds = Rc::new(RefCell::new(None));
    let nodes: Rc<RefCell<Vec<InspectorNode>>> = Rc::new(RefCell::new(vec![
        InspectorNode {
            type_name: "Root",
            screen_bounds: Rect::new(0.0, 0.0, 100.0, 50.0),
            depth: 0,
            margin: crate::layout_props::Insets::ZERO,
            padding: crate::layout_props::Insets::ZERO,
            h_anchor: crate::layout_props::HAnchor::FIT,
            v_anchor: crate::layout_props::VAnchor::FIT,
            path: vec![],
            properties: vec![],
        },
        InspectorNode {
            type_name: "Child",
            screen_bounds: Rect::new(0.0, 0.0, 50.0, 20.0),
            depth: 1,
            margin: crate::layout_props::Insets::ZERO,
            padding: crate::layout_props::Insets::ZERO,
            h_anchor: crate::layout_props::HAnchor::FIT,
            v_anchor: crate::layout_props::VAnchor::FIT,
            path: vec![],
            properties: vec![],
        },
    ]));

    let mut panel = InspectorPanel::new(Arc::clone(&font), Rc::clone(&nodes), hovered_bounds);
    panel.layout(crate::Size::new(200.0, 400.0));

    for (i, node) in panel.tree_view.nodes.iter().enumerate() {
        assert!(node.is_expanded, "node {} must be expanded by default", i);
    }
}

/// Inspector's TreeView must have drag-and-drop disabled by default.
#[test]
fn test_inspector_tree_drag_disabled() {
    use crate::geometry::Rect;
    use crate::text::Font;
    use crate::widget::InspectorNode;
    use crate::widgets::inspector::InspectorPanel;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered_bounds = Rc::new(RefCell::new(None));
    let nodes: Rc<RefCell<Vec<InspectorNode>>> = Rc::new(RefCell::new(vec![InspectorNode {
        type_name: "Root",
        screen_bounds: Rect::new(0.0, 0.0, 100.0, 50.0),
        depth: 0,
        margin: crate::layout_props::Insets::ZERO,
        padding: crate::layout_props::Insets::ZERO,
        h_anchor: crate::layout_props::HAnchor::FIT,
        v_anchor: crate::layout_props::VAnchor::FIT,
        path: vec![],
        properties: vec![],
    }]));

    let panel = InspectorPanel::new(Arc::clone(&font), Rc::clone(&nodes), hovered_bounds);
    assert!(
        !panel.tree_view.drag_enabled,
        "inspector TreeView must have drag disabled"
    );
}

/// ExpandToggle paints a filled triangle when has_children=true, nothing when false.
#[test]
fn test_expand_toggle_paints_arrow_only_when_has_children() {
    use crate::widget::paint_subtree;
    use crate::widgets::tree_view::row::ExpandToggle;

    let mut fb_with = Framebuffer::new(20, 20);
    let mut fb_without = Framebuffer::new(20, 20);
    {
        let mut ctx = GfxCtx::new(&mut fb_with);
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        let mut toggle = ExpandToggle::new(true, false);
        toggle.layout(Size::new(20.0, 20.0));
        toggle.set_bounds(crate::Rect::new(0.0, 0.0, 20.0, 20.0));
        paint_subtree(&mut toggle, &mut ctx);
    }
    {
        let mut ctx = GfxCtx::new(&mut fb_without);
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        let mut toggle = ExpandToggle::new(false, false);
        toggle.layout(Size::new(20.0, 20.0));
        toggle.set_bounds(crate::Rect::new(0.0, 0.0, 20.0, 20.0));
        paint_subtree(&mut toggle, &mut ctx);
    }
    // toggle with has_children=true must differ from has_children=false
    assert_ne!(fb_with.pixels(), fb_without.pixels());
}

/// Typing into a TextField inserts characters at the cursor.
#[test]
fn test_text_field_typing() {
    use crate::text::Font;
    use crate::widgets::text_field::TextField as TF;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut field = TF::new(font).with_font_size(14.0);
    field.layout(Size::new(200.0, 36.0));
    field.set_bounds(crate::Rect::new(0.0, 0.0, 200.0, 36.0));

    // Give focus directly.
    field.on_event(&crate::Event::FocusGained);

    // Type "Hi"
    field.on_event(&crate::Event::KeyDown {
        key: Key::Char('H'),
        modifiers: Modifiers::default(),
    });
    field.on_event(&crate::Event::KeyDown {
        key: Key::Char('i'),
        modifiers: Modifiers::default(),
    });
    assert_eq!(field.text(), "Hi", "typed characters should appear in text");

    // Backspace removes the last character.
    field.on_event(&crate::Event::KeyDown {
        key: Key::Backspace,
        modifiers: Modifiers::default(),
    });
    assert_eq!(field.text(), "H", "backspace should remove last character");
}

/// After layout(), TreeView children() returns one TreeRow per visible node.
#[test]
fn test_treeview_children_count_equals_visible_rows() {
    use crate::geometry::Size;
    use crate::text::Font;
    use crate::widgets::tree_view::{NodeIcon, TreeView};
    use std::sync::Arc;
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut tv = TreeView::new(Arc::clone(&font));
    let root = tv.add_root("Root", NodeIcon::Folder);
    tv.add_child(root, "Child A", NodeIcon::File);
    tv.add_child(root, "Child B", NodeIcon::File);
    tv.nodes[root].is_expanded = true;
    tv.layout(Size::new(300.0, 200.0));
    // root + 2 children = 3 visible rows
    assert_eq!(
        tv.children().len(),
        3,
        "expected 3 children after expanding root with 2 children"
    );
}

/// Each TreeRow child has type_name "TreeRow".
#[test]
fn test_treeview_row_node_idx() {
    use crate::geometry::Size;
    use crate::text::Font;
    use crate::widgets::tree_view::{NodeIcon, TreeView};
    use std::sync::Arc;
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut tv = TreeView::new(Arc::clone(&font));
    tv.add_root("Only Root", NodeIcon::Package);
    tv.layout(Size::new(200.0, 100.0));
    assert_eq!(tv.children().len(), 1);
    assert_eq!(tv.children()[0].type_name(), "TreeRow");
}

/// The topmost tree row in InspectorPanel must appear just below the header,
/// not in the middle of the tree area (verifies clip_rect + translate ordering).
#[test]
fn test_inspector_top_row_appears_at_top_of_tree_area() {
    use crate::geometry::{Rect, Size};
    use crate::text::Font;
    use crate::widget::{paint_subtree, InspectorNode, Widget};
    use crate::widgets::inspector::InspectorPanel;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let nodes: Rc<RefCell<Vec<InspectorNode>>> = Rc::new(RefCell::new(vec![InspectorNode {
        type_name: "Window",
        screen_bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
        depth: 0,
        margin: crate::layout_props::Insets::ZERO,
        padding: crate::layout_props::Insets::ZERO,
        h_anchor: crate::layout_props::HAnchor::FIT,
        v_anchor: crate::layout_props::VAnchor::FIT,
        path: vec![],
        properties: vec![],
    }]));
    let hovered = Rc::new(RefCell::new(None));
    let mut panel = InspectorPanel::new(Arc::clone(&font), Rc::clone(&nodes), Rc::clone(&hovered));

    let pw = 240u32;
    let ph = 400u32;
    let mut fb = Framebuffer::new(pw, ph);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        panel.layout(Size::new(pw as f64, ph as f64));
        panel.set_bounds(Rect::new(0.0, 0.0, pw as f64, ph as f64));
        paint_subtree(&mut panel, &mut ctx);
    }

    // The tree area starts just below the header (HEADER_H=30px from top).
    // In Y-down rendering (row 0 = top), check that pixel row 35 (just below header)
    // has non-white content — meaning a tree row rendered there.
    let row_y_down: usize = 35;
    // In the framebuffer (Y-up storage), convert to Y-up row index:
    let row_y_up = (ph as usize).saturating_sub(1).saturating_sub(row_y_down);
    let pixels = fb.pixels();
    let mut found_non_white = false;
    for px in 5..(pw as usize - 5) {
        let idx = (row_y_up * pw as usize + px) * 4;
        if idx + 3 < pixels.len() {
            let r = pixels[idx] as u32;
            let g = pixels[idx + 1] as u32;
            let b = pixels[idx + 2] as u32;
            // Check for non-background color (background is near-white #F7F7F9 = 247,247,249)
            if r < 240 || g < 240 || b < 240 {
                found_non_white = true;
                break;
            }
        }
    }
    assert!(
        found_non_white,
        "expected non-white content just below the header at row_y_down={}, but got all-white — check clip_rect+translate ordering in InspectorPanel::paint()",
        row_y_down
    );
}

/// During a live drag, the dragged node must not appear in row_widgets
/// (to avoid double-rendering behind the ghost).
#[test]
fn test_treeview_drag_node_excluded_from_row_widgets() {
    use crate::event::{Event, Modifiers, MouseButton};
    use crate::geometry::{Point, Size};
    use crate::widgets::tree_view::{NodeIcon, TreeView};
    use std::sync::Arc;
    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());
    use crate::geometry::Rect;
    let mut tv = TreeView::new(Arc::clone(&font)).with_drag_enabled();
    tv.add_root("Node A", NodeIcon::File);
    tv.add_root("Node B", NodeIcon::File);
    tv.layout(Size::new(200.0, 100.0));
    tv.set_bounds(Rect::new(0.0, 0.0, 200.0, 100.0));
    // 2 rows before drag
    assert_eq!(tv.children().len(), 2);

    // Start a drag on the first row (click at row-center in Y-up: h - 0.5*rh = 100 - 12 = 88)
    tv.on_event(&Event::MouseDown {
        pos: Point::new(50.0, 88.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    // Move far enough to exceed drag threshold (>4px)
    tv.on_event(&Event::MouseMove {
        pos: Point::new(50.0, 78.0),
    });

    // Re-layout with live drag active
    tv.layout(Size::new(200.0, 100.0));
    // The dragged node should be excluded → only 1 row widget
    assert_eq!(
        tv.children().len(),
        1,
        "dragged node must be excluded from row_widgets during live drag"
    );
}

// ---------------------------------------------------------------------------
// Composition tests — Button with Label child
// ---------------------------------------------------------------------------

/// Button must have exactly one child widget of type "Label" after layout.
#[test]
fn test_button_has_label_child() {
    use crate::text::Font;
    use std::sync::Arc;
    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut btn = Button::new("Click me", font);
    btn.layout(Size::new(200.0, 40.0));
    assert_eq!(
        btn.children().len(),
        1,
        "Button must expose exactly one Label child"
    );
    assert_eq!(
        btn.children()[0].type_name(),
        "Label",
        "Button's child must be a Label widget"
    );
}

/// After layout(), the Label child must have tight text bounds and be centred
/// within the button area.
#[test]
fn test_button_label_child_fills_button() {
    use crate::text::Font;
    use std::sync::Arc;
    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut btn = Button::new("Click me", font);
    let size = btn.layout(Size::new(300.0, 50.0));
    let label_bounds = btn.children()[0].bounds();
    // Tight bounds: label width must be less than button width.
    assert!(
        label_bounds.width < size.width,
        "Label width must be tight (less than button width); got label_w={} btn_w={}",
        label_bounds.width,
        size.width
    );
    assert!(label_bounds.width > 0.0, "Label width must be positive");
    assert!(label_bounds.height > 0.0, "Label height must be positive");
    // Label must be horizontally centred: x ≈ (button_w - label_w) / 2.
    let expected_x = (size.width - label_bounds.width) * 0.5;
    assert!(
        (label_bounds.x - expected_x).abs() < 1.0,
        "Label must be horizontally centred; expected x≈{:.1}, got x={:.1}",
        expected_x,
        label_bounds.x
    );
    // Label must be vertically centred.
    let expected_y = (size.height - label_bounds.height) * 0.5;
    assert!(
        (label_bounds.y - expected_y).abs() < 1.0,
        "Label must be vertically centred; expected y≈{:.1}, got y={:.1}",
        expected_y,
        label_bounds.y
    );
}

/// Label::properties() must include text, font_size, and has_backbuffer.
#[test]
fn test_label_properties() {
    use crate::{text::Font, Label};
    use std::sync::Arc;
    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let label = Label::new("Hello", font).with_font_size(13.0);
    let props: std::collections::HashMap<_, _> = label.properties().into_iter().collect();
    assert!(
        props.contains_key("text"),
        "Label must expose 'text' property"
    );
    assert_eq!(props["text"], "Hello");
    assert!(
        props.contains_key("has_backbuffer"),
        "Label must expose 'has_backbuffer'"
    );
    // Default `buffered = true` opts Label into the grayscale AGG
    // backbuffer path.  Runtime toggles off when LCD is enabled
    // globally (see `Label::backbuffer_cache_mut`), but the property
    // reflects the user-visible opt-in.
    assert_eq!(props["has_backbuffer"], "true");
}

/// Button properties must include the label text.
#[test]
fn test_button_properties() {
    use crate::text::Font;
    use std::sync::Arc;
    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let btn = Button::new("Primary Action", font);
    let props: std::collections::HashMap<_, _> = btn.properties().into_iter().collect();
    assert!(
        props.contains_key("label"),
        "Button must expose 'label' property"
    );
    assert_eq!(props["label"], "Primary Action");
}

/// collect_inspector_nodes must show Button at depth 0 and Label at depth 1.
#[test]
fn test_button_inspector_hierarchy() {
    use crate::{
        geometry::{Point, Rect},
        text::Font,
        widget::collect_inspector_nodes,
    };
    use std::sync::Arc;
    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut btn = Button::new("OK", font);
    btn.layout(Size::new(200.0, 40.0));
    btn.set_bounds(Rect::new(0.0, 0.0, 200.0, 40.0));
    let mut nodes = Vec::new();
    let boxed: Box<dyn Widget> = Box::new(btn);
    collect_inspector_nodes(boxed.as_ref(), 0, Point::new(0.0, 0.0), &mut nodes);
    assert!(nodes.len() >= 2, "Must have at least Button + Label nodes");
    assert_eq!(nodes[0].type_name, "Button");
    assert_eq!(nodes[0].depth, 0);
    assert_eq!(nodes[1].type_name, "Label");
    assert_eq!(nodes[1].depth, 1);
}

/// Invisible widgets must be excluded from the inspector snapshot (and their
/// entire subtrees must be omitted).  A closed Window should disappear from
/// the inspector just as it disappears from the rendered scene.
#[test]
fn test_invisible_widget_excluded_from_inspector() {
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
        children: Vec::new(),
    };

    let mut nodes = Vec::new();
    collect_inspector_nodes(&visible, 0, Point::ORIGIN, &mut nodes);
    assert_eq!(nodes.len(), 1, "visible widget appears once");
    assert_eq!(nodes[0].type_name, "ToggleWidget");

    nodes.clear();
    collect_inspector_nodes(&hidden, 0, Point::ORIGIN, &mut nodes);
    assert!(
        nodes.is_empty(),
        "invisible widget produces no inspector nodes"
    );
}

/// `toggle_on_row_click = false` (the inspector's mode): clicking a row
/// SELECTS it but does NOT toggle its expansion state.  This prevents the
/// inspector tree from collapsing to one visible line when the user clicks on
/// the root node to inspect it.
#[test]
fn test_treeview_click_selects_without_collapsing_when_flag_off() {
    use crate::event::Modifiers;
    use crate::geometry::{Point, Size};
    use crate::text::Font;
    use std::sync::Arc;
    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));

    let mut tv = crate::widgets::tree_view::TreeView::new(Arc::clone(&font)).with_row_height(20.0);
    // toggle_on_row_click defaults to false — inspector mode.

    let root = tv.add_root("Root", crate::widgets::tree_view::NodeIcon::Package);
    tv.expand(root);
    tv.add_child(root, "Child A", crate::widgets::tree_view::NodeIcon::File);
    tv.add_child(root, "Child B", crate::widgets::tree_view::NodeIcon::File);

    use crate::widget::Widget;
    tv.layout(Size::new(300.0, 200.0));
    tv.set_bounds(crate::geometry::Rect::new(0.0, 0.0, 300.0, 200.0));

    // 3 visible rows (Root expanded, 2 children).
    assert_eq!(
        tv.children().len(),
        3,
        "should have Root + 2 children visible"
    );

    // Click on the ROOT row body — well past the expand icon (EXPAND_W=18,
    // ICON_W+GAP=18) so x=80 is clearly in the label area, not on the toggle.
    let root_row_y = 200.0 - 20.0 * 0.5; // centre of first row (Y-up)
    tv.on_event(&crate::event::Event::MouseDown {
        pos: Point::new(80.0, root_row_y),
        button: crate::event::MouseButton::Left,
        modifiers: Modifiers::default(),
    });

    // Re-layout to reflect any expansion change.
    tv.layout(Size::new(300.0, 200.0));

    // Root must still be expanded: children must still be visible.
    assert_eq!(
        tv.children().len(),
        3,
        "clicking root row must NOT collapse it when toggle_on_row_click = false"
    );
}

/// `toggle_on_row_click = true` (file-explorer mode): clicking anywhere on a
/// row with children ALSO toggles its expansion — consistent with VS Code /
/// Cursor file-tree behaviour.
#[test]
fn test_treeview_click_collapses_when_flag_on() {
    use crate::event::Modifiers;
    use crate::geometry::{Point, Size};
    use crate::text::Font;
    use std::sync::Arc;
    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));

    let mut tv = crate::widgets::tree_view::TreeView::new(Arc::clone(&font))
        .with_row_height(20.0)
        .with_toggle_on_row_click(); // file-explorer mode

    let root = tv.add_root("Root", crate::widgets::tree_view::NodeIcon::Package);
    tv.expand(root);
    tv.add_child(root, "Child A", crate::widgets::tree_view::NodeIcon::File);

    use crate::widget::Widget;
    tv.layout(Size::new(300.0, 200.0));
    tv.set_bounds(crate::geometry::Rect::new(0.0, 0.0, 300.0, 200.0));

    assert_eq!(tv.children().len(), 2, "Root + 1 child visible initially");

    // Click the root row body (not the toggle icon).
    let root_row_y = 200.0 - 20.0 * 0.5;
    tv.on_event(&crate::event::Event::MouseDown {
        pos: Point::new(80.0, root_row_y), // well to the right of the expand icon
        button: crate::event::MouseButton::Left,
        modifiers: Modifiers::default(),
    });

    tv.layout(Size::new(300.0, 200.0));

    assert_eq!(
        tv.children().len(),
        1,
        "clicking root row body must collapse it when toggle_on_row_click = true"
    );
}
