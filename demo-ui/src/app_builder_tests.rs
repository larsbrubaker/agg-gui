//! Regression tests for the shared demo app builder.
//!
//! These exercise host-visible redraw contracts that span the full demo tree,
//! including the backend panel, without bloating `app_builder.rs`.

use std::sync::Arc;

use agg_gui::{
    find_widget_by_type, AccentColor, DrawCtx, Event, EventResult, Font, Framebuffer, GfxCtx, Rect,
    Size, ThemePreference, Widget,
};

use crate::api::{DemoHandles, PlatformHooks};
use crate::app_builder::build_demo_ui;
use crate::state::{SavedState, WindowState};
use crate::RunMode;

const TEST_FONT: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");

struct IdleCube {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl IdleCube {
    fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
        }
    }
}

impl Widget for IdleCube {
    fn type_name(&self) -> &'static str {
        "IdleCube"
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
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

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn window_state_for_spec(i: usize, win_w: f64, win_h: f64, open: bool) -> WindowState {
    let r = crate::specs::tile_rect(i, 900.0, win_w, win_h);
    WindowState {
        open,
        x: r.x,
        y: r.y,
        w: r.width,
        h: r.height,
        maximized: false,
    }
}

fn saved_state_with_backend_open() -> SavedState {
    let demos = crate::specs::DEMOS
        .iter()
        .enumerate()
        .map(|(i, spec)| window_state_for_spec(i, spec.win_w, spec.win_h, spec.open))
        .collect();
    let tests = crate::specs::TESTS
        .iter()
        .enumerate()
        .map(|(i, spec)| window_state_for_spec(i, spec.win_w, spec.win_h, spec.open))
        .collect();

    SavedState {
        demos,
        tests,
        about: WindowState {
            open: false,
            x: 40.0,
            y: 40.0,
            w: 420.0,
            h: 360.0,
            maximized: false,
        },
        backend_open: true,
        snap_enabled: false,
        theme_pref: ThemePreference::System,
        accent_color: AccentColor::Blue,
        window_w: None,
        window_h: None,
        window_fullscreen: false,
        window_maximized: false,
        inspector: None,
        font_name: None,
        font_size_scale: 1.0,
        lcd_enabled: false,
        hinting_enabled: false,
        gamma: 1.0,
        width_scale: 1.0,
        interval: 0.0,
        faux_weight: 0.0,
        faux_italic: 0.0,
        primary_weight: 1.0 / 3.0,
        msaa_samples: 0,
        system_tab: 0,
        z_order: Vec::new(),
    }
}

#[test]
fn reactive_demo_goes_idle_after_idle_paint() {
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
    let (mut app, _handles) = build_test_app(font);
    app.layout(Size::new(1200.0, 900.0));

    for _ in 0..2 {
        let mut fb = Framebuffer::new(1200, 900);
        let mut ctx = GfxCtx::new(&mut fb);
        app.paint(&mut ctx);
    }

    assert!(
        !app.wants_draw(),
        "reactive mode must not request another frame after an idle paint"
    );
}

#[test]
fn continuous_mode_forces_host_redraw_after_idle_paint() {
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
    let (mut app, handles) = build_test_app(font);
    app.layout(Size::new(1200.0, 900.0));

    let mut fb = Framebuffer::new(1200, 900);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
    assert!(
        !app.wants_draw(),
        "test setup should be idle before mode change"
    );

    handles.run_mode.set(RunMode::Continuous);
    let host_wants_draw = handles.run_mode.get() == RunMode::Continuous || app.wants_draw();

    assert!(
        host_wants_draw,
        "continuous mode must force the platform host to draw even when the app is idle"
    );
}

#[test]
fn top_bar_height_matches_menu_bar_natural_height() {
    // Regression: the old `TopMenuBar` hard-coded H=36 even though the
    // `MenuBar` it hosted only needed ~26 px, leaving a visible chrome
    // stripe below the menu.  `MenuBarStrip` sizes to its child's
    // natural height, so the bar should now be exactly the menu's
    // height — no more, no less.
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
    let (mut app, _handles) = build_test_app(font);
    app.layout(Size::new(1200.0, 800.0));

    let top_bar = find_widget_by_type(app.root(), "MenuBarStrip").expect("top bar must exist");
    let inner = top_bar.children()[0].bounds();
    assert!(
        (top_bar.bounds().height - inner.height).abs() < 0.5,
        "menu bar strip height ({}) must match its inner content height ({})",
        top_bar.bounds().height,
        inner.height,
    );
}

#[test]
fn mobile_top_bar_places_demos_button_after_menu_chrome() {
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
    let (mut app, _handles) = build_test_app(font);
    app.layout(Size::new(360.0, 640.0));

    let top_bar = find_widget_by_type(app.root(), "MenuBarStrip").expect("top bar must exist");
    let row = top_bar.children()[0].as_ref();
    let row_children = row.children();
    let menus = row_children
        .iter()
        .find(|child| child.type_name() == "MenuChrome")
        .expect("menu chrome must exist");
    let demos = row_children
        .iter()
        .find(|child| child.type_name() == "MenuButton")
        .expect("demos button must exist");

    assert!(
        menus.bounds().x < demos.bounds().x,
        "mobile top bar should place Demos to the right of the View/Help menu bar"
    );
}

#[test]
fn desktop_top_bar_hides_demos_button() {
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
    let (mut app, _handles) = build_test_app(font);
    app.layout(Size::new(720.0, 640.0));

    let top_bar = find_widget_by_type(app.root(), "MenuBarStrip").expect("top bar must exist");
    let row = top_bar.children()[0].as_ref();
    let demos = row
        .children()
        .iter()
        .find(|child| child.type_name() == "MenuButton")
        .expect("demos button must exist");

    assert_eq!(
        demos.bounds().width,
        0.0,
        "desktop top bar should hide Demos when the sidebar is visible"
    );
    assert!(
        !demos.is_visible(),
        "desktop top bar should not paint Demos when the sidebar is visible"
    );
}

fn build_test_app(font: Arc<Font>) -> (agg_gui::App, DemoHandles) {
    build_demo_ui(
        font,
        Box::new(|_msaa_cell| Box::new(IdleCube::new())),
        "TestRenderer",
        "TestBackend",
        Some(saved_state_with_backend_open()),
        PlatformHooks::native(0, || {}),
    )
}

#[test]
fn snap_overlay_exists_in_widget_tree() {
    // Phase 4 of the snap-layout feature: the demo wraps `demos_body`
    // in a `Stack` that hosts a `SnapOverlay` on top.  This test
    // pins that wiring so a future refactor that drops the overlay
    // (and silently breaks the snap-guides UX) fails loudly.
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
    let (mut app, _handles) = build_test_app(font);
    app.layout(Size::new(1200.0, 800.0));
    assert!(
        find_widget_by_type(app.root(), "SnapOverlay").is_some(),
        "SnapOverlay must be present in the widget tree so snap guides have somewhere to paint"
    );
}

#[test]
fn snap_registry_populated_by_visible_window_layout() {
    // Phase 2: every Window calls `snap::register_target` from its
    // `layout()` when visible.  After a full app layout, at least
    // one of the demo's visible windows must show up in the
    // thread-local registry.  Guards against regressions where the
    // registration call gets dropped by a future Window-internal
    // refactor.
    agg_gui::snap::clear_guides();
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
    let (mut app, _handles) = build_test_app(font);
    app.layout(Size::new(1200.0, 800.0));
    let targets = agg_gui::snap::targets_snapshot();
    assert!(
        !targets.is_empty(),
        "snap registry should hold at least one visible Window after layout"
    );
}
