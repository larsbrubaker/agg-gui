use super::*;

// ─── W5 — ↔ resizable with TextEdit ──────────────────────────────────────────

#[test]
fn w5_text_area_width_tracks_and_window_resizes_freely() {
    // Updated contract (internal TextArea scrolling): W5 is now a plain
    // resizable window with NO floor-to-content.  The editor scrolls its own
    // overflow, so the window can be dragged both taller AND shorter — all the
    // way down to MIN_H — without clipping any text unreachably off-screen.
    // Matches egui's W5 (`.vscroll(false)` + `TextEdit::multiline`).
    let (mut app, title, _pos) = make_test_app(4);
    let before = window_bounds(&app, &title);
    let ta_before = find_widget_by_type(find_widget_by_id(app.root(), &title).unwrap(), "TextArea")
        .expect("W5 contains a TextArea")
        .bounds();

    // E drag → TextArea width tracks window width.
    let e_x = before.x + before.width - 1.0;
    let mid_y_dn = to_screen(before.y + before.height * 0.5);
    drag(&mut app, (e_x, mid_y_dn), (e_x + 120.0, mid_y_dn));
    let ta_after_e =
        find_widget_by_type(find_widget_by_id(app.root(), &title).unwrap(), "TextArea")
            .unwrap()
            .bounds();
    assert!(
        (ta_after_e.width - (ta_before.width + 120.0)).abs() < 2.0,
        "TextArea width tracks window width: {} → {} (expected ≈+120)",
        ta_before.width,
        ta_after_e.width
    );

    // N drag SHOULD grow the window — resizable both ways.
    let win_after_e = window_bounds(&app, &title);
    let mid_x = win_after_e.x + win_after_e.width * 0.5;
    let top_y_up = win_after_e.y + win_after_e.height - 1.0;
    let top_y_dn = to_screen(top_y_up);
    drag(&mut app, (mid_x, top_y_dn), (mid_x, top_y_dn - 200.0));
    let win_after_n = window_bounds(&app, &title);
    assert!(
        win_after_n.height > win_after_e.height + 100.0,
        "W5 must grow when user pulls N edge up; was {}, now {}",
        win_after_e.height,
        win_after_n.height
    );

    // S drag far down → with no floor-to-content the window now shrinks
    // freely toward MIN_H (80).  The editor scrolls internally, so shrinking
    // never leaves text clipped off-screen unreachably.
    let win_after_n2 = window_bounds(&app, &title);
    let mid_x2 = win_after_n2.x + win_after_n2.width * 0.5;
    let bot_y_up = win_after_n2.y + 1.0;
    let bot_y_dn = to_screen(bot_y_up);
    drag(&mut app, (mid_x2, bot_y_dn), (mid_x2, bot_y_dn - 1000.0));
    let win_after_s = window_bounds(&app, &title);
    assert!(
        win_after_s.height < win_after_n2.height && win_after_s.height <= 82.0,
        "W5 must shrink freely toward MIN_H=80 (internal scroll); \
         was {}, now {}",
        win_after_n2.height,
        win_after_s.height
    );
}

#[test]
fn w5_long_text_scrolls_internally_instead_of_growing_window() {
    // Updated contract (internal TextArea scrolling): W5 seeds the editor with
    // LOREM_IPSUM_LONG and scrolls it internally, like egui's
    // `TextEdit::multiline`.  The window is a plain resizable window that no
    // longer grows to fit all text, so the editor's wrapped content is TALLER
    // than its bounds — the overflow is reached by scrolling, never clipped
    // off-screen unreachably.  (Scroll reachability itself is unit-tested in
    // agg-gui's `text_area` module; see `w5_text_area_wheel_reaches_overflow`
    // below for the end-to-end drive.)
    let (app, title, _pos) = make_test_app(4);
    let win = find_widget_by_id(app.root(), &title).unwrap();
    let win_h = win.bounds().height;
    let ta = find_widget_by_type(win, "TextArea").unwrap();
    let needed = ta.measure_min_height(ta.bounds().width);
    assert!(
        needed > ta.bounds().height + 1.0,
        "LONG seed must overflow the fixed editor area (internal scroll): \
         needed={needed} bounds.h={}",
        ta.bounds().height
    );
    // The window did NOT balloon to the full content height — it stays near its
    // initial size, proving floor-to-content was removed for W5.
    assert!(
        win_h < needed,
        "window height ({win_h}) must stay below full content height \
         ({needed}); it scrolls internally rather than growing"
    );
}

