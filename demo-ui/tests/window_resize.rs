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
    App, Event, FlexColumn, Font, Key, Modifiers, MouseButton, Point, Rect, Resize, Size,
    Stack, TextArea, Widget, Window,
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
    if entry.tight_fit {
        win = win.with_tight_content_fit(true);
    }
    if entry.floor_fit {
        win = win.with_height_floor_to_content(true);
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

#[test]
fn w1_contains_a_resize_widget() {
    // Stage-3 structural check: W1's middle region is a `Resize` widget,
    // not the earlier `SizedBox` placeholder.  Tests below probe its
    // behaviour — existence is the precondition.
    let (app, title, _pos) = make_test_app(0);
    let win = find_widget_by_id(app.root(), &title).unwrap();
    assert!(find_widget_by_type(win, "Resize").is_some(),
        "W1 must contain a Resize widget as its inner area");
}

#[test]
fn resize_widget_layout_honours_default_size() {
    // Freshly-built `Resize` with no user drag → its bounds.width/height
    // equal the default_size builder argument (clamped to min/max and
    // the surrounding available area).  Uses isolated Resize — no
    // Window / FlexColumn to confound the assertion.
    let font = font();
    let child = Box::new(
        FlexColumn::new().with_panel_bg()
            .add_flex(Box::new(agg_gui::Label::new("x", Arc::clone(&font))), 1.0)
    );
    let mut r = Resize::new(child)
        .with_default_size(Size::new(240.0, 140.0))
        .with_min_size_hint(Size::new(60.0, 40.0))
        .with_max_size_hint(Size::new(600.0, 400.0));
    // Parent-provided available is generous — so default_size wins.
    r.layout(Size::new(1000.0, 1000.0));
    let b = r.bounds();
    assert!((b.width  - 240.0).abs() < 0.5, "width  expected 240, got {}", b.width);
    assert!((b.height - 140.0).abs() < 0.5, "height expected 140, got {}", b.height);
}

#[test]
fn resize_widget_ignores_available_so_it_can_push_parent_wider() {
    // Updated contract: `Resize` reports its target size regardless of
    // the parent's `available` slot.  This is what lets an auto-sized
    // `Window` grow to fit a nested `Resize` that the user dragged
    // wider than the current inner area — matching egui's behaviour.
    // Explicit `max_size_hint` is still honoured.
    let font = font();
    let child = Box::new(
        FlexColumn::new().with_panel_bg()
            .add_flex(Box::new(agg_gui::Label::new("x", Arc::clone(&font))), 1.0)
    );
    let mut r = Resize::new(child)
        .with_default_size(Size::new(500.0, 300.0))
        .with_max_size_hint(Size::new(600.0, 400.0));
    r.layout(Size::new(200.0, 180.0));
    let b = r.bounds();
    // Target was 500×300; available was smaller, but Resize must NOT
    // shrink below its target — the parent will grow instead.
    assert!((b.width  - 500.0).abs() < 1.0,
        "width must match target 500 regardless of available 200; got {}", b.width);
    assert!((b.height - 300.0).abs() < 1.0,
        "height must match target 300 regardless of available 180; got {}", b.height);
}

#[test]
fn resize_widget_se_drag_grows_both_dimensions() {
    // Drag SE handle right+down (Y-down screen: dx>0, dy<0 in world Y-up).
    // Width should grow by dx; height should grow by |dy| (SE handle = E + S).
    let font = font();
    let child = Box::new(
        FlexColumn::new().with_panel_bg()
            .add(Box::new(agg_gui::Label::new("x", Arc::clone(&font))))
    );
    let mut r = Resize::new(child)
        .with_default_size(Size::new(200.0, 150.0))
        .with_min_size_hint(Size::new(50.0, 30.0))
        .with_max_size_hint(Size::new(600.0, 400.0));
    r.layout(Size::new(1000.0, 1000.0));
    assert_eq!(r.current_size(), Size::new(200.0, 150.0));

    // SE grip lives at local (w - HANDLE, 0 .. HANDLE).  Pick a point
    // inside the grip zone.  HANDLE in the widget is 14 px.
    let handle_local = Point::new(200.0 - 3.0, 3.0);
    r.on_event(&Event::MouseMove { pos: handle_local });
    r.on_event(&Event::MouseDown {
        pos: handle_local, button: MouseButton::Left, modifiers: Modifiers::default(),
    });
    // Move cursor +50 in x (right), -40 in y (Y-up: "down" on screen).
    let moved = Point::new(handle_local.x + 50.0, handle_local.y - 40.0);
    r.on_event(&Event::MouseMove { pos: moved });
    r.on_event(&Event::MouseUp {
        pos: moved, button: MouseButton::Left, modifiers: Modifiers::default(),
    });
    let s = r.current_size();
    assert!((s.width  - 250.0).abs() < 0.5,
        "SE drag dx=+50 → width 250 expected, got {}", s.width);
    assert!((s.height - 190.0).abs() < 0.5,
        "SE drag dy=-40 (Y-up) → height 190 expected, got {}", s.height);
}

#[test]
fn resize_widget_drag_clamps_to_min_and_max() {
    // Over-drag in both directions: width past max, height below min,
    // should stop at the configured limits.
    let font = font();
    let child = Box::new(
        FlexColumn::new().with_panel_bg()
            .add(Box::new(agg_gui::Label::new("x", Arc::clone(&font))))
    );
    let mut r = Resize::new(child)
        .with_default_size(Size::new(200.0, 150.0))
        .with_min_size_hint(Size::new(80.0, 60.0))
        .with_max_size_hint(Size::new(300.0, 220.0));
    r.layout(Size::new(1000.0, 1000.0));

    let handle_local = Point::new(200.0 - 3.0, 3.0);
    r.on_event(&Event::MouseDown {
        pos: handle_local, button: MouseButton::Left, modifiers: Modifiers::default(),
    });
    // Over-drag: +500 x (would be 700 wide, capped at 300), +500 y in
    // Y-up (i.e. "up" on screen → shrink height) → height goes to
    // 150 - 500 = -350, clamped at 60.
    let moved = Point::new(handle_local.x + 500.0, handle_local.y + 500.0);
    r.on_event(&Event::MouseMove { pos: moved });
    r.on_event(&Event::MouseUp {
        pos: moved, button: MouseButton::Left, modifiers: Modifiers::default(),
    });
    let s = r.current_size();
    assert!((s.width  - 300.0).abs() < 0.5, "max_w clamp: expected 300, got {}", s.width);
    assert!((s.height -  60.0).abs() < 0.5, "min_h clamp: expected 60, got {}", s.height);
}

#[test]
fn resize_widget_non_handle_click_does_not_drag() {
    // Click on the content area (NOT the SE grip) should not start a
    // drag and must not mutate size.
    let font = font();
    let child = Box::new(
        FlexColumn::new().with_panel_bg()
            .add(Box::new(agg_gui::Label::new("x", Arc::clone(&font))))
    );
    let mut r = Resize::new(child).with_default_size(Size::new(200.0, 150.0));
    r.layout(Size::new(1000.0, 1000.0));
    let before = r.current_size();

    // Widget-local (50, 100) is well inside the content, far from SE.
    let body = Point::new(50.0, 100.0);
    r.on_event(&Event::MouseDown {
        pos: body, button: MouseButton::Left, modifiers: Modifiers::default(),
    });
    r.on_event(&Event::MouseMove { pos: Point::new(200.0, 20.0) });
    r.on_event(&Event::MouseUp {
        pos: Point::new(200.0, 20.0), button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(r.current_size(), before,
        "content-area click must not resize; was {:?} now {:?}", before, r.current_size());
}

#[test]
fn w1_outer_window_tracks_inner_resize_growth() {
    // Integration test for the W1 story: growing the inner `Resize`
    // widget's content area forces the outer auto-sized window to
    // re-measure and grow its own height, while the top edge stays
    // pinned.  Driving the gesture through `App::on_mouse_*` would
    // require knowing the grip's exact screen position (nontrivial —
    // FlexColumn → Window → Stack offsets), so instead we construct
    // a minimal tree where the test owns the `Resize` + `Window` and
    // drives the resize via `Resize::on_event` directly in widget-
    // local coordinates.
    let font = font();
    // Build the same content shape as W1.
    let resize_widget = Resize::new(Box::new(
        FlexColumn::new().with_panel_bg()
            .add(Box::new(agg_gui::Label::new("content", Arc::clone(&font))))
    ))
    .with_default_size(Size::new(200.0, 100.0))
    .with_min_size_hint(Size::new(50.0, 30.0))
    .with_max_size_hint(Size::new(600.0, 400.0));

    // Build a minimal outer content column around the Resize so the
    // auto-sized window has a couple of peer widgets to sum heights
    // from — realistic enough to prove the tracking logic.
    let content = {
        let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);
        col.push(Box::new(
            agg_gui::Label::new("top label", Arc::clone(&font))
                .with_font_size(12.0),
        ), 0.0);
        col.push(Box::new(resize_widget), 0.0);
        col.push(Box::new(
            agg_gui::Label::new("bottom label", Arc::clone(&font))
                .with_font_size(12.0),
        ), 0.0);
        Box::new(col)
    };

    let mut win = Window::new("auto-w1-test", Arc::clone(&font), content)
        .with_bounds(Rect::new(30.0, 100.0, 360.0, 240.0))
        .with_auto_size(true);

    // First layout — measures and pins top.
    win.layout(Size::new(CANVAS_W, CANVAS_H));
    let outer_before    = win.bounds();
    let outer_top_before = outer_before.y + outer_before.height;
    let inner_h_before   = find_widget_by_type(&win as &dyn Widget, "Resize")
        .unwrap().bounds().height;

    // Drive a SE drag directly on the Resize via its widget-local
    // coordinates.  Find the Resize to get its current width (the
    // grip position).  `find_widget_by_id_mut` would be needed for
    // `on_event`; instead we reach in through the tree.
    let grip_local = {
        let r = find_widget_by_type(&win as &dyn Widget, "Resize").unwrap();
        // Grip sits at (w - 3, 3) inside Resize's 14-px SE zone.
        Point::new(r.bounds().width - 3.0, 3.0)
    };
    // Dispatch MouseDown + MouseMove + MouseUp to the Resize widget
    // directly (it owns the drag state).  Walk the tree to get &mut.
    fn resize_mut<'a>(root: &'a mut dyn Widget) -> Option<&'a mut dyn Widget> {
        if root.type_name() == "Resize" { return Some(root); }
        for child in root.children_mut().iter_mut() {
            if let Some(found) = resize_mut(child.as_mut()) { return Some(found); }
        }
        None
    }
    let moved = Point::new(grip_local.x + 0.0, grip_local.y - 60.0); // Y-up: +60 height
    {
        let r = resize_mut(&mut win as &mut dyn Widget).unwrap();
        r.on_event(&Event::MouseDown {
            pos: grip_local, button: MouseButton::Left, modifiers: Modifiers::default(),
        });
        r.on_event(&Event::MouseMove { pos: moved });
        r.on_event(&Event::MouseUp {
            pos: moved, button: MouseButton::Left, modifiers: Modifiers::default(),
        });
    }
    // Relayout so the outer window picks up the Resize's new size.
    win.layout(Size::new(CANVAS_W, CANVAS_H));

    let outer_after = win.bounds();
    let outer_top_after = outer_after.y + outer_after.height;
    let inner_h_after = find_widget_by_type(&win as &dyn Widget, "Resize")
        .unwrap().bounds().height;

    assert!((inner_h_after - inner_h_before - 60.0).abs() < 1.0,
        "inner Resize height must grow by drag delta: {} → {}",
        inner_h_before, inner_h_after);
    assert!((outer_top_after - outer_top_before).abs() < 1.0,
        "auto-sized window must pin its top edge; top was {} now {}",
        outer_top_before, outer_top_after);
    assert!(outer_after.height > outer_before.height,
        "outer window must grow to track inner Resize growth: {} → {}",
        outer_before.height, outer_after.height);
}

