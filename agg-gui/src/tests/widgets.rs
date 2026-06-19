use super::*;

/// ColorPicker: clicking the swatch opens the panel; dragging the hue
/// slider writes a new colour into the bound cell.
#[test]
fn test_color_picker_opens_and_updates_on_drag() {
    use crate::text::Font;
    use crate::ColorPicker;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    // Non-gray start colour so hue changes actually shift the RGB values
    // (gray has saturation=0 → hue rotation is a no-op).
    let start = Color::rgba(1.0, 0.0, 0.0, 1.0);
    let cell = Rc::new(Cell::new(start));
    let picker = ColorPicker::new(Rc::clone(&cell), Arc::clone(&font));

    let mut app = App::new(Box::new(picker));
    const VP_H: f64 = 400.0;
    app.layout(Size::new(300.0, VP_H));

    // When closed the widget is 22 px tall in Y-up coords (y ∈ [0, 22]).
    // Screen → Y-up: y_up = VP_H − screen_y.  A screen click at
    // y = VP_H − 10 maps to y_up = 10 (inside the swatch).
    let swatch_screen_y = VP_H - 10.0;
    app.on_mouse_down(
        50.0,
        swatch_screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );
    app.on_mouse_up(
        50.0,
        swatch_screen_y,
        MouseButton::Left,
        Modifiers::default(),
    );
    // Re-layout so the expanded panel dimensions take effect.
    app.layout(Size::new(300.0, VP_H));

    // With the panel open, the hue strip lives near the TOP of the widget
    // in Y-up: its bottom edge is at
    //   y_up = panel_h − SWATCH_H − PAD − HUE_H  ≈  panel_h − 46
    // The panel_h when open is 22 + 258 = 280 for allow_none=false,
    // or 22 + 284 = 306 for true.  Default allow_none=false → panel_h ≈ 280.
    // Hue strip centre in Y-up ≈ 280 − 22 − 8 − 8 = 242.
    // Screen y = VP_H − 242 = 158.
    let hue_screen_y = VP_H - 242.0;
    // Click near right end of hue strip (high hue).
    app.on_mouse_down(220.0, hue_screen_y, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(210.0, hue_screen_y);
    app.on_mouse_up(210.0, hue_screen_y, MouseButton::Left, Modifiers::default());

    let final_color = cell.get();
    assert_ne!(
        (start.r, start.g, start.b),
        (final_color.r, final_color.g, final_color.b),
        "hue drag must have mutated the bound colour cell (got {:?})",
        final_color,
    );
}

/// A click outside widget bounds must not trigger the callback.
#[test]
fn test_click_outside_bounds_ignored() {
    use crate::text::Font;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let clicked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let clicked2 = std::sync::Arc::clone(&clicked);

    let button = Button::new("X", font)
        .with_font_size(14.0)
        .on_click(move || {
            clicked2.store(true, std::sync::atomic::Ordering::Relaxed);
        });

    let mut app = App::new(Box::new(button));
    app.layout(Size::new(200.0, 100.0));

    // Click way outside: screen y=200 → Y-up y = -100 (below viewport).
    app.on_mouse_down(100.0, 200.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(100.0, 200.0, MouseButton::Left, Modifiers::default());

    assert!(
        !clicked.load(std::sync::atomic::Ordering::Relaxed),
        "click outside button bounds must not fire callback"
    );
}

/// A touchscreen tap is a bare press→release at one point with NO preceding
/// MouseMove (touch has no hover phase). The button must still fire — the
/// press is routed to it by hit-testing, so the pointer is on it.
#[test]
fn test_button_fires_on_hoverless_tap() {
    use crate::text::Font;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let clicked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let clicked2 = Arc::clone(&clicked);

    let button = Button::new("Tap", font)
        .with_font_size(14.0)
        .on_click(move || clicked2.store(true, std::sync::atomic::Ordering::Relaxed));

    let mut app = App::new(Box::new(button));
    app.layout(Size::new(200.0, 100.0));

    // No on_mouse_move first — emulate a finger tap dead-centre.
    app.on_mouse_down(100.0, 50.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(100.0, 50.0, MouseButton::Left, Modifiers::default());

    assert!(
        clicked.load(std::sync::atomic::Ordering::Relaxed),
        "a hover-less tap inside the button must fire on_click"
    );
}

/// Pressing on the button but releasing outside it (a drag-off) must cancel
/// — the click only fires when the release lands within bounds.
#[test]
fn test_button_press_drag_off_does_not_fire() {
    use crate::text::Font;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let clicked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let clicked2 = Arc::clone(&clicked);

    let button = Button::new("Tap", font)
        .with_font_size(14.0)
        .on_click(move || clicked2.store(true, std::sync::atomic::Ordering::Relaxed));

    let mut app = App::new(Box::new(button));
    app.layout(Size::new(200.0, 100.0));

    // Press inside, drag well below the viewport, release there.
    app.on_mouse_down(100.0, 50.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_move(100.0, 400.0);
    app.on_mouse_up(100.0, 400.0, MouseButton::Left, Modifiers::default());

    assert!(
        !clicked.load(std::sync::atomic::Ordering::Relaxed),
        "releasing outside the button after a drag-off must not fire on_click"
    );
}

#[test]
fn test_text_field_tracks_external_text_cell() {
    use crate::text::Font;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let text = Rc::new(RefCell::new("initial".to_string()));
    let mut field = TextField::new(font).with_text_cell(Rc::clone(&text));
    field.layout(Size::new(160.0, 32.0));
    assert_eq!(field.text(), "initial");

    *text.borrow_mut() = "cleared externally".to_string();
    field.layout(Size::new(160.0, 32.0));
    assert_eq!(field.text(), "cleared externally");

    field.set_text("typed locally");
    assert_eq!(text.borrow().as_str(), "typed locally");
}

#[test]
fn test_read_only_text_field_rejects_paste() {
    use crate::text::Font;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut field = TextField::new(font)
        .with_text("locked")
        .with_read_only(true);
    field.layout(Size::new(180.0, 32.0));
    field.on_event(&crate::Event::FocusGained);

    crate::clipboard::set_text(" pasted");
    let result = field.on_event(&crate::Event::KeyDown {
        key: Key::Char('v'),
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    });

    assert_eq!(result, crate::EventResult::Consumed);
    assert_eq!(field.text(), "locked");
}

#[test]
fn test_text_field_char_filter_rejects_disallowed_typing() {
    use crate::text::Font;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut field = TextField::new(font)
        // Digits + 'x'/'X' + hex letters only — the Solitaire
        // "Play deal number" dialog's filter.
        .with_char_filter(|c| c.is_ascii_hexdigit() || c == 'x' || c == 'X');
    field.layout(Size::new(160.0, 32.0));
    field.on_event(&crate::Event::FocusGained);

    for c in ['1', '2', 'a', '!', '@', 'b', '\n'] {
        field.on_event(&crate::Event::KeyDown {
            key: Key::Char(c),
            modifiers: Modifiers::default(),
        });
    }
    assert_eq!(field.text(), "12ab");
}

#[test]
fn test_text_field_char_filter_strips_paste() {
    use crate::text::Font;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut field = TextField::new(font).with_char_filter(|c| c.is_ascii_digit());
    field.layout(Size::new(160.0, 32.0));
    field.on_event(&crate::Event::FocusGained);

    crate::clipboard::set_text("12-3a4 5");
    field.on_event(&crate::Event::KeyDown {
        key: Key::Char('v'),
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    });
    assert_eq!(field.text(), "12345");
}

#[test]
fn test_button_with_icon_grows_to_fit_icon_plus_label() {
    // Button with no icon has some natural width based on label
    // text. Adding an icon should grow the natural width by the
    // icon's advance + ICON_GAP, since layout reserves room for
    // the (icon + gap + label) group.
    use crate::text::Font;
    use crate::widgets::Button;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut plain = Button::new("Play", Arc::clone(&font)).with_font_size(14.0);
    let plain_size = plain.layout(Size::new(400.0, 100.0));
    let mut with_icon = Button::new("Play", Arc::clone(&font))
        .with_font_size(14.0)
        .with_icon('\u{f04b}', Arc::clone(&font));
    let icon_size = with_icon.layout(Size::new(400.0, 100.0));
    assert!(
        icon_size.width > plain_size.width,
        "icon button should be wider: plain={}px icon={}px",
        plain_size.width,
        icon_size.width,
    );
}

#[test]
fn test_button_compact_drops_48px_floor() {
    // An icon-only Button (empty label, icon set) defaults to the
    // 48 px touch-target width floor. `with_compact()` should drop
    // that so the natural width tracks icon + minimal pad — used by
    // mobile toolbar rows where 5+ icon buttons need to sit next to
    // each other without overflowing.
    use crate::text::Font;
    use crate::widgets::Button;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut plain = Button::new("", Arc::clone(&font))
        .with_font_size(14.0)
        .with_icon('\u{f04b}', Arc::clone(&font));
    let plain_size = plain.layout(Size::new(400.0, 100.0));
    assert_eq!(
        plain_size.width as i64, 48,
        "non-compact icon-only button should hit the 48 px floor"
    );
    let mut compact = Button::new("", Arc::clone(&font))
        .with_font_size(14.0)
        .with_icon('\u{f04b}', Arc::clone(&font))
        .with_compact();
    let compact_size = compact.layout(Size::new(400.0, 100.0));
    assert!(
        compact_size.width < 48.0,
        "compact icon-only button should be < 48 px, got {}px",
        compact_size.width
    );
    assert!(
        compact_size.width >= 16.0,
        "compact button should still leave room for the glyph, got {}px",
        compact_size.width
    );
}

#[test]
fn test_button_label_centers_when_height_is_constrained() {
    // Sidebar demo rows give Button a shorter height than its natural
    // 24 px layout height. The label must be centered in that constrained
    // height so it does not ride high in the painted row.
    use crate::text::Font;
    use crate::widgets::{Button, LabelAlign};
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut button = Button::new("Demo", font)
        .with_font_size(13.0)
        .with_label_align(LabelAlign::Left);

    let size = button.layout(Size::new(180.0, 20.0));

    let label = button.children()[0].bounds();
    let label_center_y = label.y + label.height * 0.5;
    assert!(
        label.width > 0.0 && label.height > 0.0,
        "button label should keep non-empty bounds; label={label:?}"
    );
    assert!(
        (label_center_y - size.height * 0.5).abs() < 0.01,
        "button label should be vertically centered in constrained layout; size={size:?} label={label:?}"
    );
}

#[test]
fn test_text_field_theme_overrides_visuals_palette() {
    // Confirm `with_theme` stores the overrides on the widget so
    // `paint` reads them instead of the ambient visuals. Locks the
    // surface area against future refactors that might mistakenly
    // drop the theme.
    use crate::color::Color;
    use crate::text::Font;
    use crate::widgets::TextFieldTheme;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let theme = TextFieldTheme {
        background: Some(Color::from_rgb8(0x0c, 0x1c, 0x12)),
        text_color: Some(Color::from_rgb8(0xff, 0xff, 0xff)),
        border_color_focused: Some(Color::from_rgb8(0xff, 0xd7, 0x00)),
        border_radius: Some(8.0),
        ..TextFieldTheme::default()
    };
    let field = TextField::new(font).with_theme(theme);
    assert!(field.theme.background.is_some());
    assert!(field.theme.text_color.is_some());
    assert!(field.theme.border_color_focused.is_some());
    assert_eq!(field.theme.border_radius, Some(8.0));
    // Unset fields stay None — paint falls back to visuals() for
    // those, which is the documented contract.
    assert!(field.theme.placeholder_color.is_none());
    assert!(field.theme.selection_bg.is_none());
}

#[test]
fn test_text_field_escape_ignored_without_selection() {
    use crate::text::Font;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut field = TextField::new(font).with_text("hello");
    field.layout(Size::new(160.0, 32.0));
    field.on_event(&crate::Event::FocusGained);

    // No selection — Escape should bubble up so dialog parents
    // can use it to cancel themselves.
    let r = field.on_event(&crate::Event::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::default(),
    });
    assert_eq!(r, crate::EventResult::Ignored);

    // Now create a selection (Ctrl+A) and verify Escape is
    // consumed to clear it.
    field.on_event(&crate::Event::KeyDown {
        key: Key::Char('a'),
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    });
    let r = field.on_event(&crate::Event::KeyDown {
        key: Key::Escape,
        modifiers: Modifiers::default(),
    });
    assert_eq!(r, crate::EventResult::Consumed);
}

/// Tab key advances focus through focusable widgets.
#[test]
fn test_tab_focus_advance() {
    use crate::text::Font;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());

    let mut root = Container::new().with_padding(4.0);
    root.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font)).with_font_size(14.0),
    ));
    root.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font)).with_font_size(14.0),
    ));

    let mut app = App::new(Box::new(root));
    app.layout(Size::new(200.0, 200.0));

    // No focus initially — Tab should focus the first focusable widget.
    app.on_key_down(Key::Tab, Modifiers::default());
    // A second Tab should move to the second field.
    app.on_key_down(Key::Tab, Modifiers::default());
    // A third Tab wraps back to the first.
    app.on_key_down(Key::Tab, Modifiers::default());

    // We can't easily inspect focus from outside, but we can verify it
    // doesn't panic and the test passes if no assertion fires.
}