#[test]
fn w5_text_area_wheel_reaches_overflow() {
    // End-to-end proof of the new contract: a fixed-height multiline editor
    // seeded with long text starts at the top and scrolls on the wheel, so the
    // lower content is reachable — no text is unreachably off-screen.
    let long = "Lorem ipsum dolor sit amet consectetur adipiscing elit ".repeat(60);
    let mut ta = TextArea::new(font()).with_font_size(12.5).with_text(&long);
    ta.layout(Size::new(280.0, 160.0));
    assert!(
        ta.measure_min_height(280.0) > 160.0,
        "long seed must overflow the fixed editor area"
    );
    assert_eq!(ta.scroll_offset(), 0.0, "editor starts scrolled to the top");
    // Wheel down (negative delta_y = content moves up) reveals lower lines.
    ta.on_event(&Event::MouseWheel {
        pos: Point::new(20.0, 20.0),
        delta_y: -600.0,
        delta_x: 0.0,
        modifiers: Modifiers::default(),
    });
    assert!(
        ta.scroll_offset() > 0.0,
        "wheel must scroll the long content, got {}",
        ta.scroll_offset()
    );
}

#[test]
fn text_area_typing_inserts_at_cursor_and_tracks_lines() {
    // Fresh TextArea, type "hello\nworld" via individual KeyDown
    // events.  After: text matches, cursor at end, two visual lines.
    let mut ta = TextArea::new(font()).with_font_size(13.0);
    ta.layout(Size::new(300.0, 200.0));
    // Gain focus so the cursor is live and `on_event` accepts keys.
    let _ = ta.on_event(&Event::FocusGained);
    let mods = Modifiers::default();
    for c in "hello".chars() {
        ta.on_event(&Event::KeyDown {
            key: Key::Char(c),
            modifiers: mods,
        });
    }
    ta.on_event(&Event::KeyDown {
        key: Key::Enter,
        modifiers: mods,
    });
    for c in "world".chars() {
        ta.on_event(&Event::KeyDown {
            key: Key::Char(c),
            modifiers: mods,
        });
    }
    ta.layout(Size::new(300.0, 200.0));
    assert_eq!(ta.text(), "hello\nworld", "typed text must land at cursor");
    assert_eq!(
        ta.cursor(),
        "hello\nworld".len(),
        "cursor should be at end of inserted text"
    );
    assert_eq!(
        ta.visual_line_count(),
        2,
        "Enter must produce a second visual line; got {}",
        ta.visual_line_count()
    );
}

#[test]
fn text_area_backspace_deletes_previous_char() {
    let mut ta = TextArea::new(font()).with_text("hello");
    ta.layout(Size::new(300.0, 200.0));
    let _ = ta.on_event(&Event::FocusGained);
    let mods = Modifiers::default();
    ta.on_event(&Event::KeyDown {
        key: Key::Backspace,
        modifiers: mods,
    });
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
    ta.on_event(&Event::KeyDown {
        key: Key::ArrowLeft,
        modifiers: mods,
    });
    ta.on_event(&Event::KeyDown {
        key: Key::ArrowLeft,
        modifiers: mods,
    });
    assert_eq!(
        ta.cursor(),
        3,
        "two Lefts from end of 'ab\\ncd' lands at 'cd' start"
    );

    // Up should move to the equivalent column on the previous line
    // ('a' start = byte 0 or 1).  We just assert it moved to a byte
    // on the first line (< 3).
    ta.on_event(&Event::KeyDown {
        key: Key::ArrowUp,
        modifiers: mods,
    });
    assert!(
        ta.cursor() < 3,
        "ArrowUp from line 2 must land on line 1, got {}",
        ta.cursor()
    );

    // Down returns to line 2.
    ta.on_event(&Event::KeyDown {
        key: Key::ArrowDown,
        modifiers: mods,
    });
    assert!(
        ta.cursor() >= 3,
        "ArrowDown from line 1 must land on line 2"
    );
}

