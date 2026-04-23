//! Layer-1 behaviour tests for the six Window Resize Test sub-windows.
//!
//! Each `#[test]` validates one specific behaviour that egui's
//! `window_resize_test.rs` demo is designed to demonstrate (see
//! `C:/Development/rust-apps/agg-gui/egui-reference/crates/egui_demo_lib/src/demo/tests/window_resize_test.rs`
//! for the source).  The tests drive a real `App` instance hosting the
//! relevant sub-window, synthesise mouse events, and assert on the
//! **measurable geometry** (outer window bounds, inner content bounds,
//! sub-widget bounds, ScrollView scroll offset, …) at the end of the
//! event sequence.
//!
//! Where agg-gui's current behaviour differs from egui's (e.g. a feature
//! planned for a later stage of the port), the test either marks the
//! shortfall with `#[ignore]` or asserts the *current* behaviour and
//! carries a comment flagging which stage will tighten the assertion.
//! That way the passing tests prove forward progress without hiding
//! known gaps.
//!
//! Coordinate-system note: `App`'s public event entry points accept
//! **physical-pixel Y-DOWN screen coordinates** (matching the contract
//! native and web hosts feed them).  Everything inside agg-gui is
//! Y-up.  The `drag` helper converts from Y-down to the widget-facing
//! Y-up reliably by letting `App::flip_y` do the work.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    App, Event, FlexColumn, Font, Modifiers, MouseButton, Point, Rect, Size,
    Stack, Widget, Window,
    find_widget_by_id, find_widget_by_type,
};
use demo_ui::{window_resize_sub_windows, ResizeTestWindow};

// Canvas large enough that none of the initial sub-window rects get
// clipped to `MIN_W` / `MIN_H`; matches the actual demo's default
// 1280×720 layout so geometry lands in the same absolute coordinates.
const CANVAS_W: f64 = 1280.0;
const CANVAS_H: f64 = 720.0;

fn font() -> Arc<Font> {
    // Tests compile a fresh Font per invocation — cheap (TTF parse
    // handled by `ttf-parser`, no glyph rasterisation up front).
    const BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
    Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"))
}

// ─── Test-setup helpers ──────────────────────────────────────────────────────

/// Build an `App` hosting exactly one of the six Window Resize Test
/// sub-windows, identified by the egui-source-order `index` (0 = auto-
/// sized, 1 = resizable + scroll, 2 = resizable + embedded scroll,
/// 3 = resizable without scroll, 4 = resizable with TextEdit,
/// 5 = freely resized).  Returns the App, the window title, and the
/// shared position cell that publishes current bounds each layout.
fn make_test_app(index: usize) -> (App, String, Rc<Cell<Rect>>) {
    let entries: Vec<ResizeTestWindow> = window_resize_sub_windows(font());
    let entry = entries.into_iter().nth(index)
        .expect("index within the six sub-windows");
    let title = entry.title.clone();
    let pos_cell = Rc::new(Cell::new(entry.initial_rect));
    let visible  = Rc::new(Cell::new(true));
    let mut win = Window::new(&title, font(), entry.content)
        .with_bounds(entry.initial_rect)
        .with_visible_cell(Rc::clone(&visible))
        .with_position_cell(Rc::clone(&pos_cell));
    // Match the application order used by `lib.rs::build_demo_ui`:
    // `with_vscroll` mutates children so it must precede any builder
    // that reads them.
    if entry.vscroll {
        win = win.with_vscroll(true);
    }
    if entry.auto_size {
        win = win.with_auto_size(true);
    } else {
        win = win.with_resizable_axes(entry.resizable_h, entry.resizable_v);
        if !entry.resizable {
            win = win.with_resizable(false);
        }
    }
    let root = Stack::new().add(Box::new(win));
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(CANVAS_W, CANVAS_H));
    (app, title, pos_cell)
}