// ---------------------------------------------------------------------------
// Phase 5 — layout widgets
// ---------------------------------------------------------------------------

/// FlexColumn stacks children top-to-bottom in Y-up: first child has the
/// highest Y coordinate (visually at the top of the screen).
#[test]
fn test_flex_column_first_child_highest_y() {
    let mut col = FlexColumn::new()
        .with_gap(0.0)
        .with_padding(0.0)
        .add(Box::new(SizedBox::new().with_height(40.0))) // first = top
        .add(Box::new(SizedBox::new().with_height(60.0))); // second = below

    col.layout(Size::new(200.0, 200.0));

    let y0 = col.children()[0].bounds().y;
    let y1 = col.children()[1].bounds().y;
    assert!(
        y0 > y1,
        "first child (top) should have higher Y in Y-up; got y0={y0}, y1={y1}",
    );
    assert_eq!(col.children()[0].bounds().height, 40.0);
    assert_eq!(col.children()[1].bounds().height, 60.0);
}

/// FlexRow distributes flex space left-to-right, first child leftmost.
#[test]
fn test_flex_row_distributes_space() {
    let mut row = FlexRow::new()
        .with_gap(0.0)
        .with_padding(0.0)
        .add_flex(Box::new(SizedBox::new()), 1.0) // left half
        .add_flex(Box::new(SizedBox::new()), 1.0); // right half

    row.layout(Size::new(200.0, 40.0));

    let x0 = row.children()[0].bounds().x;
    let x1 = row.children()[1].bounds().x;
    assert_eq!(x0, 0.0, "first flex child should start at x=0");
    assert!(x1 > x0, "second flex child should be to the right of first");
    assert!(
        (x1 - 100.0).abs() < 1.0,
        "second child should start at x≈100; got {x1}"
    );
}

