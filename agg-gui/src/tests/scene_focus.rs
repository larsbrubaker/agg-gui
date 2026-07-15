//! Integration tests for keyboard focus + inspector reaching *inside* a
//! [`Scene`](crate::widgets::Scene).
//!
//! These guard the first-class-child-transform work: because the Scene's
//! content lives in `children()` and the pan/zoom is injected through
//! [`Widget::child_transform`](crate::widget::Widget::child_transform), the
//! framework's traversals map coordinates through the transform, so a widget
//! hosted in a zoomed Scene can:
//!   * receive Tab focus and typed input;
//!   * receive click-to-focus at its on-screen (transformed) position;
//!   * appear in the inspector at its transformed screen bounds.
//!
//! A minimal `SpyField` stand-in avoids font/layout coupling: it reports itself
//! focusable, records `FocusGained` and typed characters into shared cells, and
//! consumes its own `MouseDown` (so a click on it never falls through to the
//! Scene's pan gesture).

use super::*;
use crate::draw_ctx::DrawCtx;
use crate::geometry::Rect;
use crate::{Event, EventResult, Scene};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// A focusable leaf that records focus + typing into shared cells.
struct SpyField {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    focused: Rc<Cell<bool>>,
    typed: Rc<RefCell<String>>,
}

impl SpyField {
    fn new(focused: Rc<Cell<bool>>, typed: Rc<RefCell<String>>) -> Self {
        Self {
            bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
            children: Vec::new(),
            focused,
            typed,
        }
    }
}

impl Widget for SpyField {
    fn type_name(&self) -> &'static str {
        "SpyField"
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
        Size::new(100.0, 100.0)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn is_focusable(&self) -> bool {
        true
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::FocusGained => {
                self.focused.set(true);
                EventResult::Consumed
            }
            Event::FocusLost => {
                self.focused.set(false);
                EventResult::Consumed
            }
            Event::KeyDown {
                key: Key::Char(c), ..
            } => {
                self.typed.borrow_mut().push(*c);
                EventResult::Consumed
            }
            // Consume our own press so a click on the field never bubbles up
            // to the Scene as a pan gesture.
            Event::MouseDown { .. } => EventResult::Consumed,
            _ => EventResult::Ignored,
        }
    }
}

/// Build an App whose root is a Scene hosting a 100×100 `SpyField`, laid out in
/// a 200×200 viewport.  With the default zoom range the auto-fit lands on
/// zoom = 2.0, offset = (0, 0): scene point `p` maps to screen `2p`.
fn scene_app() -> (App, Rc<Cell<bool>>, Rc<RefCell<String>>) {
    let focused = Rc::new(Cell::new(false));
    let typed = Rc::new(RefCell::new(String::new()));
    let field = SpyField::new(Rc::clone(&focused), Rc::clone(&typed));
    let scene = Scene::new(Box::new(field)).with_content_size(Size::new(100.0, 100.0));
    let mut app = App::new(Box::new(scene));
    app.layout(Size::new(200.0, 200.0));
    (app, focused, typed)
}

/// Tab moves focus into the Scene's hosted field, and subsequent key events
/// are delivered to it — proving both `collect_focusable` reaches inside the
/// Scene and the focus dispatch path descends through it.
#[test]
fn tab_focuses_field_inside_scene_and_types() {
    let (mut app, focused, typed) = scene_app();
    assert!(
        app.focused_widget_type_name().is_none(),
        "nothing focused before Tab"
    );

    app.on_key_down(Key::Tab, Modifiers::default());
    assert_eq!(
        app.focused_widget_type_name(),
        Some("SpyField"),
        "Tab must reach the field hosted inside the Scene"
    );
    assert!(focused.get(), "the field must have received FocusGained");

    app.on_key_down(Key::Char('H'), Modifiers::default());
    app.on_key_down(Key::Char('i'), Modifiers::default());
    assert_eq!(
        typed.borrow().as_str(),
        "Hi",
        "typed characters must land in the focused field inside the Scene"
    );
}

/// Clicking at the field's *on-screen* position (which, at zoom 2, is twice its
/// scene-space position) focuses it — proving click-to-focus maps the pointer
/// through the child transform.
#[test]
fn click_to_focus_maps_through_scene_zoom() {
    let (mut app, focused, _typed) = scene_app();

    // Scene fit is zoom = 2, offset = 0, so scene (25, 25) is at screen
    // (50, 50).  `on_mouse_down` takes physical Y-DOWN coords; flip Y against
    // the 200-tall viewport so the logical Y-up click lands at (50, 50).
    let click_x = 50.0;
    let click_y_up = 50.0;
    app.on_mouse_down(
        click_x,
        200.0 - click_y_up,
        MouseButton::Left,
        Modifiers::default(),
    );

    assert_eq!(
        app.focused_widget_type_name(),
        Some("SpyField"),
        "a click at the field's transformed screen position must focus it"
    );
    assert!(focused.get(), "the field must have received FocusGained");
}

/// Inert content that hit-tests but never consumes anything — a press on it
/// must fall through (bubble) to the Scene as a background pan gesture.
struct InertContent {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}
impl Widget for InertContent {
    fn type_name(&self) -> &'static str {
        "InertContent"
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
        Size::new(100.0, 100.0)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Read the Scene root's `scene_rect` inspector property.
fn scene_rect_prop(app: &App) -> String {
    app.collect_inspector_nodes()
        .into_iter()
        .find(|n| n.type_name == "Scene")
        .and_then(|n| {
            n.properties
                .iter()
                .find(|(k, _)| *k == "scene_rect")
                .map(|(_, v)| v.clone())
        })
        .expect("Scene node with scene_rect property")
}

/// A press+drag on empty (non-consuming) content bubbles up to the Scene and
/// pans the view — proving the background-gesture path still works now that the
/// content is a first-class child dispatched to first.
#[test]
fn drag_on_inert_content_pans_scene() {
    let scene = Scene::new(Box::new(InertContent {
        bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
        children: Vec::new(),
    }))
    .with_content_size(Size::new(100.0, 100.0));
    let mut app = App::new(Box::new(scene));
    app.layout(Size::new(200.0, 200.0));

    let before = scene_rect_prop(&app);

    // Press at screen (100,100), drag to (140,120), release.  `on_mouse_*`
    // take physical Y-DOWN coords; flip against the 200-tall viewport.
    app.on_mouse_down(100.0, 200.0 - 100.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(140.0, 200.0 - 120.0);
    app.on_mouse_up(140.0, 200.0 - 120.0, MouseButton::Left, Modifiers::default());

    let after = scene_rect_prop(&app);
    assert_ne!(
        before, after,
        "dragging the empty background must pan the Scene view"
    );
}

/// The inspector snapshot includes the hosted field, and its `screen_bounds`
/// reflect the Scene's zoom (100×100 scene content → 200×200 on screen).
#[test]
fn inspector_sees_scene_content_at_transformed_bounds() {
    let (app, _focused, _typed) = scene_app();
    let nodes = app.collect_inspector_nodes();

    let field = nodes
        .iter()
        .find(|n| n.type_name == "SpyField")
        .expect("the Scene's hosted field must appear in the inspector snapshot");
    let b = field.screen_bounds;
    assert!(
        (b.x - 0.0).abs() < 1e-6
            && (b.y - 0.0).abs() < 1e-6
            && (b.width - 200.0).abs() < 1e-6
            && (b.height - 200.0).abs() < 1e-6,
        "hosted field screen_bounds must reflect the Scene's 2× zoom; got {:?}",
        b
    );
}
