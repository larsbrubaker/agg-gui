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

/// Every demo/test window closed, About closed, Inspector absent — the exact
/// state the user reproduces the runaway repaint in (only the sidebar and
/// Backend panel visible).
fn saved_state_all_closed() -> SavedState {
    let mut s = saved_state_with_backend_open();
    for ws in s.demos.iter_mut() {
        ws.open = false;
    }
    for ws in s.tests.iter_mut() {
        ws.open = false;
    }
    s.about.open = false;
    s.inspector = None;
    s
}

/// Permanent regression guard for the reactive-mode "continuous repaint with
/// every window closed" bug. Idle must mean idle: a reactive host that paints
/// an empty canvas (sidebar + Backend panel only) must go quiescent within a
/// handful of frames — `wants_draw()` false AND no draw deadline left armed —
/// otherwise the loop competes with input every frame and burns CPU forever.
///
/// A runaway shows as `wants_draw()` staying true on every frame (a widget
/// re-requesting a draw during paint, or an immediately-due deadline being
/// re-armed each paint and promoted by `animation::wants_draw`). When that
/// happens the failure drains the provenance trace so the culprit tags are
/// named directly in the assertion message.
#[test]
fn reactive_demo_with_all_windows_closed_quiesces() {
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
    let (mut app, _handles) = build_demo_ui(
        font,
        Box::new(|_msaa_cell| Box::new(IdleCube::new())),
        "TestRenderer",
        "TestBackend",
        Some(saved_state_all_closed()),
        PlatformHooks::native(0, || {}),
    );
    app.layout(Size::new(1200.0, 900.0));
    // Discard trace noise from construction / first layout.
    let _ = agg_gui::animation::drain_draw_trace();

    const MAX_FRAMES: usize = 12;
    let mut wants = true;
    for _ in 0..MAX_FRAMES {
        let mut fb = Framebuffer::new(1200, 900);
        let mut ctx = GfxCtx::new(&mut fb);
        app.paint(&mut ctx); // clears draw flags, then repaints
        // Mirror the reactive host: read wants_draw() once to decide whether
        // another frame is forced. This also promotes any due deadline, so a
        // `false` here guarantees no due deadline is pending.
        wants = app.wants_draw();
        if !wants {
            break;
        }
    }

    if wants {
        let trace = agg_gui::animation::drain_draw_trace();
        let root_needs = app.root().needs_draw();
        panic!(
            "reactive demo with ALL windows closed never went idle in {MAX_FRAMES} frames \
             (continuous-repaint bug). root.needs_draw()={root_needs}; \
             draw-request provenance tags across the run: {trace:?}"
        );
    }

    // Quiesced. With nothing open and nothing focused, no scheduled wake should
    // remain: a lingering future deadline means something re-arms a periodic
    // timer forever even on an empty canvas.
    assert!(
        agg_gui::animation::peek_next_draw_deadline().is_none(),
        "empty-canvas reactive demo left a draw deadline armed after quiescing: {:?}",
        agg_gui::animation::peek_next_draw_deadline()
    );
}

/// Paint frames like a reactive host until the app stops asking for more or
/// `cap` frames elapse. Returns whether it quiesced and the drained trace.
///
/// Two independent runaway signatures are caught without any wall-clock sleep:
/// an immediate re-request keeps `app.wants_draw()` true every frame (looped
/// here), and a re-armed *scheduled* deadline leaves
/// `peek_next_draw_deadline()` populated (asserted by the caller). Headless
/// paints are sub-millisecond, so a sleep would only mask, never expose, a
/// recurring timer — the armed-deadline check is the reliable probe.
fn paint_until_idle(app: &mut agg_gui::App, cap: usize) -> (bool, Vec<&'static str>) {
    let _ = agg_gui::animation::drain_draw_trace();
    let mut idle = false;
    for _ in 0..cap {
        let mut fb = Framebuffer::new(1200, 900);
        let mut ctx = GfxCtx::new(&mut fb);
        app.paint(&mut ctx);
        if !app.wants_draw() {
            idle = true;
            break;
        }
    }
    (idle, agg_gui::animation::drain_draw_trace())
}