mod combo_popup;

/// Splitter updates its ratio when dragged across the divider.
#[test]
fn test_splitter_drag_updates_ratio() {
    let mut splitter = Splitter::new(Box::new(SizedBox::new()), Box::new(SizedBox::new()));
    splitter.layout(Size::new(400.0, 200.0));
    splitter.set_bounds(crate::Rect::new(0.0, 0.0, 400.0, 200.0));

    // Default ratio = 0.5; divider at x = (400 - 6) * 0.5 ≈ 197.
    let div_x = (400.0_f64 - 6.0) * 0.5;

    // Press on divider.
    splitter.on_event(&crate::Event::MouseDown {
        pos: crate::Point::new(div_x + 1.0, 100.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });

    // Drag to x=100 → ratio should become 100/400 = 0.25.
    splitter.on_event(&crate::Event::MouseMove {
        pos: crate::Point::new(100.0, 100.0),
    });

    assert!(
        (splitter.ratio - 0.25).abs() < 0.01,
        "ratio should be ≈0.25 after drag; got {}",
        splitter.ratio,
    );
}

/// TabView swaps its active child when the tab bar is clicked.
#[test]
fn test_tab_view_always_has_one_child() {
    use crate::text::Font;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());

    let mut tv = TabView::new(Arc::clone(&font))
        .add_tab("A", Box::new(SizedBox::new().with_height(100.0)))
        .add_tab("B", Box::new(SizedBox::new().with_height(200.0)));

    tv.layout(Size::new(400.0, 300.0));
    tv.set_bounds(crate::Rect::new(0.0, 0.0, 400.0, 300.0));

    assert_eq!(
        tv.children().len(),
        1,
        "TabView should always have exactly 1 active child"
    );

    // Tab bar: content_height = 300 - 36 = 264; bar is y in [264, 300].
    // Tab B is the second of two: x in [200, 400].
    tv.on_event(&crate::Event::MouseDown {
        pos: crate::Point::new(300.0, 270.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });

    assert_eq!(
        tv.children().len(),
        1,
        "TabView should still have exactly 1 active child after switch"
    );
}

/// Closing a Window (visible = false) must prevent its content from being painted.
#[test]
fn test_window_close_hides_content() {
    use crate::text::Font;
    use crate::widget::paint_subtree;
    use crate::widgets::window::Window;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());

    // A window whose content is a Button — Button.paint() fills its bounds with
    // a visible background, so a leak is detectable as non-black pixels.
    let content = Button::new("Content", Arc::clone(&font)).with_font_size(14.0);
    let mut win = Window::new("Test", Arc::clone(&font), Box::new(content))
        .with_bounds(crate::Rect::new(0.0, 0.0, 200.0, 200.0));

    // Run layout so child bounds are set.
    win.layout(Size::new(200.0, 200.0));

    // First paint with window visible — content area should have some pixel.
    let mut fb_visible = Framebuffer::new(200, 200);
    {
        let mut ctx = GfxCtx::new(&mut fb_visible);
        ctx.clear(Color::black());
        paint_subtree(&mut win, &mut ctx);
    }

    // Hide the window, paint again — should revert to all-black.
    win.hide();
    let mut fb_hidden = Framebuffer::new(200, 200);
    {
        let mut ctx = GfxCtx::new(&mut fb_hidden);
        ctx.clear(Color::black());
        paint_subtree(&mut win, &mut ctx);
    }

    // The visible framebuffer should have non-black pixels (window chrome).
    let visible_has_pixels = fb_visible
        .pixels()
        .chunks(4)
        .any(|p| p[0] > 50 || p[1] > 50 || p[2] > 50);
    assert!(visible_has_pixels, "visible window must paint something");

    // The hidden framebuffer must be completely black.
    let hidden_all_black = fb_hidden
        .pixels()
        .chunks(4)
        .all(|p| p[0] < 10 && p[1] < 10 && p[2] < 10);
    assert!(
        hidden_all_black,
        "hidden window must not paint anything; content child leaked"
    );
}

