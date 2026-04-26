use super::*;

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
    assert!(
        (observed_top - initial_top).abs() < 1.0,
        "auto-size must pin the top edge at Y-up={initial_top}, got {observed_top}"
    );
    // And the height actually changed — i.e. the window didn't stay
    // stuck on the initial 240 px if content wants a different size.
    // (The content is content-larger-than-240, so height should grow.)
    assert_ne!(
        b.height, 240.0,
        "auto-size must measure content, not keep initial height"
    );
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
    assert!(
        find_widget_by_type(win, "Resize").is_some(),
        "W1 must contain a Resize widget as its inner area"
    );
}

#[test]
fn resize_widget_layout_honours_default_size() {
    // Freshly-built `Resize` with no user drag → its bounds.width/height
    // equal the default_size builder argument (clamped to min/max and
    // the surrounding available area).  Uses isolated Resize — no
    // Window / FlexColumn to confound the assertion.
    let font = font();
    let child = Box::new(
        FlexColumn::new()
            .with_panel_bg()
            .add_flex(Box::new(agg_gui::Label::new("x", Arc::clone(&font))), 1.0),
    );
    let mut r = Resize::new(child)
        .with_default_size(Size::new(240.0, 140.0))
        .with_min_size_hint(Size::new(60.0, 40.0))
        .with_max_size_hint(Size::new(600.0, 400.0));
    // Parent-provided available is generous — so default_size wins.
    r.layout(Size::new(1000.0, 1000.0));
    let b = r.bounds();
    assert!(
        (b.width - 240.0).abs() < 0.5,
        "width  expected 240, got {}",
        b.width
    );
    assert!(
        (b.height - 140.0).abs() < 0.5,
        "height expected 140, got {}",
        b.height
    );
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
        FlexColumn::new()
            .with_panel_bg()
            .add_flex(Box::new(agg_gui::Label::new("x", Arc::clone(&font))), 1.0),
    );
    let mut r = Resize::new(child)
        .with_default_size(Size::new(500.0, 300.0))
        .with_max_size_hint(Size::new(600.0, 400.0));
    r.layout(Size::new(200.0, 180.0));
    let b = r.bounds();
    // Target was 500×300; available was smaller, but Resize must NOT
    // shrink below its target — the parent will grow instead.
    assert!(
        (b.width - 500.0).abs() < 1.0,
        "width must match target 500 regardless of available 200; got {}",
        b.width
    );
    assert!(
        (b.height - 300.0).abs() < 1.0,
        "height must match target 300 regardless of available 180; got {}",
        b.height
    );
}