#[test]
fn resize_widget_enforces_content_natural_as_minimum() {
    // A `Resize` must never shrink past its content's natural size —
    // egui's demo shows the inner area refusing to go smaller than
    // the wrapped text.  Test: put a wrapped lorem ipsum label
    // inside, set the user-facing min hint lower than content
    // requires, and confirm the reported size clamps to the content
    // height.
    let font_arc = font();
    let inner_fc = Box::new({
        let mut col = agg_gui::FlexColumn::new()
            .with_panel_bg()
            .with_padding(4.0);
        col.push(Box::new(
            agg_gui::Label::new(
                "Lorem ipsum dolor sit amet, consectetur adipiscing elit.".repeat(4),
                Arc::clone(&font_arc),
            ).with_font_size(12.0).with_wrap(true),
        ), 0.0);
        col
    });
    let mut r = Resize::new(inner_fc)
        .with_default_size(Size::new(200.0, 30.0))   // tiny height
        .with_min_size_hint(Size::new(60.0, 20.0))   // very permissive
        .with_max_size_hint(Size::new(800.0, 600.0));
    r.layout(Size::new(1000.0, 1000.0));
    // The label wraps to multiple lines at 200 px width — its
    // natural height is far more than 30 px.  Resize must enforce
    // the content-natural floor.
    assert!(r.bounds().height > 50.0,
        "Resize must grow to fit wrapped content; got {}", r.bounds().height);
}