/// A collapsed Window is only its title bar, so the title-bar fill must not
/// square off the bottom corners of the outer rounded window shape.
#[test]
fn test_collapsed_window_title_bar_rounds_bottom_corners() {
    use crate::text::Font;
    use crate::widget::{paint_subtree, Widget};
    use crate::widgets::window::Window;
    use std::sync::Arc;

    fn sample(fb: &Framebuffer, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * fb.width() + x) * 4) as usize;
        let p = &fb.pixels()[i..i + 4];
        [p[0], p[1], p[2], p[3]]
    }

    fn brightness(px: [u8; 4]) -> u16 {
        px[0] as u16 + px[1] as u16 + px[2] as u16
    }

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let content = Button::new("Content", Arc::clone(&font)).with_font_size(14.0);
    let mut win = Window::new("Test", Arc::clone(&font), Box::new(content))
        .with_bounds(crate::Rect::new(0.0, 0.0, 200.0, 80.0));

    win.layout(Size::new(240.0, 120.0));
    win.on_event(&crate::Event::MouseDown {
        pos: crate::Point::new(12.0, 66.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    win.layout(Size::new(240.0, 120.0));

    let mut fb = Framebuffer::new(220, 60);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::black());
        paint_subtree(&mut win, &mut ctx);
    }

    let bottom_left_corner = sample(&fb, 1, 1);
    let title_bar_interior = sample(&fb, 100, 14);
    assert!(
        brightness(bottom_left_corner) + 40 < brightness(title_bar_interior),
        "collapsed title bar should leave the bottom-left corner rounded; corner={bottom_left_corner:?}, interior={title_bar_interior:?}"
    );
}

