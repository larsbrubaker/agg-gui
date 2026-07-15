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

// ── Double-click-reset must survive an intervening hosted-child click ──

/// A small hosted widget that *consumes* its own press (like a button), so a
/// click on it captures the pointer and never bubbles to the Scene.  Sits at
/// scene (0,0)-(20,20).
struct ClickCatcher {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}
impl Widget for ClickCatcher {
    fn type_name(&self) -> &'static str {
        "ClickCatcher"
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
        Size::new(20.0, 20.0)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { .. } => EventResult::Consumed,
            _ => EventResult::Ignored,
        }
    }
}

/// 100×100 content that ignores its own events but hosts a `ClickCatcher` in
/// its bottom-left corner — so scene (5,5) hits the catcher (a "button") while
/// scene (80,80) is empty background.
struct MixedContent {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}
impl Widget for MixedContent {
    fn type_name(&self) -> &'static str {
        "MixedContent"
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
        self.children[0].set_bounds(Rect::new(0.0, 0.0, 20.0, 20.0));
        self.children[0].layout(Size::new(20.0, 20.0));
        Size::new(100.0, 100.0)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Build a Scene (zoom 2, offset 0) over `MixedContent`.
fn mixed_scene_app() -> App {
    let content = MixedContent {
        bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
        children: vec![Box::new(ClickCatcher {
            bounds: Rect::new(0.0, 0.0, 20.0, 20.0),
            children: Vec::new(),
        })],
    };
    let scene = Scene::new(Box::new(content)).with_content_size(Size::new(100.0, 100.0));
    let mut app = App::new(Box::new(scene));
    app.layout(Size::new(200.0, 200.0));
    app
}

/// Physical-coord click helpers (viewport 200 tall; flip Y-up → Y-down).
fn app_click(app: &mut App, sx: f64, sy_up: f64) {
    app.on_mouse_down(sx, 200.0 - sy_up, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(sx, 200.0 - sy_up, MouseButton::Left, Modifiers::default());
}
fn app_drag(app: &mut App, from: (f64, f64), to: (f64, f64)) {
    app.on_mouse_down(from.0, 200.0 - from.1, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(to.0, 200.0 - to.1);
    app.on_mouse_up(to.0, 200.0 - to.1, MouseButton::Left, Modifiers::default());
}

/// A background double-click that *straddles a hosted-child click* must NOT
/// reset the view.  Regression test for the false reset that appeared once the
/// hosted content became a first-class child: the child's press is consumed and
/// never bubbles to the Scene, so the two background clicks looked adjacent in
/// time.  The pointer-press epoch distinguishes them.
#[test]
fn bg_double_click_straddling_child_click_does_not_reset() {
    let mut app = mixed_scene_app();
    let fitted = scene_rect_prop(&app);

    // Pan a small, KNOWN amount so a reset is observable AND the hosted
    // ClickCatcher stays on-screen at a computable position.  Fit is zoom 2 /
    // offset 0; dragging (100,100)->(90,90) is a screen delta of (-10,-10), so
    // afterwards screen = 2*scene + (-10,-10).
    app_drag(&mut app, (100.0, 100.0), (90.0, 90.0));
    let panned = scene_rect_prop(&app);
    assert_ne!(panned, fitted, "the drag should have panned the view");

    // Screen positions under `screen = 2*scene - 10`:
    //   background scene (60,60) -> screen (110,110);
    //   ClickCatcher (scene 0..20) center scene (7.5,7.5) -> screen (5,5).
    app_click(&mut app, 110.0, 110.0); // background
    app_click(&mut app, 5.0, 5.0); // hosted "button" — consumes the press
    app_click(&mut app, 110.0, 110.0); // background again

    assert_eq!(
        scene_rect_prop(&app),
        panned,
        "a background double-click interrupted by a hosted-child click must NOT reset the view"
    );
}

/// Positive control: two *consecutive* background clicks (no child click
/// between) still reset the view through real App routing.
#[test]
fn bg_genuine_double_click_resets_view_via_app() {
    let mut app = mixed_scene_app();
    let fitted = scene_rect_prop(&app);

    app_drag(&mut app, (100.0, 100.0), (90.0, 90.0));
    assert_ne!(scene_rect_prop(&app), fitted, "the drag should have panned");

    // Two consecutive background clicks (screen (110,110) = scene (60,60)) →
    // reset back to the fitted view.
    app_click(&mut app, 110.0, 110.0);
    app_click(&mut app, 110.0, 110.0);

    assert_eq!(
        scene_rect_prop(&app),
        fitted,
        "a genuine background double-click must reset the view to the fitted rect"
    );
}