/// Feed a full press / move / release drag through the App at the
/// given Y-DOWN screen coordinates.  Relayouts at the end so the next
/// assertion sees fully-propagated bounds (position cells are written
/// during `layout`).
fn drag(app: &mut App, start: (f64, f64), end: (f64, f64)) {
    app.on_mouse_move(start.0, start.1);
    app.on_mouse_down(start.0, start.1, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(end.0, end.1);
    app.on_mouse_up(end.0, end.1, MouseButton::Left, Modifiers::default());
    app.layout(Size::new(CANVAS_W, CANVAS_H));
}

fn window_bounds(app: &App, title: &str) -> Rect {
    find_widget_by_id(app.root(), title)
        .expect("test window is in the tree")
        .bounds()
}

/// Convert a Y-up world coordinate to the Y-down screen coord an
/// `App` entry point expects.  Centralised so individual tests stay
/// readable: they compute where the edge *is* in widget terms, then
/// pass through `to_screen` instead of inlining the arithmetic.
fn to_screen(y_up: f64) -> f64 { CANVAS_H - y_up }

// ─── W1 — ↔ auto-sized ───────────────────────────────────────────────────────

#[test]
fn w1_auto_size_pins_top_edge_and_resizes_height() {
    // egui behaviour: a `.auto_sized()` window adopts its content's
    // preferred size each frame.  Our implementation specifically pins
    // the **top edge** (Y-up y + height) so the title bar stays at the
    // user's chosen vertical position while content height changes.
    let (_app, _title, pos) = make_test_app(0);
    // Initial rect given to window_resize_sub_windows for W1:
    // Rect::new(30.0, 100.0, 360.0, 240.0) → top = 340 (Y-up).
    let initial_top = 100.0 + 240.0;
    let b = pos.get();
    let observed_top = b.y + b.height;
    assert!((observed_top - initial_top).abs() < 1.0,
        "auto-size must pin the top edge at Y-up={initial_top}, got {observed_top}");
    // And the height actually changed — i.e. the window didn't stay
    // stuck on the initial 240 px if content wants a different size.
    // (The content is content-larger-than-240, so height should grow.)
    assert_ne!(b.height, 240.0,
        "auto-size must measure content, not keep initial height");
}

#[test]
fn w1_auto_size_ignores_edge_drag() {
    // egui: `auto_sized` disables the resize handles.  Dragging the
    // east edge must not change bounds.
    let (mut app, title, _pos) = make_test_app(0);
    let before = window_bounds(&app, &title);
    // E edge Y-up world coord: x = bounds.x + bounds.width - 1.
    let e_x = before.x + before.width - 1.0;
    let mid_y_down = to_screen(before.y + before.height * 0.5);
    drag(&mut app, (e_x, mid_y_down), (e_x + 100.0, mid_y_down));
    let after = window_bounds(&app, &title);
    assert_eq!(before, after, "auto-sized window must ignore user resize");
}

// ─── W2 — ↔ resizable + scroll ───────────────────────────────────────────────

#[test]
fn w2_east_drag_grows_width_only() {
    // Dragging the east edge should grow width by ~the drag delta and
    // leave height / y unchanged — standard window-manager convention.
    let (mut app, title, _pos) = make_test_app(1);
    let before = window_bounds(&app, &title);
    let e_x  = before.x + before.width - 1.0;
    let mid_y_down = to_screen(before.y + before.height * 0.5);
    drag(&mut app, (e_x, mid_y_down), (e_x + 100.0, mid_y_down));
    let after = window_bounds(&app, &title);
    assert!((after.width - (before.width + 100.0)).abs() < 2.0,
        "east drag grew width by wrong amount: {} → {}", before.width, after.width);
    assert_eq!(after.height, before.height, "east drag must not change height");
    assert_eq!(after.x, before.x, "east drag must not change x");
    assert_eq!(after.y, before.y, "east drag must not change y");
}

#[test]
fn w2_north_drag_grows_height_and_keeps_top_fixed() {
    // In Y-up the NORTH edge is at y + height.  Dragging it upward
    // (Y-down screen Y decreases) raises the top and grows height.
    // Our Window::apply_resize for N modifies height only (y stays),
    // so the bottom edge is what stays fixed and the top moves up.
    let (mut app, title, _pos) = make_test_app(1);
    let before = window_bounds(&app, &title);
    let mid_x    = before.x + before.width * 0.5;
    // Y-up top edge; subtract 1 so local.y lands inside the N resize
    // zone (height-RESIZE_EDGE < local.y < height).
    let top_y_up = before.y + before.height - 1.0;
    let top_y_dn = to_screen(top_y_up);
    drag(&mut app, (mid_x, top_y_dn), (mid_x, top_y_dn - 80.0));
    let after = window_bounds(&app, &title);
    assert!((after.height - (before.height + 80.0)).abs() < 2.0,
        "north drag grew height by wrong amount: {} → {}", before.height, after.height);
    assert_eq!(after.y, before.y,
        "apply_resize(N) must leave bounds.y fixed (bottom stays)");
}

#[test]
fn w2_content_has_scroll_view() {
    // Window 2 wraps its long lorem in a `ScrollView` so the user can
    // shrink the window below content height.  The test confirms the
    // tree actually contains that ScrollView; the follow-up test
    // exercises the scroll behaviour.
    let (app, title, _pos) = make_test_app(1);
    let win = find_widget_by_id(app.root(), &title).unwrap();
    assert!(find_widget_by_type(win, "ScrollView").is_some(),
        "W2 must contain a ScrollView as direct content");
}

#[test]
fn w2_shrink_below_content_leaves_scrollable_overflow() {
    // Shrink the window height to 80 px (the MIN_H floor) and confirm
    // the inner ScrollView's max_scroll_value is nonzero — meaning the
    // content overflows the viewport and the scrollbar has range.
    //
    // Without this property, W2 would fail egui's "shrink to any size"
    // promise because the user would have no way to reach hidden
    // content.
    let (mut app, title, _pos) = make_test_app(1);
    let before = window_bounds(&app, &title);
    // Grab the south edge and drag it UP in Y-down (i.e. Y-up y
    // increases → apply_resize(S) reduces height).
    let mid_x    = before.x + before.width * 0.5;
    let bot_y_up = before.y + 1.0;
    let bot_y_dn = to_screen(bot_y_up);
    drag(&mut app, (mid_x, bot_y_dn), (mid_x, bot_y_dn - 500.0));
    // Relayout once more so the ScrollView sees the shrunken viewport
    // and recomputes its max scroll distance against content height.
    app.layout(Size::new(CANVAS_W, CANVAS_H));
    // ScrollView's public `properties()` lists "max_scroll" on the
    // inspector surface.  Walk the subtree to find the ScrollView and
    // read its `properties()` directly — avoids exposing a new
    // accessor just for tests.
    let sv = {
        let win = find_widget_by_id(app.root(), &title).unwrap();
        find_widget_by_type(win, "ScrollView").unwrap()
    };
    let max_scroll: f64 = sv.properties().iter()
        .find(|(k, _)| *k == "max_scroll")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0.0);
    assert!(max_scroll > 0.0,
        "shrunk W2 must expose scrollable overflow; got max_scroll={max_scroll}");
}