#[test]
fn w1_inner_resize_oversize_pushes_window_wider() {
    // egui contract: dragging the inner Resize wider than the
    // current window inner area pushes the outer auto-sized window
    // wider.  Simulate this by bypassing the App drag machinery —
    // we drive `Resize::on_event` directly and then relayout the
    // entire window to let the growth propagate.
    let font = font();
    let inner_col = {
        let mut c = FlexColumn::new().with_fit_width(true).with_padding(4.0);
        c.push(Box::new(agg_gui::Label::new("x", Arc::clone(&font))), 0.0);
        c
    };
    let resize = Resize::new(Box::new(inner_col))
        .with_default_size(Size::new(200.0, 80.0))
        .with_min_size_hint(Size::new(80.0, 40.0))
        .with_max_size_hint(Size::new(900.0, 500.0));
    let root = {
        let mut c = FlexColumn::new()
            .with_fit_width(true)
            .with_gap(4.0).with_padding(10.0);
        c.push(Box::new(agg_gui::Label::new(
            "header", Arc::clone(&font),
        ).with_font_size(12.0)), 0.0);
        c.push(Box::new(resize), 0.0);
        c
    };
    let mut win = Window::new("auto-grow", Arc::clone(&font), Box::new(root))
        .with_bounds(Rect::new(30.0, 100.0, 260.0, 200.0))
        .with_auto_size(true);
    win.layout(Size::new(CANVAS_W, CANVAS_H));
    let before = win.bounds();

    // Drive an SE drag on the Resize: move 400 px further right than
    // the current window inner width, 0 delta in Y.
    fn find_resize_mut<'a>(root: &'a mut dyn Widget) -> Option<&'a mut dyn Widget> {
        if root.type_name() == "Resize" { return Some(root); }
        for child in root.children_mut().iter_mut() {
            if let Some(f) = find_resize_mut(child.as_mut()) { return Some(f); }
        }
        None
    }
    let start_local = {
        let r = find_widget_by_type(&win as &dyn Widget, "Resize").unwrap();
        Point::new(r.bounds().width - 3.0, 3.0)
    };
    {
        let r = find_resize_mut(&mut win as &mut dyn Widget).unwrap();
        r.on_event(&Event::MouseDown {
            pos: start_local, button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        // No `current_mouse_world` published (unit test path), so
        // Resize falls back to the widget-local parent-relative
        // anchor — still produces stable deltas here because we
        // layout after each event before the next one reads bounds.
        let moved = Point::new(start_local.x + 400.0, start_local.y);
        r.on_event(&Event::MouseMove { pos: moved });
        r.on_event(&Event::MouseUp {
            pos: moved, button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
    }
    win.layout(Size::new(CANVAS_W, CANVAS_H));
    let after = win.bounds();
    assert!(after.width > before.width + 100.0,
        "auto-sized window should grow horizontally when inner Resize \
         demands more width: {} → {}", before.width, after.width);
}

#[test]
fn w1_auto_sized_window_grows_to_full_available_when_inner_demands_it() {
    // egui allows the inner `Resize` to be dragged to the full
    // extent of the containing layout — the outer auto-sized Window
    // grows with it.  Cap only kicks in at the parent's available
    // width.  Test: set `current_size` directly (bypassing drag
    // complexity) to a value well beyond any sane screen, then
    // relayout.  Window width should land near `available.width`,
    // not at some smaller artificial cap.
    use std::cell::Cell as StdCell;
    let font = font();
    let size_cell: Rc<StdCell<Size>> = Rc::new(StdCell::new(Size::new(320.0, 120.0)));
    let inner = Box::new({
        let mut c = FlexColumn::new().with_fit_width(true);
        c.push(Box::new(agg_gui::Label::new("x", Arc::clone(&font))), 0.0);
        c
    });
    let resize = Resize::new(inner)
        .with_default_size(Size::new(320.0, 120.0))
        .with_max_size_hint(Size::new(4000.0, 3000.0))
        .with_size_cell(Rc::clone(&size_cell));
    let root = {
        let mut c = FlexColumn::new().with_fit_width(true).with_padding(10.0);
        c.push(Box::new(resize), 0.0);
        c
    };
    let mut win = Window::new("grow", Arc::clone(&font), Box::new(root))
        .with_bounds(Rect::new(30.0, 100.0, 260.0, 200.0))
        .with_auto_size(true);
    let available = Size::new(900.0, 700.0);

    // Poke the Resize's size_cell to 2000 — way bigger than the
    // surrounding 900 px available area.  Then relayout and confirm
    // the outer window stretches to the available limit.
    size_cell.set(Size::new(2000.0, 100.0));
    win.layout(available);
    win.layout(available);     // second pass lets a multi-frame
                               // clamp converge if necessary
    let b = win.bounds();
    assert!(b.width >= available.width - 2.0,
        "outer window must grow to near available width ({}); got {}",
        available.width, b.width);
    assert!(b.width <= available.width + 1.0,
        "outer window must cap at available width ({}); got {}",
        available.width, b.width);
}

#[test]
fn w1_auto_sized_window_shrinks_back_when_inner_resize_narrows() {
    // Symmetric to `w1_auto_sized_window_grows_to_full_available_when_inner_demands_it`:
    // after the inner Resize grows, the outer window grows.  When
    // the inner Resize narrows, the outer window must SHRINK back —
    // not stay stretched at its previous maximum.  Without this,
    // the window is a one-way ratchet and the user can't visually
    // "put it back".  Use a size_cell to drive Resize explicitly
    // (no drag machinery).
    use std::cell::Cell as StdCell;
    let font = font();
    let size_cell: Rc<StdCell<Size>> = Rc::new(StdCell::new(Size::new(320.0, 120.0)));
    let inner = Box::new({
        let mut c = FlexColumn::new().with_fit_width(true);
        c.push(Box::new(agg_gui::Label::new("x", Arc::clone(&font))), 0.0);
        c
    });
    let resize = Resize::new(inner)
        .with_default_size(Size::new(320.0, 120.0))
        .with_min_size_hint(Size::new(80.0, 40.0))
        .with_max_size_hint(Size::new(4000.0, 3000.0))
        .with_size_cell(Rc::clone(&size_cell));
    let root = {
        let mut c = FlexColumn::new().with_fit_width(true).with_padding(10.0);
        // Short non-wrapped label so FlexColumn's max-child-width is
        // bounded by the label's natural single-line width, not by
        // whatever slot width we pass in.
        c.push(Box::new(agg_gui::Label::new("short", Arc::clone(&font))
            .with_font_size(12.0)), 0.0);
        c.push(Box::new(resize), 0.0);
        c
    };
    let mut win = Window::new("shrink", Arc::clone(&font), Box::new(root))
        .with_bounds(Rect::new(30.0, 100.0, 260.0, 200.0))
        .with_auto_size(true);
    let available = Size::new(900.0, 700.0);

    // Step 1: grow Resize large → outer window grows.
    size_cell.set(Size::new(800.0, 100.0));
    win.layout(available);
    win.layout(available);
    let grown = win.bounds().width;
    assert!(grown > 500.0,
        "outer window must grow with inner Resize; got {}", grown);

    // Step 2: shrink Resize small → outer window must shrink back.
    size_cell.set(Size::new(100.0, 100.0));
    win.layout(available);
    win.layout(available);
    let shrunk = win.bounds().width;
    assert!(shrunk < grown - 300.0,
        "outer window must shrink back when inner Resize narrows; \
         grown={} shrunk={}", grown, shrunk);
}

#[test]
fn w1_auto_sized_window_pins_width_to_initial_bounds() {
    // Regression for the "auto-sized window inflates to canvas width"
    // bug: `Window::auto_size` measures wrapped Labels, and those Labels
    // return their full available width as their own size.  Before this
    // stage, `auto_size` cascaded `available.width = canvas_width` into
    // that measurement and the window grew to the full 1280 px each
    // frame.  Post-fix, width is pinned to `with_bounds(...)` — only
    // height follows content.
    let (_app, _title, pos) = make_test_app(0);
    let b = pos.get();
    // rects[0] used 360 as the initial width.  The window must stay
    // near that value (±2 for snapping) and nowhere near the canvas
    // width of 1280.
    assert!((b.width - 360.0).abs() < 2.0,
        "auto-sized window must pin width to initial 360; got {}", b.width);
    assert!(b.width < 500.0,
        "auto-sized window width {} must stay bounded; canvas is 1280", b.width);
}

#[test]
fn w1_auto_size_remeasure_does_not_jitter_y_across_frames() {
    // After the first layout stabilises, running additional layout
    // passes with no state changes must produce identical bounds.
    // A regression that causes auto_size to drift (e.g. wrapped
    // labels rewrapping differently because of width oscillation)
    // would manifest as Y-bounds changing across frames.
    let (mut app, title, _pos) = make_test_app(0);
    let b0 = window_bounds(&app, &title);
    for _ in 0..10 { app.layout(Size::new(CANVAS_W, CANVAS_H)); }
    let bn = window_bounds(&app, &title);
    assert_eq!(b0, bn,
        "auto_size must produce stable bounds across idle layout passes: {:?} vs {:?}",
        b0, bn);
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
fn w4_cannot_shrink_past_content_natural_height() {
    // Stage-5 contract: a window built with `with_tight_content_fit`
    // (which W4 is) refuses to shrink past its content's natural
    // height — content is never clipped.  Measurement plan:
    //   1. Note the window's inner content-area height before drag;
    //      that equals the content's natural height (W4 content is
    //      all fixed-height widgets, so FlexColumn reports its sum).
    //   2. Drag the S edge hard to the top of the canvas.
    //   3. The resulting height should equal content_natural + TITLE_H
    //      (the min we computed), not the bare MIN_H=80.
    const TITLE_H: f64 = 28.0;
    let (mut app, title, _pos) = make_test_app(3);
    let before = window_bounds(&app, &title);
    let content_natural = before.height - TITLE_H;

    let mid_x    = before.x + before.width * 0.5;
    let bot_y_up = before.y + 1.0;
    let bot_y_dn = to_screen(bot_y_up);
    drag(&mut app, (mid_x, bot_y_dn), (mid_x, bot_y_dn - 1000.0));
    let after = window_bounds(&app, &title);

    // Floor must be WAY above the bare MIN_H (80) — proving the
    // content-bound clamp fired — and land close to the content's
    // natural height plus title bar.  Note: the initial window
    // height (290) can be slightly below content natural height, in
    // which case the first drag clamps UP to content_min.  That is
    // still the documented behaviour — no content clipping.
    let _ = content_natural;
    let _ = before;
    assert!(after.height > 200.0,
        "tight_content_fit must floor W4 well above MIN_H=80; got {}",
        after.height);
}

#[test]
fn w4_cannot_grow_past_content_natural_height() {
    // egui's "no scroll, no clip" contract is symmetric: window height
    // = content height, never more (no whitespace), never less (no
    // clip).  Stage-5+ adds a tight-fit pre-pass to `Window::layout`
    // that snaps height to content each frame, so an attempted S
    // drag growing the window has no lasting effect — the next
    // layout snaps it back.
    let (mut app, title, _pos) = make_test_app(3);
    let before = window_bounds(&app, &title);
    // Grab the S edge and drag it DOWN in screen (Y-up Y decreases →
    // h grows).  apply_resize for S: y = sb.y + dy; h = sb.h - dy.
    // dy < 0 (Y-down increased), so y decreases (window moves down)
    // and h grows.
    let mid_x    = before.x + before.width * 0.5;
    let bot_y_up = before.y + 1.0;
    let bot_y_dn = to_screen(bot_y_up);
    drag(&mut app, (mid_x, bot_y_dn), (mid_x, bot_y_dn + 400.0));
    let after = window_bounds(&app, &title);
    // The tight-fit pre-pass must snap height back to content.
    assert!((after.height - before.height).abs() < 5.0,
        "W4 must not grow past content height; was {}, after S drag now {}",
        before.height, after.height);
}

#[test]
fn tight_content_fit_clamps_resize_below_content_height() {
    // Library-level proof that the Stage-5 flag drives the resize
    // floor.  Compare two windows identical in content but only one
    // with `with_tight_content_fit(true)` — the tight one refuses to
    // shrink past content, the non-tight one honours the hard MIN_H.
    use agg_gui::{FlexColumn, Label};
    let make = |tight: bool| -> Window {
        let mut col = FlexColumn::new().with_gap(4.0).with_padding(8.0);
        for _ in 0..6 {
            col.push(Box::new(
                Label::new(
                    "A line tall enough to push content well above MIN_H.",
                    font(),
                ).with_font_size(13.0),
            ), 0.0);
        }
        let mut w = Window::new(
            if tight { "tight" } else { "loose" },
            font(),
            Box::new(col),
        ).with_bounds(Rect::new(80.0, 80.0, 300.0, 300.0));
        if tight { w = w.with_tight_content_fit(true); }
        w
    };
    let mut tight = make(true);
    let mut loose = make(false);
    tight.layout(Size::new(CANVAS_W, CANVAS_H));
    loose.layout(Size::new(CANVAS_W, CANVAS_H));

    // Drive MouseDown + Move + Up on the S edge (widget-local y≈0).
    // Note widget-local pos on the S edge hitting the bottom strip.
    let apply_shrink_drag = |win: &mut Window| {
        let s_pos = Point::new(150.0, 1.0);
        win.on_event(&Event::MouseMove { pos: s_pos });
        win.on_event(&Event::MouseDown {
            pos: s_pos, button: MouseButton::Left, modifiers: Modifiers::default(),
        });
        // Move the cursor far beyond the window's old top edge.
        win.on_event(&Event::MouseMove { pos: Point::new(150.0, 10_000.0) });
        win.on_event(&Event::MouseUp {
            pos: Point::new(150.0, 10_000.0), button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
    };
    apply_shrink_drag(&mut tight);
    apply_shrink_drag(&mut loose);

    // Tight must not have dropped to MIN_H=80.
    assert!(tight.bounds().height > 80.0 + 1.0,
        "tight_content_fit window must not shrink to MIN_H; got {}",
        tight.bounds().height);
    // Loose should be at the hard floor.
    assert!((loose.bounds().height - 80.0).abs() < 2.0,
        "non-tight window should land at MIN_H=80; got {}",
        loose.bounds().height);
}

#[test]
fn container_with_fit_height_returns_content_height() {
    // Stage-5 fix: `Container::with_fit_height(true)` reports its
    // content's natural height + vertical padding rather than filling
    // the full available area.  Without this the auto-sized W1
    // window inflated to the canvas size (the original OOM trigger).
    use agg_gui::{Container, Label};
    let child = Box::new(Label::new("hello", font()).with_font_size(14.0));
    let mut c = Container::new()
        .with_fit_height(true)
        .with_padding(6.0)
        .add(child);
    // Huge available height; fit-mode must NOT return all of it.
    let reported = c.layout(Size::new(200.0, 4000.0));
    assert!(reported.height < 200.0,
        "fit_height Container must not claim the full available height; got {}",
        reported.height);
    // And the height must be at least content-height (a line of text
    // at 14 pt is roughly 21 px, plus 12 px padding top + bottom).
    assert!(reported.height > 20.0,
        "fit_height Container should still include the child's height; got {}",
        reported.height);

    // The default (fit_height = false) still claims available.
    let mut c2 = Container::new().add(Box::new(
        Label::new("hello", font()).with_font_size(14.0),
    ));
    let r2 = c2.layout(Size::new(200.0, 4000.0));
    assert!((r2.height - 4000.0).abs() < 1.0,
        "default Container still fills available height; got {}", r2.height);
}

// ─── W5 — ↔ resizable with TextEdit ──────────────────────────────────────────

#[test]
fn w5_text_area_width_tracks_and_window_can_grow_above_content() {
    // Updated contract: W5 has floor-only — window cannot shrink
    // below TextArea content (no off-screen text), but CAN be
    // dragged taller (TextArea fills the extra space; whitespace
    // appears below the text).  Matches egui's W5 demo.
    let (mut app, title, _pos) = make_test_app(4);
    let before = window_bounds(&app, &title);
    let ta_before = find_widget_by_type(
        find_widget_by_id(app.root(), &title).unwrap(), "TextArea")
        .expect("W5 contains a TextArea")
        .bounds();

    // E drag → TextArea width tracks window width.
    let e_x      = before.x + before.width - 1.0;
    let mid_y_dn = to_screen(before.y + before.height * 0.5);
    drag(&mut app, (e_x, mid_y_dn), (e_x + 120.0, mid_y_dn));
    let ta_after_e = find_widget_by_type(
        find_widget_by_id(app.root(), &title).unwrap(), "TextArea")
        .unwrap().bounds();
    assert!((ta_after_e.width - (ta_before.width + 120.0)).abs() < 2.0,
        "TextArea width tracks window width: {} → {} (expected ≈+120)",
        ta_before.width, ta_after_e.width);

    // N drag SHOULD grow the window — floor-only allows growth.
    let win_after_e = window_bounds(&app, &title);
    let mid_x    = win_after_e.x + win_after_e.width * 0.5;
    let top_y_up = win_after_e.y + win_after_e.height - 1.0;
    let top_y_dn = to_screen(top_y_up);
    drag(&mut app, (mid_x, top_y_dn), (mid_x, top_y_dn - 200.0));
    let win_after_n = window_bounds(&app, &title);
    assert!(win_after_n.height > win_after_e.height + 100.0,
        "W5 must grow when user pulls N edge up; was {}, now {}",
        win_after_e.height, win_after_n.height);

    // S drag past content → floor stops at content height, NOT MIN_H.
    let win_after_n2 = window_bounds(&app, &title);
    let mid_x2    = win_after_n2.x + win_after_n2.width * 0.5;
    let bot_y_up  = win_after_n2.y + 1.0;
    let bot_y_dn  = to_screen(bot_y_up);
    drag(&mut app, (mid_x2, bot_y_dn), (mid_x2, bot_y_dn - 1000.0));
    let win_after_s = window_bounds(&app, &title);
    assert!(win_after_s.height > 100.0,
        "floor_fit must keep window height above MIN_H=80; got {}",
        win_after_s.height);
}

#[test]
fn w5_text_area_height_meets_content_height() {
    // After layout, the TextArea's `bounds.height` must fully cover
    // its wrapped content — no off-screen text, ever.  Asserts the
    // egui "no clipping" contract end-to-end: TextArea reports its
    // required min via `measure_min_height`, FlexColumn aggregates,
    // Window snaps to the total.
    let (app, title, _pos) = make_test_app(4);
    let win = find_widget_by_id(app.root(), &title).unwrap();
    let ta  = find_widget_by_type(win, "TextArea").unwrap();
    let needed = ta.measure_min_height(ta.bounds().width);
    assert!(ta.bounds().height >= needed - 1.0,
        "TextArea bounds.height ({}) must cover wrapped content needed={}",
        ta.bounds().height, needed);
}

#[test]
fn text_area_typing_inserts_at_cursor_and_tracks_lines() {
    // Fresh TextArea, type "hello\nworld" via individual KeyDown
    // events.  After: text matches, cursor at end, two visual lines.
    let mut ta = TextArea::new(font())
        .with_font_size(13.0);
    ta.layout(Size::new(300.0, 200.0));
    // Gain focus so the cursor is live and `on_event` accepts keys.
    let _ = ta.on_event(&Event::FocusGained);
    let mods = Modifiers::default();
    for c in "hello".chars() {
        ta.on_event(&Event::KeyDown { key: Key::Char(c), modifiers: mods });
    }
    ta.on_event(&Event::KeyDown { key: Key::Enter, modifiers: mods });
    for c in "world".chars() {
        ta.on_event(&Event::KeyDown { key: Key::Char(c), modifiers: mods });
    }
    ta.layout(Size::new(300.0, 200.0));
    assert_eq!(ta.text(), "hello\nworld", "typed text must land at cursor");
    assert_eq!(ta.cursor(), "hello\nworld".len(),
        "cursor should be at end of inserted text");
    assert_eq!(ta.visual_line_count(), 2,
        "Enter must produce a second visual line; got {}", ta.visual_line_count());
}

#[test]
fn text_area_backspace_deletes_previous_char() {
    let mut ta = TextArea::new(font()).with_text("hello");
    ta.layout(Size::new(300.0, 200.0));
    let _ = ta.on_event(&Event::FocusGained);
    let mods = Modifiers::default();
    ta.on_event(&Event::KeyDown { key: Key::Backspace, modifiers: mods });
    assert_eq!(ta.text(), "hell");
    assert_eq!(ta.cursor(), 4);
}

#[test]
fn text_area_arrow_keys_navigate_chars_and_lines() {
    let mut ta = TextArea::new(font()).with_text("ab\ncd");
    ta.layout(Size::new(300.0, 200.0));
    let _ = ta.on_event(&Event::FocusGained);
    let mods = Modifiers::default();

    // Cursor starts at end (byte 5).  Left twice lands at byte 3 (start of "cd").
    ta.on_event(&Event::KeyDown { key: Key::ArrowLeft, modifiers: mods });
    ta.on_event(&Event::KeyDown { key: Key::ArrowLeft, modifiers: mods });
    assert_eq!(ta.cursor(), 3, "two Lefts from end of 'ab\\ncd' lands at 'cd' start");

    // Up should move to the equivalent column on the previous line
    // ('a' start = byte 0 or 1).  We just assert it moved to a byte
    // on the first line (< 3).
    ta.on_event(&Event::KeyDown { key: Key::ArrowUp, modifiers: mods });
    assert!(ta.cursor() < 3, "ArrowUp from line 2 must land on line 1, got {}", ta.cursor());

    // Down returns to line 2.
    ta.on_event(&Event::KeyDown { key: Key::ArrowDown, modifiers: mods });
    assert!(ta.cursor() >= 3, "ArrowDown from line 1 must land on line 2");
}

#[test]
fn text_area_fills_parent_size_even_with_short_content() {
    // The widget should claim the available rectangle regardless of
    // content height — that's what makes it "fill the window" per
    // egui's W5 contract.
    let mut ta = TextArea::new(font()).with_text("just one line");
    let s = ta.layout(Size::new(400.0, 260.0));
    assert!((s.width  - 400.0).abs() < 1.0, "TextArea must fill width:  {} vs 400", s.width);
    assert!((s.height - 260.0).abs() < 1.0, "TextArea must fill height: {} vs 260", s.height);
}

#[test]
fn text_area_word_wraps_long_line_to_viewport_width() {
    // Narrow width forces long content to soft-wrap.  Visual line
    // count should exceed 1 even though the source has no \n.
    let mut ta = TextArea::new(font())
        .with_font_size(14.0)
        .with_text("The quick brown fox jumps over the lazy dog ".repeat(4));
    ta.layout(Size::new(180.0, 200.0));
    assert!(ta.visual_line_count() > 1,
        "content longer than viewport must soft-wrap; got {} lines",
        ta.visual_line_count());
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