#[test]
fn text_area_fills_parent_size_even_with_short_content() {
    // The widget should claim the available rectangle regardless of
    // content height — that's what makes it "fill the window" per
    // egui's W5 contract.
    let mut ta = TextArea::new(font()).with_text("just one line");
    let s = ta.layout(Size::new(400.0, 260.0));
    assert!(
        (s.width - 400.0).abs() < 1.0,
        "TextArea must fill width:  {} vs 400",
        s.width
    );
    assert!(
        (s.height - 260.0).abs() < 1.0,
        "TextArea must fill height: {} vs 260",
        s.height
    );
}

#[test]
fn text_area_word_wraps_long_line_to_viewport_width() {
    // Narrow width forces long content to soft-wrap.  Visual line
    // count should exceed 1 even though the source has no \n.
    let mut ta = TextArea::new(font())
        .with_font_size(14.0)
        .with_text("The quick brown fox jumps over the lazy dog ".repeat(4));
    ta.layout(Size::new(180.0, 200.0));
    assert!(
        ta.visual_line_count() > 1,
        "content longer than viewport must soft-wrap; got {} lines",
        ta.visual_line_count()
    );
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
    for _ in 0..10 {
        app.layout(Size::new(CANVAS_W, CANVAS_H));
    }
    let after = window_bounds(&app, &title);
    assert_eq!(
        before, after,
        "flex-fill must keep bounds stable; shrank from {:?} to {:?}",
        before, after
    );
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
        col.push(
            Box::new(
                Label::new(
                    "Auto-sized windows must not cascade an unbounded cap \
                 from the default max_size sentinel.",
                    font(),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
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
    assert!(
        b.width <= sane_canvas.width + 1.0,
        "window width ({}) overflowed viewport ({}) — auto_size cap \
         regression: the max_size sentinel is being accepted as finite",
        b.width,
        sane_canvas.width
    );
    assert!(
        b.height <= sane_canvas.height + 1.0,
        "window height ({}) overflowed viewport ({})",
        b.height,
        sane_canvas.height
    );
    assert!(
        b.width < 1.0e6,
        "window width ({}) is pathological — f64::MAX sentinel leaking",
        b.width
    );
}

#[test]
fn resizable_false_keeps_bounds_frozen_on_east_drag() {
    // Unit-level proof that `with_resizable(false)` removes the entire
    // resize hit-zone, independent of axis flags.  A minimal window
    // with empty content sits far from the canvas edges so we know any
    // movement came from the drag, not a clamp.
    let mut win = Window::new("inert", font(), Box::new(FlexColumn::new().with_panel_bg()))
        .with_bounds(Rect::new(100.0, 100.0, 300.0, 200.0))
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
        pos: on_edge,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    let moved = Point::new(400.0, 100.0);
    let _ = win.on_event(&Event::MouseMove { pos: moved });
    let _ = win.on_event(&Event::MouseUp {
        pos: moved,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(
        win.bounds(),
        before,
        "resizable(false) must keep bounds frozen against a drag"
    );
}

#[test]
fn resizable_axes_vertical_only_locks_east_edge() {
    // `with_resizable_axes(false, true)` → only N/S edges are live; the
    // east edge should be inert.  Using content-area Y (not title-bar
    // Y) avoids cross-talk with the title-bar drag handler.
    let mut win = Window::new(
        "v-only",
        font(),
        Box::new(FlexColumn::new().with_panel_bg()),
    )
    .with_bounds(Rect::new(100.0, 100.0, 300.0, 200.0))
    .with_resizable_axes(false, true);
    win.layout(Size::new(CANVAS_W, CANVAS_H));
    let before = win.bounds();
    // Widget-local y=100 sits in the content region (title bar occupies
    // y ∈ [172, 200] when height=200 and TITLE_H=28).
    let on_east = Point::new(299.0, 100.0);
    let _ = win.on_event(&Event::MouseMove { pos: on_east });
    let _ = win.on_event(&Event::MouseDown {
        pos: on_east,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    let _ = win.on_event(&Event::MouseMove {
        pos: Point::new(400.0, 100.0),
    });
    let _ = win.on_event(&Event::MouseUp {
        pos: Point::new(400.0, 100.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(
        win.bounds(),
        before,
        "E edge must be inert when resizable_h=false"
    );
}