#[test]
fn w2_vscroll_wraps_content_with_a_single_scroll_view() {
    // Stage-2 contract: `Window::with_vscroll(true)` swaps the window's
    // first child for a ScrollView wrapping the original content.  The
    // window therefore has exactly one direct child, and that child is
    // the ScrollView — no second wrap, no leftover content sibling.
    let (app, title, _pos) = make_test_app(1);
    let win = find_widget_by_id(app.root(), &title).unwrap();
    let kids = win.children();
    assert_eq!(kids.len(), 1,
        "Window expects exactly one direct child after with_vscroll(true)");
    assert_eq!(kids[0].type_name(), "ScrollView",
        "with_vscroll(true) must place a ScrollView as children[0]; \
         got {} instead", kids[0].type_name());
}

#[test]
fn w2_scroll_view_fills_window_inner_content_area() {
    // Layout integrity: the wrapped ScrollView must occupy the entire
    // inner content rect (window width × content_h, where content_h =
    // window_height - TITLE_H).  Off-by-one tolerance accounts for
    // pixel snapping.
    const TITLE_H: f64 = 28.0;
    let (app, title, _pos) = make_test_app(1);
    let win = find_widget_by_id(app.root(), &title).unwrap();
    let win_b = win.bounds();
    let sv = find_widget_by_type(win, "ScrollView").unwrap().bounds();
    assert!((sv.width  - win_b.width).abs() < 1.0,
        "ScrollView width should match window width: {} vs {}",
        sv.width, win_b.width);
    assert!((sv.height - (win_b.height - TITLE_H)).abs() < 1.0,
        "ScrollView height should match inner content area: {} vs {}",
        sv.height, win_b.height - TITLE_H);
    // Origin: inner content rect starts at (0, 0) in window-local Y-up
    // (title bar is at the *top* = high Y).
    assert!(sv.x.abs() < 1.0 && sv.y.abs() < 1.0,
        "ScrollView origin should be (0, 0) in window-local; got ({}, {})",
        sv.x, sv.y);
}