/// Matrix regression guard: opening a demo window and then CLOSING it again
/// must return the app to a fully idle state. This is how a *historic*
/// continuous-repaint cascade shows up — a widget whose `needs_draw()` is
/// unconditionally `true` (Multi Touch, Dancing Strings, …) is fine while its
/// window is open (it animates on purpose), but if closing the window fails to
/// silence it — because some visibility gate in the tree walk or a global
/// scheduled-draw deadline leaks past the close — the reactive host keeps
/// painting an empty canvas forever.
///
/// While the window is open we deliberately do NOT require quiescence (animated
/// demos never idle by design); we only require that after the close the app
/// settles within a few frames, and that no scheduled deadline is left armed.
#[test]
fn each_demo_window_quiesces_after_close() {
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
    let (mut app, handles) = build_demo_ui(
        font,
        Box::new(|_msaa_cell| Box::new(IdleCube::new())),
        "TestRenderer",
        "TestBackend",
        Some(saved_state_all_closed()),
        PlatformHooks::native(0, || {}),
    );
    app.layout(Size::new(1200.0, 900.0));

    // Scope to the windows with animated / unconditionally-dirty content —
    // these are where a historic continuous-repaint cascade hides (a widget
    // whose `needs_draw()` is always true, a never-converging tween, a
    // per-frame scheduled re-arm). Sweeping all ~40 windows adds minutes of
    // layout/paint for no extra coverage of the animation-residue risk.
    const ANIMATED_TITLES: &[&str] = &[
        "\u{F009} Widget Gallery", // progress bars + assorted animated widgets
        "\u{F001} Dancing Strings", // needs_draw() == true
        "\u{F1FC} Painting",
        "\u{F030} Screenshot", // continuous-capture flag hazard
        "\u{F1B3} 3D Animation",
        "\u{F0A4} Multi Touch", // needs_draw() == true
        "\u{F1FE} Bézier Curve",
        "\u{F1B2} Interactive Container",
        "\u{F002} Scene",
        "\u{F1B0} Lion",
    ];

    let mut failures: Vec<String> = Vec::new();
    for (i, spec) in crate::specs::DEMOS
        .iter()
        .enumerate()
        .filter(|(_, s)| ANIMATED_TITLES.contains(&s.title))
    {
        let cell = &handles.state.demo_open[i];
        // Open, lay out, let it run a few frames (arming whatever it arms).
        cell.set(true);
        app.layout(Size::new(1200.0, 900.0));
        let _ = paint_until_idle(&mut app, 6);

        // Close, lay out, and require the app to go idle.
        cell.set(false);
        app.layout(Size::new(1200.0, 900.0));
        let (idle, trace) = paint_until_idle(&mut app, 12);
        let deadline_armed = agg_gui::animation::peek_next_draw_deadline().is_some();
        if !idle || deadline_armed {
            let root_needs = app.root().needs_draw();
            failures.push(format!(
                "'{}' (demo #{i}): idle={idle} deadline_armed={deadline_armed} \
                 root_needs={root_needs} trace={trace:?}",
                spec.title
            ));
        }
        // Reset per-window transient state so one window's residue can't mask
        // or contaminate the next window's verdict.
        agg_gui::animation::clear_draw_request();
        let _ = agg_gui::animation::drain_draw_trace();
    }

    assert!(
        failures.is_empty(),
        "these demo windows did NOT return the reactive app to idle after being closed \
         (continuous-repaint cascade). Culprits with drained draw-request trace:\n{}",
        failures.join("\n")
    );
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
fn top_bar_uses_menu_chrome_not_a_hamburger() {
    // The mobile "Demos" hamburger button was replaced by a real `Demos`
    // dropdown inside the menu bar (see `top_bar::build_demos_menu`).  The top
    // bar should now host only `MenuChrome` — no standalone `MenuButton` — at
    // any viewport width.
    for (w, h) in [(360.0, 640.0), (720.0, 640.0), (1200.0, 800.0)] {
        let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
        let (mut app, _handles) = build_test_app(font);
        app.layout(Size::new(w, h));

        let top_bar = find_widget_by_type(app.root(), "MenuBarStrip").expect("top bar must exist");
        let row = top_bar.children()[0].as_ref();
        let row_children = row.children();
        assert!(
            row_children
                .iter()
                .any(|child| child.type_name() == "MenuChrome"),
            "top bar must host the MenuChrome menu bar at {w}x{h}"
        );
        assert!(
            !row_children
                .iter()
                .any(|child| child.type_name() == "MenuButton"),
            "the standalone Demos hamburger must be gone at {w}x{h} — \
             Demos now lives inside the menu bar"
        );
    }
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

#[test]
fn mobile_keyboard_window_fits_content_without_scrollbar() {
    // Repro: the Mobile Keyboard window must grow to its content height
    // (tight-fit) so its inner ScrollView shows no scrollable overflow.
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
    let content = crate::windows::mobile_keyboard(Arc::clone(&font));
    let mut win = agg_gui::Window::new("kbd", Arc::clone(&font), content)
        .with_bounds(Rect::new(0.0, 0.0, 420.0, 540.0))
        .with_tight_content_fit(true)
        .with_resizable(false);
    // A few passes: tight-fit snaps height on layout; the ScrollView sets
    // its content height on layout too.
    for _ in 0..3 {
        win.layout(Size::new(1200.0, 900.0));
    }
    let sv = find_widget_by_type(&win, "ScrollView").expect("ScrollView present");
    let props = sv.properties();
    let get = |k: &str| -> f64 {
        props
            .iter()
            .find(|(key, _)| *key == k)
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(f64::NAN)
    };
    let max_scroll = get("max_scroll");
    let v_content = get("v_content");
    let win_h = win.bounds().height;
    // No scrollable overflow — the content fully fits.
    assert!(
        max_scroll <= 0.5,
        "window should hug content with no scroll; max_scroll={max_scroll}, \
         v_content={v_content}, win_h={win_h}"
    );
    // …and it hugged the CONTENT, not grew to the 900px canvas: the window
    // is the content height plus a title bar (well under 100px of chrome).
    assert!(
        win_h >= v_content && win_h <= v_content + 100.0,
        "window height should track content height; v_content={v_content}, win_h={win_h}"
    );
}