#[test]
fn resize_widget_se_drag_grows_both_dimensions() {
    // Drag SE handle right+down (Y-down screen: dx>0, dy<0 in world Y-up).
    // Width should grow by dx; height should grow by |dy| (SE handle = E + S).
    let font = font();
    let child = Box::new(
        FlexColumn::new()
            .with_panel_bg()
            .add(Box::new(agg_gui::Label::new("x", Arc::clone(&font)))),
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
        pos: handle_local,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    // Move cursor +50 in x (right), -40 in y (Y-up: "down" on screen).
    let moved = Point::new(handle_local.x + 50.0, handle_local.y - 40.0);
    r.on_event(&Event::MouseMove { pos: moved });
    r.on_event(&Event::MouseUp {
        pos: moved,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    let s = r.current_size();
    assert!(
        (s.width - 250.0).abs() < 0.5,
        "SE drag dx=+50 → width 250 expected, got {}",
        s.width
    );
    assert!(
        (s.height - 190.0).abs() < 0.5,
        "SE drag dy=-40 (Y-up) → height 190 expected, got {}",
        s.height
    );
}

#[test]
fn resize_widget_middle_drag_grows_both_dimensions_for_touch_scroll_bridge() {
    // Mobile touch scrolling is bridged through a synthetic middle-button drag.
    // Resize handles should treat that the same as a left-button drag.
    let font = font();
    let child = Box::new(
        FlexColumn::new()
            .with_panel_bg()
            .add(Box::new(agg_gui::Label::new("x", Arc::clone(&font)))),
    );
    let mut r = Resize::new(child)
        .with_default_size(Size::new(200.0, 150.0))
        .with_min_size_hint(Size::new(50.0, 30.0))
        .with_max_size_hint(Size::new(600.0, 400.0));
    r.layout(Size::new(1000.0, 1000.0));

    let handle_local = Point::new(200.0 - 3.0, 3.0);
    r.on_event(&Event::MouseDown {
        pos: handle_local,
        button: MouseButton::Middle,
        modifiers: Modifiers::default(),
    });
    let moved = Point::new(handle_local.x + 50.0, handle_local.y - 40.0);
    r.on_event(&Event::MouseMove { pos: moved });
    r.on_event(&Event::MouseUp {
        pos: moved,
        button: MouseButton::Middle,
        modifiers: Modifiers::default(),
    });

    let s = r.current_size();
    assert!((s.width - 250.0).abs() < 0.5, "got width {}", s.width);
    assert!((s.height - 190.0).abs() < 0.5, "got height {}", s.height);
}

#[test]
fn resize_widget_drag_clamps_to_min_and_max() {
    // Over-drag in both directions: width past max, height below min,
    // should stop at the configured limits.
    let font = font();
    let child = Box::new(
        FlexColumn::new()
            .with_panel_bg()
            .add(Box::new(agg_gui::Label::new("x", Arc::clone(&font)))),
    );
    let mut r = Resize::new(child)
        .with_default_size(Size::new(200.0, 150.0))
        .with_min_size_hint(Size::new(80.0, 60.0))
        .with_max_size_hint(Size::new(300.0, 220.0));
    r.layout(Size::new(1000.0, 1000.0));

    let handle_local = Point::new(200.0 - 3.0, 3.0);
    r.on_event(&Event::MouseDown {
        pos: handle_local,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    // Over-drag: +500 x (would be 700 wide, capped at 300), +500 y in
    // Y-up (i.e. "up" on screen → shrink height) → height goes to
    // 150 - 500 = -350, clamped at 60.
    let moved = Point::new(handle_local.x + 500.0, handle_local.y + 500.0);
    r.on_event(&Event::MouseMove { pos: moved });
    r.on_event(&Event::MouseUp {
        pos: moved,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    let s = r.current_size();
    assert!(
        (s.width - 300.0).abs() < 0.5,
        "max_w clamp: expected 300, got {}",
        s.width
    );
    assert!(
        (s.height - 60.0).abs() < 0.5,
        "min_h clamp: expected 60, got {}",
        s.height
    );
}

#[test]
fn resize_widget_non_handle_click_does_not_drag() {
    // Click on the content area (NOT the SE grip) should not start a
    // drag and must not mutate size.
    let font = font();
    let child = Box::new(
        FlexColumn::new()
            .with_panel_bg()
            .add(Box::new(agg_gui::Label::new("x", Arc::clone(&font)))),
    );
    let mut r = Resize::new(child).with_default_size(Size::new(200.0, 150.0));
    r.layout(Size::new(1000.0, 1000.0));
    let before = r.current_size();

    // Widget-local (50, 100) is well inside the content, far from SE.
    let body = Point::new(50.0, 100.0);
    r.on_event(&Event::MouseDown {
        pos: body,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    r.on_event(&Event::MouseMove {
        pos: Point::new(200.0, 20.0),
    });
    r.on_event(&Event::MouseUp {
        pos: Point::new(200.0, 20.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(
        r.current_size(),
        before,
        "content-area click must not resize; was {:?} now {:?}",
        before,
        r.current_size()
    );
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
        FlexColumn::new()
            .with_panel_bg()
            .add(Box::new(agg_gui::Label::new("content", Arc::clone(&font)))),
    ))
    .with_default_size(Size::new(200.0, 100.0))
    .with_min_size_hint(Size::new(50.0, 30.0))
    .with_max_size_hint(Size::new(600.0, 400.0));

    // Build a minimal outer content column around the Resize so the
    // auto-sized window has a couple of peer widgets to sum heights
    // from — realistic enough to prove the tracking logic.
    let content = {
        let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);
        col.push(
            Box::new(agg_gui::Label::new("top label", Arc::clone(&font)).with_font_size(12.0)),
            0.0,
        );
        col.push(Box::new(resize_widget), 0.0);
        col.push(
            Box::new(agg_gui::Label::new("bottom label", Arc::clone(&font)).with_font_size(12.0)),
            0.0,
        );
        Box::new(col)
    };

    let mut win = Window::new("auto-w1-test", Arc::clone(&font), content)
        .with_bounds(Rect::new(30.0, 100.0, 360.0, 240.0))
        .with_auto_size(true);

    // First layout — measures and pins top.
    win.layout(Size::new(CANVAS_W, CANVAS_H));
    let outer_before = win.bounds();
    let outer_top_before = outer_before.y + outer_before.height;
    let inner_h_before = find_widget_by_type(&win as &dyn Widget, "Resize")
        .unwrap()
        .bounds()
        .height;

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
        if root.type_name() == "Resize" {
            return Some(root);
        }
        for child in root.children_mut().iter_mut() {
            if let Some(found) = resize_mut(child.as_mut()) {
                return Some(found);
            }
        }
        None
    }
    let moved = Point::new(grip_local.x + 0.0, grip_local.y - 60.0); // Y-up: +60 height
    {
        let r = resize_mut(&mut win as &mut dyn Widget).unwrap();
        r.on_event(&Event::MouseDown {
            pos: grip_local,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        r.on_event(&Event::MouseMove { pos: moved });
        r.on_event(&Event::MouseUp {
            pos: moved,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
    }
    // Relayout so the outer window picks up the Resize's new size.
    win.layout(Size::new(CANVAS_W, CANVAS_H));

    let outer_after = win.bounds();
    let outer_top_after = outer_after.y + outer_after.height;
    let inner_h_after = find_widget_by_type(&win as &dyn Widget, "Resize")
        .unwrap()
        .bounds()
        .height;

    assert!(
        (inner_h_after - inner_h_before - 60.0).abs() < 1.0,
        "inner Resize height must grow by drag delta: {} → {}",
        inner_h_before,
        inner_h_after
    );
    assert!(
        (outer_top_after - outer_top_before).abs() < 1.0,
        "auto-sized window must pin its top edge; top was {} now {}",
        outer_top_before,
        outer_top_after
    );
    assert!(
        outer_after.height > outer_before.height,
        "outer window must grow to track inner Resize growth: {} → {}",
        outer_before.height,
        outer_after.height
    );
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
        let mut col = agg_gui::FlexColumn::new().with_panel_bg().with_padding(4.0);
        col.push(
            Box::new(
                agg_gui::Label::new(
                    "Lorem ipsum dolor sit amet, consectetur adipiscing elit.".repeat(4),
                    Arc::clone(&font_arc),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        col
    });
    let mut r = Resize::new(inner_fc)
        .with_default_size(Size::new(200.0, 30.0)) // tiny height
        .with_min_size_hint(Size::new(60.0, 20.0)) // very permissive
        .with_max_size_hint(Size::new(800.0, 600.0));
    r.layout(Size::new(1000.0, 1000.0));
    // The label wraps to multiple lines at 200 px width — its
    // natural height is far more than 30 px.  Resize must enforce
    // the content-natural floor.
    assert!(
        r.bounds().height > 50.0,
        "Resize must grow to fit wrapped content; got {}",
        r.bounds().height
    );
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
            .with_gap(4.0)
            .with_padding(10.0);
        c.push(
            Box::new(agg_gui::Label::new("header", Arc::clone(&font)).with_font_size(12.0)),
            0.0,
        );
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
        if root.type_name() == "Resize" {
            return Some(root);
        }
        for child in root.children_mut().iter_mut() {
            if let Some(f) = find_resize_mut(child.as_mut()) {
                return Some(f);
            }
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
            pos: start_local,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        // No `current_mouse_world` published (unit test path), so
        // Resize falls back to the widget-local parent-relative
        // anchor — still produces stable deltas here because we
        // layout after each event before the next one reads bounds.
        let moved = Point::new(start_local.x + 400.0, start_local.y);
        r.on_event(&Event::MouseMove { pos: moved });
        r.on_event(&Event::MouseUp {
            pos: moved,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
    }
    win.layout(Size::new(CANVAS_W, CANVAS_H));
    let after = win.bounds();
    assert!(
        after.width > before.width + 100.0,
        "auto-sized window should grow horizontally when inner Resize \
         demands more width: {} → {}",
        before.width,
        after.width
    );
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
    win.layout(available); // second pass lets a multi-frame
                           // clamp converge if necessary
    let b = win.bounds();
    assert!(
        b.width >= available.width - 2.0,
        "outer window must grow to near available width ({}); got {}",
        available.width,
        b.width
    );
    assert!(
        b.width <= available.width + 1.0,
        "outer window must cap at available width ({}); got {}",
        available.width,
        b.width
    );
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
        c.push(
            Box::new(agg_gui::Label::new("short", Arc::clone(&font)).with_font_size(12.0)),
            0.0,
        );
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
    assert!(
        grown > 500.0,
        "outer window must grow with inner Resize; got {}",
        grown
    );

    // Step 2: shrink Resize small → outer window must shrink back.
    size_cell.set(Size::new(100.0, 100.0));
    win.layout(available);
    win.layout(available);
    let shrunk = win.bounds().width;
    assert!(
        shrunk < grown - 300.0,
        "outer window must shrink back when inner Resize narrows; \
         grown={} shrunk={}",
        grown,
        shrunk
    );
}

#[test]
fn w1_auto_sized_window_fits_inner_resize_default_width() {
    // Regression for two related auto-size width bugs: the window must
    // not inflate to canvas width, and it must not keep stale whitespace
    // from the initial rect. W1's widest content is the inner `Resize`
    // at 320 px plus the root column's 10 px left/right padding.
    let (_app, _title, pos) = make_test_app(0);
    let b = pos.get();
    assert!(
        (b.width - 340.0).abs() < 2.0,
        "auto-sized window must fit inner Resize + padding (340); got {}",
        b.width
    );
    assert!(
        b.width < 500.0,
        "auto-sized window width {} must stay bounded; canvas is 1280",
        b.width
    );
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
    for _ in 0..10 {
        app.layout(Size::new(CANVAS_W, CANVAS_H));
    }
    let bn = window_bounds(&app, &title);
    assert_eq!(
        b0, bn,
        "auto_size must produce stable bounds across idle layout passes: {:?} vs {:?}",
        b0, bn
    );
}