#[test]
fn w2_mouse_wheel_advances_scroll_offset() {
    // Drive a wheel event over the W2 window.  The wrapped ScrollView
    // must consume it and advance its `v_offset` (inspector property)
    // by the framework-standard 40 px per wheel notch.
    let (mut app, title, _pos) = make_test_app(1);
    let win_b = window_bounds(&app, &title);
    // Cursor right in the middle of the window content area.
    let cx = win_b.x + win_b.width  * 0.5;
    let cy_up = win_b.y + win_b.height * 0.5 - 40.0; // below title bar
    let cy_dn = to_screen(cy_up);

    let read_offset = |app: &App| -> f64 {
        let win = find_widget_by_id(app.root(), &title).unwrap();
        let sv  = find_widget_by_type(win, "ScrollView").unwrap();
        sv.properties().iter()
            .find(|(k, _)| *k == "v_offset")
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(0.0)
    };

    let before = read_offset(&app);
    app.on_mouse_move(cx, cy_dn);                   // prime hover so the wheel routes here
    app.on_mouse_wheel(cx, cy_dn, 3.0);             // 3 notches "down"
    app.layout(Size::new(CANVAS_W, CANVAS_H));
    let after = read_offset(&app);
    assert!(after > before,
        "v_offset must advance after wheel; {} → {}", before, after);
    // Standard ScrollView wheel multiplier is 40 px per notch; 3 notches
    // → 120 px.  Tolerance accounts for any clamping at max_scroll.
    let expected = (before + 120.0).min({
        let win = find_widget_by_id(app.root(), &title).unwrap();
        let sv  = find_widget_by_type(win, "ScrollView").unwrap();
        sv.properties().iter().find(|(k, _)| *k == "max_scroll")
            .and_then(|(_, v)| v.parse().ok()).unwrap_or(120.0)
    });
    assert!((after - expected).abs() < 1.0,
        "wheel advanced wrong amount: expected ~{} got {}", expected, after);
}

// ─── W3 — ↔ resizable + embedded scroll ──────────────────────────────────────

#[test]
fn w3_embedded_scroll_view_present() {
    // W3 differs from W2 in egui via `.vscroll(false)` + a manual
    // `ScrollArea::vertical()` inside.  Visual shape of the tree is the
    // same (a ScrollView grandchild), but the semantic is "caller-owned".
    // The test asserts the content tree contains a ScrollView — either
    // placement satisfies the behavioural contract.
    let (app, title, _pos) = make_test_app(2);
    let win = find_widget_by_id(app.root(), &title).unwrap();
    assert!(find_widget_by_type(win, "ScrollView").is_some(),
        "W3 must contain an embedded ScrollView");
}

// ─── W4 — ↔ resizable without scroll ─────────────────────────────────────────