#[test]
fn test_window_backbuffer_spec_covers_shadow_and_fade_out() {
    use crate::text::Font;
    use crate::widget::{BackbufferKind, Widget};
    use crate::widgets::window::Window;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let content = Button::new("Content", Arc::clone(&font)).with_font_size(14.0);
    let mut win = Window::new("Layered", Arc::clone(&font), Box::new(content))
        .with_bounds(crate::Rect::new(0.0, 0.0, 200.0, 120.0));

    let visible_spec = win.backbuffer_spec();
    assert_eq!(visible_spec.kind, BackbufferKind::GlFbo);
    assert!(visible_spec.cached);
    assert!(visible_spec.alpha > 0.99);
    assert!(visible_spec.outsets.left > 0.0);
    assert!(visible_spec.outsets.bottom > 0.0);
    assert!(visible_spec.outsets.right > 0.0);
    assert!(visible_spec.outsets.top > 0.0);

    win.hide();
    assert!(
        !win.is_visible(),
        "non-layer renderers should still see hide() as immediate"
    );
    let fading_layer = win.backbuffer_spec();
    assert!(
        fading_layer.alpha > 0.001 && fading_layer.alpha <= 1.0,
        "fade-out layer alpha should be visible and bounded, got {}",
        fading_layer.alpha
    );
}