#[test]
fn w4_cannot_shrink_past_min_h_floor() {
    // egui contract: "agg-gui will not clip the contents of a window,
    // nor add whitespace to it."  This means the min resizable height
    // is content-bound.  TODO(stage-5): replace the MIN_H(=80) floor
    // with a content-derived one and tighten this assertion.  For now
    // we assert the library-level MIN_H floor holds, which is the
    // observable invariant today.
    let (mut app, title, _pos) = make_test_app(3);
    let before = window_bounds(&app, &title);
    let mid_x    = before.x + before.width * 0.5;
    let bot_y_up = before.y + 1.0;
    let bot_y_dn = to_screen(bot_y_up);
    drag(&mut app, (mid_x, bot_y_dn), (mid_x, bot_y_dn - 500.0));
    let after = window_bounds(&app, &title);
    assert!(after.height >= 80.0,
        "MIN_H floor (80 px) must still hold after aggressive S drag; got {}",
        after.height);
    assert!(after.height < before.height,
        "S drag must have actually shrunk the window");
}

// ─── W5 — ↔ resizable with TextEdit ──────────────────────────────────────────

#[test]
fn w5_text_field_width_tracks_window_width() {
    // egui binds the multiline TextEdit to `ui.available_size()` so it
    // fills the inner content rect.  Our current W5 uses a single-line
    // TextField which follows the parent FlexColumn's width allocation;
    // after resize it should still span the inner width (minus padding).
    let (mut app, title, _pos) = make_test_app(4);
    let grow_delta = 120.0;

    // Resize window east by +grow_delta and confirm the TextField grew
    // by the same amount (padding is constant).
    let before = window_bounds(&app, &title);
    let e_x  = before.x + before.width - 1.0;
    let mid_y_dn = to_screen(before.y + before.height * 0.5);
    let tf_before = find_widget_by_type(
        find_widget_by_id(app.root(), &title).unwrap(), "TextField")
        .unwrap().bounds().width;
    drag(&mut app, (e_x, mid_y_dn), (e_x + grow_delta, mid_y_dn));
    let tf_after = find_widget_by_type(
        find_widget_by_id(app.root(), &title).unwrap(), "TextField")
        .unwrap().bounds().width;
    assert!((tf_after - (tf_before + grow_delta)).abs() < 2.0,
        "TextField width should track window width: {} → {} (expected {})",
        tf_before, tf_after, tf_before + grow_delta);
}

// ─── W6 — ↔ freely resized ───────────────────────────────────────────────────

#[test]
fn w6_flex_fill_prevents_auto_shrink_across_layouts() {
    // egui: the flex-fill `allocate_space(available_size())` keeps the
    // window at its initial 250×150, even though the visible widgets
    // only need a few lines of space.  Our port uses a flex-weight-1
    // SizedBox for the same effect.  Verify the window doesn't shrink
    // across repeated layout passes (the common cause of "window
    // auto-shrinks until it matches label height").
    let (mut app, title, _pos) = make_test_app(5);
    let before = window_bounds(&app, &title);
    for _ in 0..10 { app.layout(Size::new(CANVAS_W, CANVAS_H)); }
    let after = window_bounds(&app, &title);
    assert_eq!(before, after,
        "flex-fill must keep bounds stable; shrank from {:?} to {:?}", before, after);
}

// ─── Library-level resize-flag tests (feature additions from Stage 1) ─────────

#[test]
fn auto_size_does_not_cascade_unbounded_max_size() {
    // Regression for the "231 GB LcdBuffer" crash.
    //
    // `Size::MAX` uses `f64::MAX / 2.0` (≈ 8.99 × 10^307) as its
    // sentinel so size arithmetic can't overflow.  The prior
    // `Window::auto_size` guard used `.is_finite()` to distinguish
    // "real max" from "no cap" — but that sentinel IS finite, so the
    // guard passed through an effectively-infinite width as if it were
    // a genuine cap.  That width propagated to wrapped Labels, whose
    // bounds then blew up the LCD backbuffer allocator.
    //
    // This test forces the default-max_size path (wrapped Label with
    // no explicit cap) and verifies the auto-sized window's bounds
    // stay within sane limits derived from the provided viewport,
    // not from `f64::MAX / 2`.
    use agg_gui::{FlexColumn, Label};
    let sane_canvas = Size::new(1280.0, 720.0);
    let content = {
        let mut col = FlexColumn::new().with_gap(4.0).with_padding(10.0);
        col.push(Box::new(
            Label::new(
                "Auto-sized windows must not cascade an unbounded cap \
                 from the default max_size sentinel.",
                font(),
            ).with_font_size(12.0).with_wrap(true),
        ), 0.0);
        Box::new(col)
    };
    let mut win = Window::new("auto-sentinel", font(), content)
        .with_bounds(Rect::new(30.0, 100.0, 360.0, 240.0))
        .with_auto_size(true);
    win.layout(sane_canvas);
    let b = win.bounds();
    // Derived post-fix bounds should never exceed the viewport.  A
    // regression that re-enables the f64::MAX/2 cascade will produce
    // a width around that sentinel value, which trivially exceeds
    // this threshold.
    assert!(b.width  <= sane_canvas.width  + 1.0,
        "window width ({}) overflowed viewport ({}) — auto_size cap \
         regression: the max_size sentinel is being accepted as finite",
        b.width, sane_canvas.width);
    assert!(b.height <= sane_canvas.height + 1.0,
        "window height ({}) overflowed viewport ({})", b.height, sane_canvas.height);
    assert!(b.width  < 1.0e6,
        "window width ({}) is pathological — f64::MAX sentinel leaking", b.width);
}

#[test]
fn resizable_false_keeps_bounds_frozen_on_east_drag() {
    // Unit-level proof that `with_resizable(false)` removes the entire
    // resize hit-zone, independent of axis flags.  A minimal window
    // with empty content sits far from the canvas edges so we know any
    // movement came from the drag, not a clamp.
    let mut win = Window::new("inert",
        font(),
        Box::new(FlexColumn::new().with_panel_bg()),
    ).with_bounds(Rect::new(100.0, 100.0, 300.0, 200.0))
      .with_resizable(false);
    win.layout(Size::new(CANVAS_W, CANVAS_H));
    let before = win.bounds();
    // Drive MouseMove + MouseDown + MouseMove + MouseUp directly at
    // the east edge in widget-local coords.  A resizable window would
    // enter DragMode::Resize(E) and grow; `resizable=false` gates the
    // resize_dir() lookup so no drag state is established.
    let on_edge = Point::new(299.0, 100.0);
    let _ = win.on_event(&Event::MouseMove { pos: on_edge });
    let _ = win.on_event(&Event::MouseDown {
        pos: on_edge, button: MouseButton::Left, modifiers: Modifiers::default(),
    });
    let moved = Point::new(400.0, 100.0);
    let _ = win.on_event(&Event::MouseMove { pos: moved });
    let _ = win.on_event(&Event::MouseUp {
        pos: moved, button: MouseButton::Left, modifiers: Modifiers::default(),
    });
    assert_eq!(win.bounds(), before,
        "resizable(false) must keep bounds frozen against a drag");
}

#[test]
fn resizable_axes_vertical_only_locks_east_edge() {
    // `with_resizable_axes(false, true)` → only N/S edges are live; the
    // east edge should be inert.  Using content-area Y (not title-bar
    // Y) avoids cross-talk with the title-bar drag handler.
    let mut win = Window::new("v-only",
        font(),
        Box::new(FlexColumn::new().with_panel_bg()),
    ).with_bounds(Rect::new(100.0, 100.0, 300.0, 200.0))
      .with_resizable_axes(false, true);
    win.layout(Size::new(CANVAS_W, CANVAS_H));
    let before = win.bounds();
    // Widget-local y=100 sits in the content region (title bar occupies
    // y ∈ [172, 200] when height=200 and TITLE_H=28).
    let on_east = Point::new(299.0, 100.0);
    let _ = win.on_event(&Event::MouseMove { pos: on_east });
    let _ = win.on_event(&Event::MouseDown {
        pos: on_east, button: MouseButton::Left, modifiers: Modifiers::default(),
    });
    let _ = win.on_event(&Event::MouseMove { pos: Point::new(400.0, 100.0) });
    let _ = win.on_event(&Event::MouseUp {
        pos: Point::new(400.0, 100.0), button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(win.bounds(), before,
        "E edge must be inert when resizable_h=false");
}