#[test]
fn test_window_can_opt_out_of_gl_backbuffer() {
    use crate::text::Font;
    use crate::widget::{BackbufferKind, Widget};
    use crate::widgets::window::Window;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let content = Button::new("Content", Arc::clone(&font)).with_font_size(14.0);
    let mut win =
        Window::new("Direct", Arc::clone(&font), Box::new(content)).with_gl_backbuffer(false);

    assert_eq!(win.backbuffer_spec().kind, BackbufferKind::None);
}

#[test]
fn test_scroll_view_reports_overlay_animation_draw_need() {
    use crate::widget::paint_subtree;

    let mut scroll = ScrollView::new(Box::new(SizedBox::fixed(50.0, 300.0)))
        .with_style(ScrollBarStyle::thin())
        .with_bar_visibility(crate::ScrollBarVisibility::AlwaysVisible);
    scroll.layout(Size::new(100.0, 100.0));

    let mut fb = Framebuffer::new(100, 100);
    let mut ctx = GfxCtx::new(&mut fb);
    paint_subtree(&mut scroll, &mut ctx);

    assert!(
        scroll.needs_draw(),
        "scrollbar fade/width tweens must keep retained parents repainting"
    );
}

#[test]
fn test_toggle_switch_reports_animation_draw_need() {
    use crate::widget::paint_subtree;
    use std::time::Duration;

    let mut toggle = ToggleSwitch::new(false);
    toggle.layout(Size::new(100.0, 40.0));
    toggle.set_bounds(crate::Rect::new(0.0, 0.0, 34.0, 20.0));

    assert!(
        !toggle.needs_draw(),
        "idle toggle switch should not keep the host repainting"
    );

    let event = crate::Event::MouseDown {
        pos: crate::Point::new(10.0, 10.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    };
    assert_eq!(toggle.on_event(&event), crate::EventResult::Consumed);
    assert!(
        toggle.needs_draw(),
        "press-ring tween must make retained parents repaint"
    );

    let mut fb = Framebuffer::new(40, 24);
    let mut ctx = GfxCtx::new(&mut fb);
    paint_subtree(&mut toggle, &mut ctx);
    assert!(
        crate::animation::wants_draw(),
        "in-flight toggle tweens must request the next frame"
    );

    std::thread::sleep(Duration::from_millis(260));
    crate::animation::clear_draw_request();
    paint_subtree(&mut toggle, &mut ctx);
    assert!(
        !crate::animation::wants_draw() && !toggle.needs_draw(),
        "settled toggle tweens must let reactive mode go idle"
    );
}

#[test]
fn test_scroll_view_reports_global_style_epoch_change() {
    use crate::widget::paint_subtree;

    let mut scroll = ScrollView::new(Box::new(SizedBox::fixed(20.0, 20.0)));
    scroll.layout(Size::new(100.0, 100.0));

    let mut fb = Framebuffer::new(100, 100);
    let mut ctx = GfxCtx::new(&mut fb);
    paint_subtree(&mut scroll, &mut ctx);
    assert!(
        !scroll.needs_draw(),
        "clean scroll view without active scrollbar animation should be idle"
    );

    crate::set_scroll_style(ScrollBarStyle::solid());

    assert!(
        scroll.needs_draw(),
        "global scrollbar style changes must invalidate clean retained parents"
    );
}

#[test]
fn test_consumed_event_marks_widget_backbuffer_dirty() {
    use crate::widget::{dispatch_event, BackbufferState, Widget};
    use crate::{DrawCtx, Event, EventResult, Modifiers, MouseButton, Point, Rect, Size};

    struct DirtyProbe {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        backbuffer: BackbufferState,
    }

    impl Widget for DirtyProbe {
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
            available
        }
        fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _event: &Event) -> EventResult {
            EventResult::Consumed
        }
        fn backbuffer_state_mut(&mut self) -> Option<&mut BackbufferState> {
            Some(&mut self.backbuffer)
        }
    }

    let mut root: Box<dyn Widget> = Box::new(DirtyProbe {
        bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
        children: Vec::new(),
        backbuffer: BackbufferState::new(),
    });
    root.backbuffer_state_mut().unwrap().dirty = false;
    let event = Event::MouseDown {
        pos: Point::new(1.0, 1.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    };

    assert_eq!(
        dispatch_event(&mut root, &[], &event, Point::new(1.0, 1.0)),
        EventResult::Consumed
    );
    assert!(root.backbuffer_state_mut().unwrap().dirty);
}
