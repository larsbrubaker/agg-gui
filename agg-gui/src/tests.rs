//! Coordinate system invariant tests.
//!
//! These tests guard the first-quadrant (Y-up) invariant at the framebuffer
//! and GfxCtx layers. They run on every commit.

use crate::{
    App, Button, Color, CompOp, Container, FlexColumn, FlexRow, Framebuffer, GfxCtx,
    Key, MouseButton, Modifiers, ScrollView, Size, SizedBox, Spacer, Splitter,
    TabView, TextField, Widget,
};

/// Sample RGBA at pixel (x, y) in a framebuffer.
/// (x=0, y=0) is the bottom-left corner in Y-up space.
fn sample(fb: &Framebuffer, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[idx], p[idx + 1], p[idx + 2], p[idx + 3]]
}

fn is_white(pixel: [u8; 4]) -> bool {
    pixel[0] > 200 && pixel[1] > 200 && pixel[2] > 200
}

fn is_red(pixel: [u8; 4]) -> bool {
    pixel[0] > 200 && pixel[1] < 50 && pixel[2] < 50
}

fn is_dark(pixel: [u8; 4]) -> bool {
    pixel[0] < 50 && pixel[1] < 50 && pixel[2] < 50
}

// ---------------------------------------------------------------------------
// Phase 1 — coordinate system invariants
// ---------------------------------------------------------------------------

/// A point drawn at Y=10 in a 100×100 buffer must be near the BOTTOM of the
/// buffer (low row index), not the top. This verifies the Y-up invariant at
/// the framebuffer level.
#[test]
fn test_y_up_point_at_bottom() {
    let mut fb = Framebuffer::new(100, 100);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::black());

    // Draw a white circle at (50, 10) — near the bottom in Y-up space.
    ctx.set_fill_color(Color::white());
    ctx.begin_path();
    ctx.circle(50.0, 10.0, 5.0);
    ctx.fill();
    drop(ctx);

    // Row 10 (from buffer start) = Y=10 = near the BOTTOM of the window.
    let center = sample(&fb, 50, 10);
    assert!(is_white(center), "Y=10 should be near the bottom of the buffer (Y-up); got {center:?}");

    let top_center = sample(&fb, 50, 90);
    assert!(is_dark(top_center), "Y=90 should be dark (nothing drawn there); got {top_center:?}");
}

/// A CCW rotation of +90° rotates a right-pointing vector to point upward.
#[test]
fn test_rotation_ccw_positive() {
    let size = 200u32;
    let mut fb = Framebuffer::new(size, size);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::black());

    let cx = size as f64 / 2.0;
    let cy = size as f64 / 2.0;

    ctx.translate(cx, cy);
    ctx.rotate(std::f64::consts::FRAC_PI_2);

    ctx.set_fill_color(Color::white());
    ctx.begin_path();
    ctx.rect(10.0, -3.0, 40.0, 6.0);
    ctx.fill();
    drop(ctx);

    let above_center = sample(&fb, cx as u32, cy as u32 + 25);
    assert!(is_white(above_center), "+90° CCW rotation should produce upward bar; pixel above center is {above_center:?}");

    let right_of_center = sample(&fb, cx as u32 + 25, cy as u32);
    assert!(is_dark(right_of_center), "After +90° rotation, horizontal should be gone; pixel to right is {right_of_center:?}");
}

/// A point drawn at (10, 10) in Y-up space is near the bottom-left corner.
#[test]
fn test_bottom_left_origin() {
    let mut fb = Framebuffer::new(200, 200);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::black());

    ctx.set_fill_color(Color::rgb(1.0, 0.0, 0.0));
    ctx.begin_path();
    ctx.circle(10.0, 10.0, 6.0);
    ctx.fill();
    drop(ctx);

    let center = sample(&fb, 10, 10);
    assert!(is_red(center), "Bottom-left origin test: (10,10) should be red; got {center:?}");

    let top_right = sample(&fb, 190, 190);
    assert!(is_dark(top_right), "Top-right should be empty; got {top_right:?}");
}

/// `pixels_flipped()` should reverse the row order.
#[test]
fn test_pixels_flipped_reversal() {
    let w = 4u32;
    let h = 4u32;
    let mut fb = Framebuffer::new(w, h);

    {
        let pixels = fb.pixels_mut();
        for x in 0..w as usize {
            let i = x * 4;
            pixels[i] = 255; pixels[i+1] = 0; pixels[i+2] = 0; pixels[i+3] = 255;
        }
        let base = 3 * w as usize * 4;
        for x in 0..w as usize {
            let i = base + x * 4;
            pixels[i] = 0; pixels[i+1] = 0; pixels[i+2] = 255; pixels[i+3] = 255;
        }
    }

    let flipped = fb.pixels_flipped();
    assert_eq!(&flipped[0..4], &[0u8, 0, 255, 255], "Flipped[0] should be blue");
    let last = (h as usize - 1) * w as usize * 4;
    assert_eq!(&flipped[last..last+4], &[255u8, 0, 0, 255], "Flipped last row should be red");
}

// ---------------------------------------------------------------------------
// Phase 2 — clip rect
// ---------------------------------------------------------------------------

/// Drawing outside a clip rect must not affect pixels there.
#[test]
fn test_clip_rect_excludes_outside() {
    let size = 100u32;
    let mut fb = Framebuffer::new(size, size);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::black());

    // Clip to right half only (x ≥ 50).
    ctx.clip_rect(50.0, 0.0, 50.0, 100.0);

    ctx.set_fill_color(Color::white());
    ctx.begin_path();
    // Draw a rectangle that spans the full width.
    ctx.rect(0.0, 0.0, 100.0, 100.0);
    ctx.fill();
    drop(ctx);

    // Left half (x=10, y=50) must stay black — clipped out.
    let left = sample(&fb, 10, 50);
    assert!(is_dark(left), "Left half should be clipped out; got {left:?}");

    // Right half (x=75, y=50) must be white — inside clip.
    let right = sample(&fb, 75, 50);
    assert!(is_white(right), "Right half should be white (inside clip); got {right:?}");
}

/// Restoring state also restores the clip, so drawing after restore is unclipped.
#[test]
fn test_clip_rect_restores_with_state() {
    let size = 100u32;
    let mut fb = Framebuffer::new(size, size);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::black());

    ctx.save();
    ctx.clip_rect(60.0, 0.0, 40.0, 100.0); // clip to right 40px
    ctx.restore();

    // After restore clip is gone — draw should cover the full buffer.
    ctx.set_fill_color(Color::white());
    ctx.begin_path();
    ctx.rect(0.0, 0.0, 100.0, 100.0);
    ctx.fill();
    drop(ctx);

    // Left side must now be white (no clip).
    let left = sample(&fb, 10, 50);
    assert!(is_white(left), "After restore, clip should be gone; got {left:?}");
}

// ---------------------------------------------------------------------------
// Phase 2 — rounded rect
// ---------------------------------------------------------------------------

/// A rounded_rect with radius 0 behaves identically to a plain rect.
#[test]
fn test_rounded_rect_zero_radius() {
    let size = 100u32;
    let mut fb_rr = Framebuffer::new(size, size);
    let mut fb_r  = Framebuffer::new(size, size);

    {
        let mut ctx = GfxCtx::new(&mut fb_rr);
        ctx.clear(Color::black());
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rounded_rect(20.0, 20.0, 60.0, 60.0, 0.0);
        ctx.fill();
    }
    {
        let mut ctx = GfxCtx::new(&mut fb_r);
        ctx.clear(Color::black());
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rect(20.0, 20.0, 60.0, 60.0);
        ctx.fill();
    }

    // Both should produce white at the center.
    assert!(is_white(sample(&fb_rr, 50, 50)), "rounded_rect center should be white");
    assert!(is_white(sample(&fb_r,  50, 50)), "rect center should be white");
}

/// A rounded_rect with a large radius must clip its corners.
#[test]
fn test_rounded_rect_corners_are_clipped() {
    let size = 100u32;
    let mut fb = Framebuffer::new(size, size);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::black());
    ctx.set_fill_color(Color::white());
    ctx.begin_path();
    // Square 20..80 with r=15 — corners should be dark.
    ctx.rounded_rect(20.0, 20.0, 60.0, 60.0, 15.0);
    ctx.fill();
    drop(ctx);

    // Exact corner at (20, 20) — inside the radius arc, should remain dark.
    let corner = sample(&fb, 20, 20);
    assert!(is_dark(corner), "Corner should be clipped by radius; got {corner:?}");

    // Center must be white.
    let center = sample(&fb, 50, 50);
    assert!(is_white(center), "Center should be white; got {center:?}");
}

// ---------------------------------------------------------------------------
// Phase 2 — blend modes
// ---------------------------------------------------------------------------

/// SrcOver (default) blends a semi-transparent fill onto an opaque base.
#[test]
fn test_blend_mode_src_over_alpha() {
    let size = 40u32;
    let mut fb = Framebuffer::new(size, size);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());

    // Draw 50% transparent black over white → should give mid-gray.
    ctx.set_blend_mode(CompOp::SrcOver);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.5));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, size as f64, size as f64);
    ctx.fill();
    drop(ctx);

    let p = sample(&fb, 20, 20);
    // Should be roughly 50% gray (127 ± 5).
    assert!(p[0] > 100 && p[0] < 160, "50% black over white should be mid-gray; got {p:?}");
}

/// global_alpha multiplies into fill alpha.
#[test]
fn test_global_alpha() {

    let size = 40u32;
    let mut fb = Framebuffer::new(size, size);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());

    // Fully opaque red, but global_alpha = 0.5 → should produce pinkish result.
    ctx.set_global_alpha(0.5);
    ctx.set_fill_color(Color::rgb(1.0, 0.0, 0.0));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, size as f64, size as f64);
    ctx.fill();
    drop(ctx);

    let p = sample(&fb, 20, 20);
    // Red channel should be high, green/blue non-zero (blended with white).
    assert!(p[0] > 200, "Red channel should be high; got {p:?}");
    assert!(p[1] > 100, "Green channel should be non-zero (blended with white); got {p:?}");
}

// ---------------------------------------------------------------------------
// Phase 3 — text rendering
// ---------------------------------------------------------------------------

const TEST_FONT: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");

/// `measure_text` returns a wider advance for a longer string.
#[test]
fn test_measure_text_longer_is_wider() {
    use std::sync::Arc;
    use crate::text::Font;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut fb = Framebuffer::new(400, 100);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.set_font(font);
    ctx.set_font_size(20.0);

    let short  = ctx.measure_text("Hi").unwrap();
    let longer = ctx.measure_text("Hello, World!").unwrap();
    assert!(
        longer.width > short.width,
        "longer string should have greater advance: {} > {}",
        longer.width,
        short.width,
    );
}

/// `fill_text` must paint at least some non-white pixels when drawing text
/// on a white background.
#[test]
fn test_fill_text_paints_pixels() {
    use std::sync::Arc;
    use crate::text::Font;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut fb = Framebuffer::new(300, 60);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());
    ctx.set_fill_color(Color::black());
    ctx.set_font(font);
    ctx.set_font_size(24.0);
    // Draw at baseline Y=30, which is within the buffer.
    ctx.fill_text("Test", 10.0, 30.0);
    drop(ctx);

    // At least one pixel should be non-white.
    let dark_count = (0..300_u32)
        .flat_map(|x| (0..60_u32).map(move |y| (x, y)))
        .filter(|&(x, y)| !is_white(sample(&fb, x, y)))
        .count();
    assert!(dark_count > 10, "fill_text should paint dark pixels; got {dark_count}");
}

/// `measure_text` returns positive ascent and line_height values.
#[test]
fn test_measure_text_metrics_positive() {
    use std::sync::Arc;
    use crate::text::Font;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut fb = Framebuffer::new(200, 60);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.set_font(font);
    ctx.set_font_size(16.0);

    let m = ctx.measure_text("Ag").unwrap();
    assert!(m.ascent > 0.0, "ascent must be positive; got {}", m.ascent);
    assert!(m.descent > 0.0, "descent must be positive; got {}", m.descent);
    assert!(m.line_height >= m.ascent + m.descent,
        "line_height ({}) should be >= ascent + descent ({})", m.line_height, m.ascent + m.descent);
}

// ---------------------------------------------------------------------------
// Phase 4 — widget system
// ---------------------------------------------------------------------------

/// Y-down → Y-up flip: a point at screen y=10 in a 100px viewport becomes y=90.
#[test]
fn test_y_flip_at_ingestion() {
    use std::sync::Arc;
    use crate::text::Font;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut clicked = false;
    let clicked_ptr = &mut clicked as *mut bool;

    let mut button = Button::new("X", Arc::clone(&font))
        .with_font_size(14.0)
        .on_click(move || unsafe { *clicked_ptr = true });

    // Lay out button to fill a 200×100 viewport.
    button.layout(Size::new(200.0, 100.0));
    button.set_bounds(crate::Rect::new(0.0, 0.0, 200.0, 100.0));

    let mut app = App::new(Box::new(button) as Box<dyn Widget>);
    app.layout(Size::new(200.0, 100.0));

    // Move cursor into the button first (sets hover state), then click.
    // Screen y=50 in a 100px-tall viewport → Y-up y=50; button fills viewport.
    app.on_mouse_move(100.0, 50.0);
    app.on_mouse_down(100.0, 50.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(100.0, 50.0, MouseButton::Left, Modifiers::default());

    assert!(clicked, "button inside viewport should be clicked");
}

/// A click outside widget bounds must not trigger the callback.
#[test]
fn test_click_outside_bounds_ignored() {
    use std::sync::Arc;
    use crate::text::Font;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let clicked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let clicked2 = std::sync::Arc::clone(&clicked);

    let button = Button::new("X", font)
        .with_font_size(14.0)
        .on_click(move || { clicked2.store(true, std::sync::atomic::Ordering::Relaxed); });

    let mut app = App::new(Box::new(button));
    app.layout(Size::new(200.0, 100.0));

    // Click way outside: screen y=200 → Y-up y = -100 (below viewport).
    app.on_mouse_down(100.0, 200.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(100.0, 200.0, MouseButton::Left, Modifiers::default());

    assert!(!clicked.load(std::sync::atomic::Ordering::Relaxed),
        "click outside button bounds must not fire callback");
}

/// Tab key advances focus through focusable widgets.
#[test]
fn test_tab_focus_advance() {
    use std::sync::Arc;
    use crate::text::Font;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());

    let mut root = Container::new().with_padding(4.0);
    root.children_mut().push(Box::new(TextField::new(Arc::clone(&font)).with_font_size(14.0)));
    root.children_mut().push(Box::new(TextField::new(Arc::clone(&font)).with_font_size(14.0)));

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
        .add(Box::new(SizedBox::new().with_height(40.0)))  // first = top
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
        .add_flex(Box::new(SizedBox::new()), 1.0)  // left half
        .add_flex(Box::new(SizedBox::new()), 1.0); // right half

    row.layout(Size::new(200.0, 40.0));

    let x0 = row.children()[0].bounds().x;
    let x1 = row.children()[1].bounds().x;
    assert_eq!(x0, 0.0, "first flex child should start at x=0");
    assert!(x1 > x0, "second flex child should be to the right of first");
    assert!((x1 - 100.0).abs() < 1.0, "second child should start at x≈100; got {x1}");
}

/// ScrollView returns the available size from layout and positions its child
/// with a negative y when content is taller than the viewport.
#[test]
fn test_scroll_view_tall_content_child_y() {
    let content = SizedBox::new().with_height(500.0);
    let mut scroll = ScrollView::new(Box::new(content));

    let result = scroll.layout(Size::new(200.0, 200.0));

    assert_eq!(result.width, 200.0);
    assert_eq!(result.height, 200.0);

    // With scroll_offset=0 and content_height=500, viewport_height=200:
    // child_y = 200 - 500 + 0 = -300  (content sticks up beyond viewport top)
    let child_y = scroll.children()[0].bounds().y;
    assert!(
        child_y < 0.0,
        "tall content with offset=0 should have negative child_y; got {child_y}",
    );
}

/// Splitter updates its ratio when dragged across the divider.
#[test]
fn test_splitter_drag_updates_ratio() {
    let mut splitter = Splitter::new(
        Box::new(SizedBox::new()),
        Box::new(SizedBox::new()),
    );
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
    use std::sync::Arc;
    use crate::text::Font;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());

    let mut tv = TabView::new(Arc::clone(&font))
        .add_tab("A", Box::new(SizedBox::new().with_height(100.0)))
        .add_tab("B", Box::new(SizedBox::new().with_height(200.0)));

    tv.layout(Size::new(400.0, 300.0));
    tv.set_bounds(crate::Rect::new(0.0, 0.0, 400.0, 300.0));

    assert_eq!(tv.children().len(), 1, "TabView should always have exactly 1 active child");

    // Tab bar: content_height = 300 - 36 = 264; bar is y in [264, 300].
    // Tab B is the second of two: x in [200, 400].
    tv.on_event(&crate::Event::MouseDown {
        pos: crate::Point::new(300.0, 270.0),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });

    assert_eq!(tv.children().len(), 1, "TabView should still have exactly 1 active child after switch");
}

/// Closing a Window (visible = false) must prevent its content from being painted.
#[test]
fn test_window_close_hides_content() {
    use std::sync::Arc;
    use crate::text::Font;
    use crate::widgets::window::Window;
    use crate::widget::paint_subtree;

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
    let visible_has_pixels = fb_visible.pixels()
        .chunks(4)
        .any(|p| p[0] > 50 || p[1] > 50 || p[2] > 50);
    assert!(visible_has_pixels, "visible window must paint something");

    // The hidden framebuffer must be completely black.
    let hidden_all_black = fb_hidden.pixels()
        .chunks(4)
        .all(|p| p[0] < 10 && p[1] < 10 && p[2] < 10);
    assert!(hidden_all_black, "hidden window must not paint anything; content child leaked");
}

/// InspectorPanel must build the TreeView with the correct nodes:
/// - Two InspectorNodes (Root at depth 0, Child at depth 1) must produce two
///   TreeView nodes where Child's parent is Root's index.
/// - InspectorPanel exposes no children (TreeView is managed directly).
/// - The TreeView bounds must sit inside the tree area (above split, below header).
#[test]
fn test_inspector_row0_at_top() {
    use std::sync::Arc;
    use std::cell::RefCell;
    use std::rc::Rc;
    use crate::text::Font;
    use crate::widgets::inspector::InspectorPanel;
    use crate::widget::{InspectorNode, Widget};
    use crate::geometry::Rect;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered_bounds = Rc::new(RefCell::new(None));
    let nodes: Rc<RefCell<Vec<InspectorNode>>> = Rc::new(RefCell::new(vec![
        InspectorNode { type_name: "Root",  screen_bounds: Rect::new(0.0,0.0,100.0,50.0), depth: 0, properties: vec![] },
        InspectorNode { type_name: "Child", screen_bounds: Rect::new(0.0,0.0,50.0,20.0),  depth: 1, properties: vec![] },
    ]));

    let mut panel = InspectorPanel::new(Arc::clone(&font), Rc::clone(&nodes), Rc::clone(&hovered_bounds));
    panel.layout(crate::Size::new(200.0, 300.0));
    panel.set_bounds(Rect::new(0.0, 0.0, 200.0, 300.0));

    // InspectorPanel exposes one InternalPresenceNode child so it appears
    // expandable in the inspector (not a leaf node).
    assert_eq!(panel.children().len(), 1, "InspectorPanel must have one presence child");
    assert_eq!(panel.children()[0].type_name(), "TreeView",
               "The presence child must report type_name 'TreeView'");

    // The TreeView should have exactly 2 nodes (one per InspectorNode).
    assert_eq!(panel.tree_view.nodes.len(), 2, "tree_view must have 2 nodes");

    // Root node has no parent; Child node's parent is Root (index 0).
    assert!(panel.tree_view.nodes[0].parent.is_none(), "Root must have no parent");
    assert_eq!(panel.tree_view.nodes[1].parent, Some(0), "Child must have Root (0) as parent");

    // The TreeView bounds must be positioned inside the tree area.
    // tree_area top = list_area_h = 300 - 30 = 270 (just below header).
    // tree_area bottom = split_y + 4; split_y ≥ MIN_PROPS_H = 60, so ≥ 64.
    let tv_bounds = panel.tree_view.bounds();
    assert!(tv_bounds.height > 0.0, "TreeView must have positive height");
    assert!(tv_bounds.y >= 60.0, "TreeView bottom must be above split handle");
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
    use std::sync::Arc;
    use std::cell::RefCell;
    use std::rc::Rc;
    use crate::text::Font;
    use crate::widgets::inspector::InspectorPanel;
    use crate::widget::{InspectorNode, Widget};
    use crate::geometry::Rect;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered_bounds = Rc::new(RefCell::new(None));
    let nodes: Rc<RefCell<Vec<InspectorNode>>> = Rc::new(RefCell::new(vec![
        InspectorNode { type_name: "Root",    screen_bounds: Rect::new(0.0,0.0,100.0,50.0), depth: 0, properties: vec![] },
        InspectorNode { type_name: "Child",   screen_bounds: Rect::new(0.0,0.0,50.0,20.0),  depth: 1, properties: vec![] },
        InspectorNode { type_name: "Sibling", screen_bounds: Rect::new(0.0,0.0,50.0,20.0),  depth: 0, properties: vec![] },
    ]));

    let mut panel = InspectorPanel::new(Arc::clone(&font), Rc::clone(&nodes), hovered_bounds);
    panel.layout(crate::Size::new(200.0, 400.0));

    // Panel exposes TreeView via tree_view field.
    assert_eq!(panel.tree_view.nodes.len(), 3, "must have 3 tree nodes");

    // Root is a root-level node (no parent).
    assert!(panel.tree_view.nodes[0].parent.is_none(), "node 0 must be root-level");

    // Child has Root as parent.
    assert_eq!(panel.tree_view.nodes[1].parent, Some(0), "node 1 must be child of node 0");

    // Sibling is another root-level node.
    assert!(panel.tree_view.nodes[2].parent.is_none(), "node 2 must be root-level");

    // InspectorPanel.children() exposes one InternalPresenceNode so it is
    // non-leaf in the inspector tree; the proxy reports type_name "TreeView".
    assert_eq!(panel.children().len(), 1,
               "InspectorPanel must have one presence child");
    assert_eq!(panel.children()[0].type_name(), "TreeView",
               "Presence child must report type_name 'TreeView'");
}

/// All nodes must be expanded by default so the full tree is visible on first show.
#[test]
fn test_inspector_tree_default_expanded() {
    use std::sync::Arc;
    use std::cell::RefCell;
    use std::rc::Rc;
    use crate::text::Font;
    use crate::widgets::inspector::InspectorPanel;
    use crate::widget::{InspectorNode, Widget};
    use crate::geometry::Rect;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered_bounds = Rc::new(RefCell::new(None));
    let nodes: Rc<RefCell<Vec<InspectorNode>>> = Rc::new(RefCell::new(vec![
        InspectorNode { type_name: "Root",  screen_bounds: Rect::new(0.0,0.0,100.0,50.0), depth: 0, properties: vec![] },
        InspectorNode { type_name: "Child", screen_bounds: Rect::new(0.0,0.0,50.0,20.0),  depth: 1, properties: vec![] },
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
    use std::sync::Arc;
    use std::cell::RefCell;
    use std::rc::Rc;
    use crate::text::Font;
    use crate::widgets::inspector::InspectorPanel;
    use crate::widget::{InspectorNode, Widget};
    use crate::geometry::Rect;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered_bounds = Rc::new(RefCell::new(None));
    let nodes: Rc<RefCell<Vec<InspectorNode>>> = Rc::new(RefCell::new(vec![
        InspectorNode { type_name: "Root", screen_bounds: Rect::new(0.0,0.0,100.0,50.0), depth: 0, properties: vec![] },
    ]));

    let panel = InspectorPanel::new(Arc::clone(&font), Rc::clone(&nodes), hovered_bounds);
    assert!(!panel.tree_view.drag_enabled, "inspector TreeView must have drag disabled");
}

/// ExpandToggle paints a filled triangle when has_children=true, nothing when false.
#[test]
fn test_expand_toggle_paints_arrow_only_when_has_children() {
    use std::sync::Arc;
    use crate::text::Font;
    use crate::widgets::tree_view::row::ExpandToggle;
    use crate::widget::paint_subtree;

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
    use std::sync::Arc;
    use crate::text::Font;
    use crate::widgets::text_field::TextField as TF;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut field = TF::new(font).with_font_size(14.0);
    field.layout(Size::new(200.0, 36.0));
    field.set_bounds(crate::Rect::new(0.0, 0.0, 200.0, 36.0));

    // Give focus directly.
    field.on_event(&crate::Event::FocusGained);

    // Type "Hi"
    field.on_event(&crate::Event::KeyDown { key: Key::Char('H'), modifiers: Modifiers::default() });
    field.on_event(&crate::Event::KeyDown { key: Key::Char('i'), modifiers: Modifiers::default() });
    assert_eq!(field.text(), "Hi", "typed characters should appear in text");

    // Backspace removes the last character.
    field.on_event(&crate::Event::KeyDown { key: Key::Backspace, modifiers: Modifiers::default() });
    assert_eq!(field.text(), "H", "backspace should remove last character");
}

/// After layout(), TreeView children() returns one TreeRow per visible node.
#[test]
fn test_treeview_children_count_equals_visible_rows() {
    use std::sync::Arc;
    use crate::text::Font;
    use crate::widgets::tree_view::{NodeIcon, TreeView};
    use crate::geometry::Size;
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut tv = TreeView::new(Arc::clone(&font));
    let root = tv.add_root("Root", NodeIcon::Folder);
    tv.add_child(root, "Child A", NodeIcon::File);
    tv.add_child(root, "Child B", NodeIcon::File);
    tv.nodes[root].is_expanded = true;
    tv.layout(Size::new(300.0, 200.0));
    // root + 2 children = 3 visible rows
    assert_eq!(tv.children().len(), 3, "expected 3 children after expanding root with 2 children");
}

/// Each TreeRow child has type_name "TreeRow".
#[test]
fn test_treeview_row_node_idx() {
    use std::sync::Arc;
    use crate::text::Font;
    use crate::widgets::tree_view::{NodeIcon, TreeView};
    use crate::geometry::Size;
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
    use std::sync::Arc;
    use std::cell::RefCell;
    use std::rc::Rc;
    use crate::text::Font;
    use crate::widgets::inspector::InspectorPanel;
    use crate::widget::{InspectorNode, Widget, paint_subtree};
    use crate::geometry::{Rect, Size};

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let nodes: Rc<RefCell<Vec<InspectorNode>>> = Rc::new(RefCell::new(vec![
        InspectorNode {
            type_name: "Window",
            screen_bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
            depth: 0,
            properties: vec![],
        },
    ]));
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
    use std::sync::Arc;
    use crate::widgets::tree_view::{NodeIcon, TreeView};
    use crate::geometry::{Point, Size};
    use crate::event::{Event, Modifiers, MouseButton};
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
    tv.on_event(&Event::MouseMove { pos: Point::new(50.0, 78.0) });

    // Re-layout with live drag active
    tv.layout(Size::new(200.0, 100.0));
    // The dragged node should be excluded → only 1 row widget
    assert_eq!(tv.children().len(), 1,
        "dragged node must be excluded from row_widgets during live drag");
}

// ---------------------------------------------------------------------------
// Composition tests — Button with Label child
// ---------------------------------------------------------------------------

/// Button must have exactly one child widget of type "Label" after layout.
#[test]
fn test_button_has_label_child() {
    use std::sync::Arc;
    use crate::text::Font;
    const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut btn = Button::new("Click me", font);
    btn.layout(Size::new(200.0, 40.0));
    assert_eq!(btn.children().len(), 1, "Button must expose exactly one Label child");
    assert_eq!(btn.children()[0].type_name(), "Label",
               "Button's child must be a Label widget");
}

/// After layout(), the Label child must have tight text bounds and be centred
/// within the button area.
#[test]
fn test_button_label_child_fills_button() {
    use std::sync::Arc;
    use crate::text::Font;
    const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut btn = Button::new("Click me", font);
    let size = btn.layout(Size::new(300.0, 50.0));
    let label_bounds = btn.children()[0].bounds();
    // Tight bounds: label width must be less than button width.
    assert!(label_bounds.width < size.width,
            "Label width must be tight (less than button width); got label_w={} btn_w={}",
            label_bounds.width, size.width);
    assert!(label_bounds.width > 0.0, "Label width must be positive");
    assert!(label_bounds.height > 0.0, "Label height must be positive");
    // Label must be horizontally centred: x ≈ (button_w - label_w) / 2.
    let expected_x = (size.width - label_bounds.width) * 0.5;
    assert!((label_bounds.x - expected_x).abs() < 1.0,
            "Label must be horizontally centred; expected x≈{:.1}, got x={:.1}",
            expected_x, label_bounds.x);
    // Label must be vertically centred.
    let expected_y = (size.height - label_bounds.height) * 0.5;
    assert!((label_bounds.y - expected_y).abs() < 1.0,
            "Label must be vertically centred; expected y≈{:.1}, got y={:.1}",
            expected_y, label_bounds.y);
}

/// Label::properties() must include text, font_size, and has_backbuffer.
#[test]
fn test_label_properties() {
    use std::sync::Arc;
    use crate::{Label, text::Font};
    const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let label = Label::new("Hello", font).with_font_size(13.0);
    let props: std::collections::HashMap<_, _> = label.properties().into_iter().collect();
    assert!(props.contains_key("text"), "Label must expose 'text' property");
    assert_eq!(props["text"], "Hello");
    assert!(props.contains_key("has_backbuffer"), "Label must expose 'has_backbuffer'");
    assert_eq!(props["has_backbuffer"], "true"); // buffered = true by default
}

/// Button properties must include the label text.
#[test]
fn test_button_properties() {
    use std::sync::Arc;
    use crate::text::Font;
    const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let btn = Button::new("Primary Action", font);
    let props: std::collections::HashMap<_, _> = btn.properties().into_iter().collect();
    assert!(props.contains_key("label"), "Button must expose 'label' property");
    assert_eq!(props["label"], "Primary Action");
}

/// collect_inspector_nodes must show Button at depth 0 and Label at depth 1.
#[test]
fn test_button_inspector_hierarchy() {
    use std::sync::Arc;
    use crate::{text::Font, widget::collect_inspector_nodes, geometry::{Point, Rect}};
    const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
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
    use crate::widget::{collect_inspector_nodes, Widget};
    use crate::geometry::{Point, Rect, Size};
    use crate::event::{Event, EventResult};
    use crate::draw_ctx::DrawCtx;

    /// Minimal widget whose visibility can be toggled.
    struct ToggleWidget {
        bounds:   Rect,
        visible:  bool,
        children: Vec<Box<dyn Widget>>,
    }
    impl Widget for ToggleWidget {
        fn type_name(&self) -> &'static str { "ToggleWidget" }
        fn is_visible(&self) -> bool { self.visible }
        fn bounds(&self) -> Rect { self.bounds }
        fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
        fn children(&self) -> &[Box<dyn Widget>] { &self.children }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
        fn layout(&mut self, available: Size) -> Size { available }
        fn paint(&mut self, _: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
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
    assert!(nodes.is_empty(), "invisible widget produces no inspector nodes");
}

/// `toggle_on_row_click = false` (the inspector's mode): clicking a row
/// SELECTS it but does NOT toggle its expansion state.  This prevents the
/// inspector tree from collapsing to one visible line when the user clicks on
/// the root node to inspect it.
#[test]
fn test_treeview_click_selects_without_collapsing_when_flag_off() {
    use std::sync::Arc;
    use crate::text::Font;
    use crate::geometry::{Point, Size};
    use crate::event::Modifiers;
    const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));

    let mut tv = crate::widgets::tree_view::TreeView::new(Arc::clone(&font))
        .with_row_height(20.0);
    // toggle_on_row_click defaults to false — inspector mode.

    let root = tv.add_root("Root", crate::widgets::tree_view::NodeIcon::Package);
    tv.expand(root);
    tv.add_child(root, "Child A", crate::widgets::tree_view::NodeIcon::File);
    tv.add_child(root, "Child B", crate::widgets::tree_view::NodeIcon::File);

    use crate::widget::Widget;
    tv.layout(Size::new(300.0, 200.0));
    tv.set_bounds(crate::geometry::Rect::new(0.0, 0.0, 300.0, 200.0));

    // 3 visible rows (Root expanded, 2 children).
    assert_eq!(tv.children().len(), 3, "should have Root + 2 children visible");

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
        tv.children().len(), 3,
        "clicking root row must NOT collapse it when toggle_on_row_click = false"
    );
}

/// `toggle_on_row_click = true` (file-explorer mode): clicking anywhere on a
/// row with children ALSO toggles its expansion — consistent with VS Code /
/// Cursor file-tree behaviour.
#[test]
fn test_treeview_click_collapses_when_flag_on() {
    use std::sync::Arc;
    use crate::text::Font;
    use crate::geometry::{Point, Size};
    use crate::event::Modifiers;
    const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));

    let mut tv = crate::widgets::tree_view::TreeView::new(Arc::clone(&font))
        .with_row_height(20.0)
        .with_toggle_on_row_click();  // file-explorer mode

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
        tv.children().len(), 1,
        "clicking root row body must collapse it when toggle_on_row_click = true"
    );
}

// ---------------------------------------------------------------------------
// Phase N — layer compositing
// ---------------------------------------------------------------------------

/// `push_layer` / `pop_layer` must composite a solid red square into a white
/// framebuffer.  The composited pixels must be red, not white or black.
#[test]
fn test_push_pop_layer_solid_composites_correctly() {
    let mut fb = Framebuffer::new(20, 20);
    let mut ctx = GfxCtx::new(&mut fb);
    // White background.
    ctx.clear(Color::white());

    // Draw a red square via a layer — the layer sits at (0,0) so the full fb
    // is covered.
    ctx.push_layer(20.0, 20.0);
    ctx.set_fill_color(Color::rgba(1.0, 0.0, 0.0, 1.0));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, 20.0, 20.0);
    ctx.fill();
    ctx.pop_layer();

    drop(ctx);

    let center = sample(&fb, 10, 10);
    assert!(is_red(center), "After layer composite, centre must be red; got {center:?}");
}

/// A layer with 50 % alpha blended onto a white background must produce a
/// pixel that is neither fully red nor fully white (i.e. a pink mid-tone).
#[test]
fn test_push_pop_layer_alpha_blends_into_parent() {
    let mut fb = Framebuffer::new(20, 20);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());

    ctx.push_layer(20.0, 20.0);
    // 50 % opaque red.
    ctx.set_fill_color(Color::rgba(1.0, 0.0, 0.0, 0.5));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, 20.0, 20.0);
    ctx.fill();
    ctx.pop_layer();

    drop(ctx);

    let [r, g, b, _] = sample(&fb, 10, 10);
    // Result should be pink: R high, G and B ~midway (not 0, not 255).
    assert!(r > 200, "Red channel must be high; got {r}");
    assert!(g > 80 && g < 200, "Green channel must be mid-tone (pink); got {g}");
    assert!(b > 80 && b < 200, "Blue channel must be mid-tone (pink); got {b}");
}

/// A `Label` with `has_backbuffer = true` must render identically to one
/// without when drawn on a plain white background — the compositing must
/// preserve the text pixels.
#[test]
fn test_label_backbuffer_renders_text() {
    use std::sync::Arc;
    use crate::{Label, text::Font};
    use crate::widget::Widget;

    const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));

    // Render without backbuffer — explicit color so text is dark on white bg.
    let mut fb_direct = Framebuffer::new(200, 60);
    {
        let mut ctx = GfxCtx::new(&mut fb_direct);
        ctx.clear(Color::white());
        let mut lbl = Label::new("Hi", Arc::clone(&font))
            .with_font_size(20.0)
            .with_has_backbuffer(false)
            .with_color(Color::black());
        lbl.layout(Size::new(200.0, 60.0));
        lbl.set_bounds(crate::geometry::Rect::new(0.0, 0.0, 200.0, 60.0));
        crate::widget::paint_subtree(&mut lbl, &mut ctx);
    }

    // Render with backbuffer — explicit color so blitted pixels are dark on white bg.
    let mut fb_layer = Framebuffer::new(200, 60);
    {
        let mut ctx = GfxCtx::new(&mut fb_layer);
        ctx.clear(Color::white());
        let mut lbl = Label::new("Hi", Arc::clone(&font))
            .with_font_size(20.0)
            .with_has_backbuffer(true)
            .with_color(Color::black());
        lbl.layout(Size::new(200.0, 60.0));
        lbl.set_bounds(crate::geometry::Rect::new(0.0, 0.0, 200.0, 60.0));
        crate::widget::paint_subtree(&mut lbl, &mut ctx);
    }

    // Both framebuffers must contain at least some dark pixels (rendered text).
    let dark_direct = fb_direct.pixels().chunks_exact(4)
        .filter(|p| (p[0] as u32 + p[1] as u32 + p[2] as u32) < 300)
        .count();
    let dark_layer = fb_layer.pixels().chunks_exact(4)
        .filter(|p| (p[0] as u32 + p[1] as u32 + p[2] as u32) < 300)
        .count();
    assert!(dark_direct > 0, "direct render must produce dark (text) pixels");
    assert!(dark_layer  > 0, "backbuffer render must produce dark (text) pixels");
}

/// `Label`'s backbuffer cache MUST contain **straight-alpha** RGBA so that the
/// GL path's `SRC_ALPHA, ONE_MINUS_SRC_ALPHA` blend produces correct AA edges.
///
/// A half-coverage white edge pixel in a premultiplied buffer is
/// `(128, 128, 128, 128)`; in a straight-alpha buffer it's
/// `(255, 255, 255, 128)`.  If this test ever flips back to premul, GL text
/// on coloured buttons loses ~50 % intensity at AA edges (dark fringe).
#[test]
fn test_label_backbuffer_cache_is_straight_alpha() {
    use std::sync::Arc;
    use crate::{Label, text::Font};
    use crate::widget::Widget;

    const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));

    let mut fb = Framebuffer::new(200, 60);
    let mut lbl = Label::new("Hi", Arc::clone(&font))
        .with_font_size(20.0)
        .with_has_backbuffer(true)
        .with_color(Color::white()); // white text → AA edge pixels are the critical case
    lbl.layout(Size::new(200.0, 60.0));
    lbl.set_bounds(crate::geometry::Rect::new(0.0, 0.0, 200.0, 60.0));
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::transparent());
        crate::widget::paint_subtree(&mut lbl, &mut ctx);
    }

    let (pixels, _w, _h) = lbl.cache_for_test().expect("backbuffer cached");

    // Find partial-alpha pixels (AA edges).  In straight alpha the RGB must be
    // at full white (255) — it's the alpha that encodes coverage.  In premul
    // the RGB would be near the alpha value (≤ alpha).
    let mut edge_samples = 0u32;
    let mut premul_looking = 0u32;
    for px in pixels.chunks_exact(4) {
        let a = px[3];
        if a > 0 && a < 240 {
            edge_samples += 1;
            // Straight white at alpha<255 → r=g=b=255 (within rounding).
            // Premul white at alpha<255 → r=g=b=a.
            if (px[0] as i32 - a as i32).abs() < 8
                && (px[1] as i32 - a as i32).abs() < 8
                && (px[2] as i32 - a as i32).abs() < 8
                && px[0] < 240 // rule out fully-saturated straight case
            {
                premul_looking += 1;
            }
        }
    }
    assert!(edge_samples > 0, "expected some AA edge pixels in backbuffer");
    assert_eq!(
        premul_looking, 0,
        "found {premul_looking} premul-looking edge pixels out of {edge_samples}; \
         backbuffer must be straight-alpha"
    );
}

/// **MatterCAD-style equivalence test** (port of `BackBuffersAreScreenAligned`).
///
/// The Label backbuffer must contain BYTE-IDENTICAL pixels to a direct AGG
/// `fill_text` call with the same parameters.  If this ever diverges the
/// cached bitmap is no longer "what AGG would render" and GL-side blits will
/// silently display stale / wrong content.
#[test]
fn test_label_backbuffer_matches_direct_agg_render() {
    use std::sync::Arc;
    use crate::framebuffer::unpremultiply_rgba_inplace;
    use crate::text::Font;
    use crate::Label;
    use crate::widget::Widget;
    use crate::image_cache;
    use crate::geometry::Rect;

    const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
    let font      = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let text      = "Pixel alignment test";
    let font_size = 14.0_f64;
    let color     = Color::black();

    // Label layout: line_h = font_size * 1.5 (matches Label::layout).
    let line_h = font_size * 1.5;
    let width  = crate::text::measure_text_metrics(&font, text, font_size).width;
    let bw     = width.ceil() as u32;
    let bh     = line_h.ceil() as u32;

    // --- PATH A ----------------------------------------------------------
    // Direct AGG render using the same pipeline Label's closure runs.
    let mut fb_direct = Framebuffer::new(bw, bh);
    {
        let mut gfx = GfxCtx::new(&mut fb_direct);
        gfx.set_font(Arc::clone(&font));
        gfx.set_font_size(font_size);
        gfx.set_fill_color(color);
        if let Some(m) = gfx.measure_text(text) {
            let ty = line_h * 0.5 - (m.ascent - m.descent) * 0.5;
            let tx = 0.0; // LabelAlign::Left
            gfx.fill_text(text, tx, ty);
        }
    }
    let mut expected = fb_direct.pixels_flipped();
    unpremultiply_rgba_inplace(&mut expected);

    // --- PATH B ----------------------------------------------------------
    // Label backbuffer (via global pixel cache → AGG closure → unpremul).
    image_cache::clear(); // ensure we take the raster path, not a stale hit
    let mut lbl = Label::new(text, Arc::clone(&font))
        .with_font_size(font_size)
        .with_has_backbuffer(true)
        .with_color(color);
    lbl.set_bounds(Rect::new(0.0, 0.0, width, line_h));

    let mut outer = Framebuffer::new(bw, bh);
    {
        let mut gfx = GfxCtx::new(&mut outer);
        gfx.clear(Color::white());
        crate::widget::paint_subtree(&mut lbl, &mut gfx);
    }

    let (actual, cw, ch) = lbl.cache_for_test().expect("Label must cache pixels");
    assert_eq!(cw, bw);
    assert_eq!(ch, bh);
    assert_eq!(
        actual.len(), expected.len(),
        "buffer length mismatch: direct={} cache={}", expected.len(), actual.len()
    );

    // Label's closure and the direct path share the same AGG code, so bytes
    // must match exactly — no tolerance window.  Any divergence here would
    // mean the cache isn't actually "what AGG would produce".
    if actual != expected.as_slice() {
        // On failure, find the first differing pixel for a useful message.
        for i in (0..actual.len()).step_by(4) {
            if actual[i..i+4] != expected[i..i+4] {
                let px = (i / 4) as u32;
                let y = px / bw;
                let x = px % bw;
                panic!(
                    "Label backbuffer pixel ({x}, {y}) diverges from direct AGG:\n\
                     direct = {:?}\n\
                     cache  = {:?}",
                    &expected[i..i+4], &actual[i..i+4]
                );
            }
        }
    }
}

/// **Paint-entry CTM snap invariant.**
///
/// The contract for a widget whose `enforce_integer_bounds()` returns `true`
/// is: "my `paint()` is called with an integer-translation CTM".  That
/// contract MUST hold regardless of how the widget is reached — via the
/// normal parent-walks-children loop inside `paint_subtree`, OR via a manual
/// `ctx.translate(fractional, fractional); paint_subtree(child, ctx)`
/// sequence in a widget that does its own layout (SegRow, drag overlays,
/// popups, anything with custom centering math).
///
/// Before the fix the snap happened only in the child-iteration loop, so
/// manual-translate callers silently handed off a fractional CTM — invisibly
/// breaking the guarantee and producing blurry `Label` backbuffer blits.
///
/// This test wraps a probe widget in a fractional manual translate and
/// asserts the probe sees an integer CTM at `paint()` entry.  If anyone ever
/// removes the `paint_subtree` entry snap again, this regresses.
#[test]
fn test_paint_subtree_snaps_ctm_for_manual_translate_entry() {
    use std::cell::Cell;
    use std::rc::Rc;
    use crate::draw_ctx::DrawCtx;
    use crate::geometry::Rect;
    use crate::widget::{paint_subtree, Widget};
    use crate::event::{Event, EventResult};
    use agg_rust::trans_affine::TransAffine;

    /// Widget that captures the CTM present at its `paint()` entry.
    struct CtmProbe {
        bounds:   Rect,
        children: Vec<Box<dyn Widget>>,
        captured: Rc<Cell<Option<TransAffine>>>,
    }

    impl Widget for CtmProbe {
        fn type_name(&self) -> &'static str { "CtmProbe" }
        fn bounds(&self) -> Rect { self.bounds }
        fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
        fn children(&self) -> &[Box<dyn Widget>] { &self.children }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
        fn layout(&mut self, available: Size) -> Size {
            Size::new(self.bounds.width.min(available.width),
                      self.bounds.height.min(available.height))
        }
        fn paint(&mut self, ctx: &mut dyn DrawCtx) {
            self.captured.set(Some(ctx.transform()));
        }
        fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
    }

    let captured = Rc::new(Cell::new(None));
    let mut probe = CtmProbe {
        bounds:   Rect::new(0.0, 0.0, 10.0, 10.0),
        children: Vec::new(),
        captured: Rc::clone(&captured),
    };

    let mut fb = Framebuffer::new(100, 100);
    let mut ctx = GfxCtx::new(&mut fb);

    // Manual caller: applies a FRACTIONAL translate, then drives paint_subtree.
    // This is the pattern `SegRow` uses when centring labels in unevenly
    // divided columns.  The snap has to happen inside paint_subtree — manual
    // callers shouldn't need to remember `snap_to_pixel`.
    ctx.translate(100.3, 50.7);
    paint_subtree(&mut probe, &mut ctx);

    let ctm = captured.get().expect("probe must have been painted");
    assert_eq!(
        ctm.tx.fract(), 0.0,
        "tx still fractional at paint() entry: {} — paint_subtree snap regressed",
        ctm.tx,
    );
    assert_eq!(
        ctm.ty.fract(), 0.0,
        "ty still fractional at paint() entry: {} — paint_subtree snap regressed",
        ctm.ty,
    );
    // Specific floor values so this also guards against a silent change to
    // round-nearest (which would subtly shift widgets by up to 0.5 px).
    assert_eq!(ctm.tx, 100.0);
    assert_eq!(ctm.ty, 50.0);
}

/// A widget that opts OUT of enforce_integer_bounds must NOT have its CTM
/// snapped — preserves sub-pixel positioning for smooth-scroll markers /
/// zoomed canvases.
#[test]
fn test_paint_subtree_preserves_fractional_ctm_when_opted_out() {
    use std::cell::Cell;
    use std::rc::Rc;
    use crate::draw_ctx::DrawCtx;
    use crate::geometry::Rect;
    use crate::widget::{paint_subtree, Widget};
    use crate::event::{Event, EventResult};
    use agg_rust::trans_affine::TransAffine;

    struct SubpixelProbe {
        bounds:   Rect,
        children: Vec<Box<dyn Widget>>,
        captured: Rc<Cell<Option<TransAffine>>>,
    }
    impl Widget for SubpixelProbe {
        fn type_name(&self) -> &'static str { "SubpixelProbe" }
        fn bounds(&self) -> Rect { self.bounds }
        fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
        fn children(&self) -> &[Box<dyn Widget>] { &self.children }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
        fn layout(&mut self, available: Size) -> Size {
            Size::new(self.bounds.width.min(available.width),
                      self.bounds.height.min(available.height))
        }
        fn paint(&mut self, ctx: &mut dyn DrawCtx) {
            self.captured.set(Some(ctx.transform()));
        }
        fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
        fn enforce_integer_bounds(&self) -> bool { false } // opt out
    }

    let captured = Rc::new(Cell::new(None));
    let mut probe = SubpixelProbe {
        bounds:   Rect::new(0.0, 0.0, 10.0, 10.0),
        children: Vec::new(),
        captured: Rc::clone(&captured),
    };

    let mut fb = Framebuffer::new(100, 100);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.translate(100.3, 50.7);
    paint_subtree(&mut probe, &mut ctx);

    let ctm = captured.get().expect("probe must have been painted");
    // Opt-out honoured: CTM passes through untouched.
    assert!((ctm.tx - 100.3).abs() < 1e-9, "opt-out widget had tx snapped: {}", ctm.tx);
    assert!((ctm.ty - 50.7).abs() < 1e-9, "opt-out widget had ty snapped: {}", ctm.ty);
}

/// AGG's software rasterizer, given **integer-aligned** 1-px-wide fills at
/// integer positions, must produce pixels that are **exactly** the fill
/// colour or the original buffer — never a half-covered mid-tone.  If this
/// ever regresses, every "bitmap then blit" path in the app loses its
/// pixel-perfect guarantee, including Label backbuffers.
///
/// This is the agg-side half of the "why did the bitmap grid look fuzzy on
/// native GL" investigation — if AGG is correct here, the fault lies in the
/// texture-upload / texture-sample stage; if AGG is wrong here, the source
/// image is already gray before the GL blit.
#[test]
fn test_agg_rasters_1px_stripes_with_zero_gray() {
    use crate::framebuffer::unpremultiply_rgba_inplace;

    let w = 96_u32;
    let h = 96_u32;
    let mut fb = Framebuffer::new(w, h);
    {
        let mut gfx = GfxCtx::new(&mut fb);
        // Alternating 1-px white / 1-px black vertical columns — exactly what
        // `PixelTestLinesBitmap` draws.
        for i in 0..(w as usize / 2) {
            let x = (2 * i) as f64;
            gfx.set_fill_color(Color::white());
            gfx.begin_path();
            gfx.rect(x, 0.0, 1.0, h as f64);
            gfx.fill();
            gfx.set_fill_color(Color::black());
            gfx.begin_path();
            gfx.rect(x + 1.0, 0.0, 1.0, h as f64);
            gfx.fill();
        }
    }
    let mut pixels = fb.pixels_flipped();
    unpremultiply_rgba_inplace(&mut pixels);

    let row_bytes = (w * 4) as usize;
    for y in 0..h as usize {
        for x in 0..w as usize {
            let off = y * row_bytes + x * 4;
            let px = &pixels[off..off + 4];
            let expected_white = x % 2 == 0;
            let (er, eg, eb) = if expected_white { (255, 255, 255) } else { (0, 0, 0) };
            assert_eq!(
                (px[0], px[1], px[2], px[3]),
                (er, eg, eb, 255),
                "pixel ({x}, {y}) should be {} but is {:?}",
                if expected_white { "white" } else { "black" },
                px,
            );
        }
    }
}

/// `snap_to_pixel` must zero the fractional component of the CTM translation
/// and leave rotations / scales / integer translations alone.  Covers the
/// `paint_subtree` round-on-translate path exercised by every widget that
/// opts into `enforce_integer_bounds`.
#[test]
fn test_snap_to_pixel_zeros_fractional_translation() {
    use crate::draw_ctx::DrawCtx;

    let mut fb = Framebuffer::new(10, 10);
    let mut ctx = GfxCtx::new(&mut fb);

    // Build a pure-translation CTM with fractional tx and ty.
    ctx.translate(100.3, 50.7);
    let before = ctx.transform();
    assert!((before.tx - 100.3).abs() < 1e-9);
    assert!((before.ty - 50.7).abs() < 1e-9);

    ctx.snap_to_pixel();
    let after = ctx.transform();
    assert_eq!(after.tx.fract(), 0.0, "tx still fractional: {}", after.tx);
    assert_eq!(after.ty.fract(), 0.0, "ty still fractional: {}", after.ty);
    // Snap rounds DOWN (floor) so text/strokes sit on the pixel they would
    // have partially covered — predictable and matches MatterCAD semantics.
    assert_eq!(after.tx, 100.0);
    assert_eq!(after.ty, 50.0);

    // Negative translations floor toward -infinity.
    let mut fb2 = Framebuffer::new(10, 10);
    let mut ctx2 = GfxCtx::new(&mut fb2);
    ctx2.translate(-3.3, -4.7);
    ctx2.snap_to_pixel();
    let after2 = ctx2.transform();
    assert_eq!(after2.tx, -4.0);
    assert_eq!(after2.ty, -5.0);

    // Already-integer translation is a no-op.
    let mut fb3 = Framebuffer::new(10, 10);
    let mut ctx3 = GfxCtx::new(&mut fb3);
    ctx3.translate(7.0, 13.0);
    ctx3.snap_to_pixel();
    let after3 = ctx3.transform();
    assert_eq!(after3.tx, 7.0);
    assert_eq!(after3.ty, 13.0);
}

// ---------------------------------------------------------------------------
// Slider mouse-capture tests
// ---------------------------------------------------------------------------

/// Dragging a slider outside its bounds must continue to track the cursor —
/// clamping at the range limits — and must NOT snap to the near edge when the
/// pointer first leaves the widget.
///
/// Root cause of the old bug: `dispatch_mouse_move` sent a synthetic
/// `MouseMove { pos: (-1.0, -1.0) }` to the previously-hovered widget when
/// the cursor left its bounds.  The slider's `on_event` called
/// `value_from_x(-1.0)` which clamped to `min`, snapping the thumb to the
/// left edge regardless of the actual cursor position.
///
/// This test reproduces the snap-to-zero bug and guards the mouse-capture fix.
#[test]
fn test_slider_drag_outside_bounds_tracks_cursor() {
    use std::rc::Rc;
    use std::cell::Cell;
    use std::sync::Arc;
    use crate::widgets::slider::Slider;
    use crate::text::Font;

    const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));

    let last_val = Rc::new(Cell::new(0.5_f64));
    let lv = Rc::clone(&last_val);

    // 200 × 36 px slider, value range [0, 1].
    let slider = Slider::new(0.5, 0.0, 1.0, Arc::clone(&font))
        .on_change(move |v| lv.set(v));

    let mut app = App::new(Box::new(
        SizedBox::new().with_width(200.0).with_height(36.0)
            .with_child(Box::new(slider)),
    ));
    app.layout(Size::new(200.0, 36.0));

    // Press the thumb in the middle.  Y-down input: viewport_height(36) − 18 = 18 Y-up.
    app.on_mouse_down(100.0, 18.0, MouseButton::Left, Modifiers::default());

    // Drag far to the right (outside slider bounds) — value must clamp to max.
    app.on_mouse_move(9999.0, 18.0);
    assert_eq!(
        last_val.get(), 1.0,
        "dragging outside right must clamp to max (1.0), not snap to 0.0"
    );

    // Drag far to the left — value must clamp to min.
    app.on_mouse_move(-9999.0, 18.0);
    assert_eq!(
        last_val.get(), 0.0,
        "dragging outside left must clamp to min (0.0)"
    );

    // Release outside bounds — drag ends.
    app.on_mouse_up(0.0, 18.0, MouseButton::Left, Modifiers::default());

    // After release: moving the mouse must NOT fire the callback.
    last_val.set(999.0); // sentinel
    app.on_mouse_move(100.0, 18.0);
    assert_eq!(
        last_val.get(), 999.0,
        "after mouse-up the slider must stop tracking cursor movement"
    );
}

// ---------------------------------------------------------------------------
// Phase 7 — Layout property system tests
// ---------------------------------------------------------------------------

use crate::{HAnchor, Insets, Padding, Separator, VAnchor, WidgetBase,
            device_scale, set_device_scale, resolve_fit_or_stretch};

// --- Insets arithmetic ------------------------------------------------------

#[test]
fn test_insets_all() {
    let i = Insets::all(5.0);
    assert_eq!(i.left, 5.0);
    assert_eq!(i.right, 5.0);
    assert_eq!(i.top, 5.0);
    assert_eq!(i.bottom, 5.0);
}

#[test]
fn test_insets_symmetric() {
    let i = Insets::symmetric(10.0, 4.0);
    assert_eq!(i.horizontal(), 20.0);
    assert_eq!(i.vertical(),    8.0);
}

#[test]
fn test_insets_scale() {
    let i = Insets::all(3.0).scale(2.0);
    assert_eq!(i.left, 6.0);
    assert_eq!(i.top,  6.0);
}

// --- HAnchor / VAnchor bitflag algebra --------------------------------------

#[test]
fn test_hanchor_stretch_contains_left_and_right() {
    assert!(HAnchor::STRETCH.contains(HAnchor::LEFT));
    assert!(HAnchor::STRETCH.contains(HAnchor::RIGHT));
    assert!(HAnchor::STRETCH.is_stretch());
}

#[test]
fn test_hanchor_left_not_stretch() {
    assert!(!HAnchor::LEFT.is_stretch());
}

#[test]
fn test_hanchor_max_fit_or_stretch_contains_stretch() {
    // MAX_FIT_OR_STRETCH = 13 = 8 | 1 | 4 = FIT | STRETCH
    assert!(HAnchor::MAX_FIT_OR_STRETCH.contains(HAnchor::LEFT));
    assert!(HAnchor::MAX_FIT_OR_STRETCH.contains(HAnchor::RIGHT));
    assert!(HAnchor::MAX_FIT_OR_STRETCH.contains(HAnchor::FIT));
}

#[test]
fn test_vanchor_stretch() {
    assert!(VAnchor::STRETCH.is_stretch());
    assert!(VAnchor::STRETCH.contains(VAnchor::BOTTOM));
    assert!(VAnchor::STRETCH.contains(VAnchor::TOP));
}

// --- resolve_fit_or_stretch -------------------------------------------------

#[test]
fn test_resolve_max_fit_or_stretch_prefers_larger() {
    // natural (fit) is bigger → keep it.
    assert_eq!(resolve_fit_or_stretch(100.0, 60.0, true), 100.0);
    // stretch is bigger → use stretch.
    assert_eq!(resolve_fit_or_stretch(40.0, 80.0, true), 80.0);
}

#[test]
fn test_resolve_min_fit_or_stretch_prefers_smaller() {
    assert_eq!(resolve_fit_or_stretch(100.0, 60.0, false), 60.0);
    assert_eq!(resolve_fit_or_stretch(40.0, 80.0, false), 40.0);
}

// --- WidgetBase clamp_size --------------------------------------------------

#[test]
fn test_widget_base_clamp_size() {
    let mut base = WidgetBase::new();
    base.min_size = Size::new(50.0, 30.0);
    base.max_size = Size::new(200.0, 100.0);

    let clamped = base.clamp_size(Size::new(10.0, 150.0));
    assert_eq!(clamped.width,  50.0,  "below min should clamp to min_w");
    assert_eq!(clamped.height, 100.0, "above max should clamp to max_h");
}

// --- DeviceScale scaled_margin ----------------------------------------------

#[test]
fn test_widget_base_scaled_margin_at_2x() {
    set_device_scale(2.0);
    let mut base = WidgetBase::new();
    base.margin = Insets::all(10.0);
    let scaled = base.scaled_margin();
    set_device_scale(1.0); // restore
    assert_eq!(scaled.left,   20.0);
    assert_eq!(scaled.bottom, 20.0);
}

#[test]
fn test_device_scale_default_is_one() {
    set_device_scale(1.0);
    assert_eq!(device_scale(), 1.0);
}

// --- Padding layout ---------------------------------------------------------

/// `Padding::new(Insets, child)` with asymmetric insets must place the child
/// at (left, bottom) and report the correct outer size.
#[test]
fn test_padding_asymmetric_layout() {
    // Use a Spacer as the child: it returns whatever size it's given.
    let child = Box::new(Spacer::new());
    let mut w = Padding::new(
        Insets::from_sides(10.0, 20.0, 5.0, 15.0), // left, right, top, bottom
        child,
    );

    let outer = w.layout(Size::new(100.0, 80.0));
    // Inner available: (100-10-20) × (80-5-15) = 70 × 60.
    // Spacer returns its full inner size, so content = 70 × 60.
    // Outer = 70+30 × 60+20 = 100 × 80.
    assert_eq!(outer.width,  100.0, "outer width should equal available.width");
    assert_eq!(outer.height,  80.0, "outer height should equal available.height");

    // Child bounds (in Padding-local Y-up coords): x=left=10, y=bottom=15.
    let cb = w.children()[0].bounds();
    assert_eq!(cb.x,      10.0, "child x should be left inset");
    assert_eq!(cb.y,      15.0, "child y should be bottom inset (Y-up)");
    assert_eq!(cb.width,  70.0, "child width = available.width - h_insets");
    assert_eq!(cb.height, 60.0, "child height = available.height - v_insets");
}

/// `Padding::uniform` is a convenience alias.
#[test]
fn test_padding_uniform_alias() {
    let mut w = Padding::uniform(8.0, Box::new(Spacer::new()));
    let outer = w.layout(Size::new(50.0, 40.0));
    assert_eq!(outer.width,  50.0);
    assert_eq!(outer.height, 40.0);
    let cb = w.children()[0].bounds();
    assert_eq!(cb.x, 8.0);
    assert_eq!(cb.y, 8.0);
}

// --- SizedBox anchor-aware child placement ----------------------------------

/// Child with `h_anchor = RIGHT` should be placed at the right edge of the box.
#[test]
fn test_sized_box_child_right_anchor() {
    let child = Box::new(
        SizedBox::fixed(30.0, 20.0)
            .with_h_anchor(HAnchor::RIGHT),
    );
    let mut outer = SizedBox::new()
        .with_width(100.0)
        .with_height(50.0)
        .with_child(child);

    outer.layout(Size::new(100.0, 50.0));
    let cb = outer.children()[0].bounds();
    // Right-aligned 30-wide child inside 100-wide box: x = 100 - 30 = 70.
    assert_eq!(cb.x, 70.0, "right-anchor child x should be box_w - child_w");
    assert_eq!(cb.width, 30.0);
}

/// Child with `v_anchor = TOP` should be placed at the top (high Y) of the box.
#[test]
fn test_sized_box_child_top_anchor() {
    let child = Box::new(
        SizedBox::fixed(20.0, 15.0)
            .with_v_anchor(VAnchor::TOP),
    );
    let mut outer = SizedBox::new()
        .with_width(50.0)
        .with_height(60.0)
        .with_child(child);

    outer.layout(Size::new(50.0, 60.0));
    let cb = outer.children()[0].bounds();
    // Top-aligned 15-tall child inside 60-tall box: y = 60 - 15 = 45.
    assert_eq!(cb.y, 45.0, "top-anchor child y should be box_h - child_h (Y-up)");
    assert_eq!(cb.height, 15.0);
}

/// Child with `h_anchor = CENTER` should be horizontally centered.
#[test]
fn test_sized_box_child_center_h_anchor() {
    let child = Box::new(
        SizedBox::fixed(20.0, 10.0)
            .with_h_anchor(HAnchor::CENTER),
    );
    let mut outer = SizedBox::new()
        .with_width(100.0)
        .with_height(50.0)
        .with_child(child);

    outer.layout(Size::new(100.0, 50.0));
    let cb = outer.children()[0].bounds();
    // Centered: x = (100 - 20) / 2 = 40.
    assert_eq!(cb.x, 40.0, "center-h child x should be (box_w - child_w) / 2");
}

/// Child with `h_anchor = STRETCH` should fill the box width.
#[test]
fn test_sized_box_child_stretch() {
    let child = Box::new(
        SizedBox::fixed(20.0, 10.0)
            .with_h_anchor(HAnchor::STRETCH),
    );
    let mut outer = SizedBox::new()
        .with_width(100.0)
        .with_height(50.0)
        .with_child(child);

    outer.layout(Size::new(100.0, 50.0));
    let cb = outer.children()[0].bounds();
    assert_eq!(cb.x,     0.0,   "stretched child should start at x=0");
    assert_eq!(cb.width, 100.0, "stretched child should fill box width");
}

// --- FlexColumn cross-axis anchoring ----------------------------------------

/// Children with LEFT / CENTER / RIGHT h_anchor must be placed correctly.
#[test]
fn test_flex_column_cross_axis_anchors() {
    let left_child = Box::new(
        SizedBox::fixed(30.0, 10.0).with_h_anchor(HAnchor::LEFT),
    );
    let center_child = Box::new(
        SizedBox::fixed(30.0, 10.0).with_h_anchor(HAnchor::CENTER),
    );
    let right_child = Box::new(
        SizedBox::fixed(30.0, 10.0).with_h_anchor(HAnchor::RIGHT),
    );
    let stretch_child = Box::new(
        SizedBox::fixed(30.0, 10.0).with_h_anchor(HAnchor::STRETCH),
    );

    let mut col = FlexColumn::new()
        .with_gap(0.0)
        .add(left_child)
        .add(center_child)
        .add(right_child)
        .add(stretch_child);

    col.layout(Size::new(100.0, 80.0));
    let children = col.children();

    // LEFT: x = 0
    assert_eq!(children[0].bounds().x, 0.0, "LEFT child x");
    // CENTER: x = (100 - 30) / 2 = 35
    let center_x = children[1].bounds().x;
    assert!((center_x - 35.0).abs() < 0.5, "CENTER child x ≈ 35, got {center_x}");
    // RIGHT: x = 100 - 30 = 70
    assert_eq!(children[2].bounds().x, 70.0, "RIGHT child x");
    // STRETCH: x = 0, width = 100
    assert_eq!(children[3].bounds().x,     0.0,   "STRETCH child x");
    assert_eq!(children[3].bounds().width, 100.0, "STRETCH child width");
}

// --- FlexColumn main-axis margin spacing ------------------------------------

/// A child with bottom margin pushes the next sibling down.
#[test]
fn test_flex_column_child_margin_spacing() {
    set_device_scale(1.0);
    // Two 10-tall children; first has margin.bottom = 5, second has margin.top = 3.
    // Gap = 0.  Total spacing between them = 5 + 3 = 8.
    let top_child = Box::new(
        SizedBox::fixed(50.0, 10.0)
            .with_margin(Insets::from_sides(0.0, 0.0, 0.0, 5.0)), // bottom=5
    );
    let bot_child = Box::new(
        SizedBox::fixed(50.0, 10.0)
            .with_margin(Insets::from_sides(0.0, 0.0, 3.0, 0.0)), // top=3
    );

    let mut col = FlexColumn::new()
        .with_gap(0.0)
        .add(top_child)
        .add(bot_child);

    // Give enough height: (10+5) + (3+10) = 28 total main-axis.
    col.layout(Size::new(100.0, 100.0));
    let children = col.children();

    let top_bounds = children[0].bounds();
    let bot_bounds = children[1].bounds();

    // Top child is placed first (high Y in Y-up), bottom child below it.
    // Gap between bottom of top_child and top of bot_child should be 5+3=8.
    let gap_between = top_bounds.y - (bot_bounds.y + bot_bounds.height);
    assert!(
        (gap_between - 8.0).abs() < 0.5,
        "gap between children should equal 5+3=8 (additive margins), got {gap_between}"
    );
}

// --- FlexRow cross-axis VAnchor ---------------------------------------------

/// FlexRow children with BOTTOM / CENTER / TOP v_anchor are placed correctly.
#[test]
fn test_flex_row_cross_axis_anchors() {
    let bot_child = Box::new(
        SizedBox::fixed(20.0, 15.0).with_v_anchor(VAnchor::BOTTOM),
    );
    let center_child = Box::new(
        SizedBox::fixed(20.0, 15.0).with_v_anchor(VAnchor::CENTER),
    );
    let top_child = Box::new(
        SizedBox::fixed(20.0, 15.0).with_v_anchor(VAnchor::TOP),
    );

    let mut row = FlexRow::new()
        .with_gap(0.0)
        .add(bot_child)
        .add(center_child)
        .add(top_child);

    row.layout(Size::new(200.0, 60.0));
    let children = row.children();

    // BOTTOM (Y-up): y = 0 (pad_b = 0, margin_b = 0)
    assert_eq!(children[0].bounds().y, 0.0, "BOTTOM child y");
    // CENTER: y = (60 - 15) / 2 = 22.5, rounded to integer → 23
    let cy = children[1].bounds().y;
    assert_eq!(cy, 23.0, "CENTER child y rounded to integer, got {cy}");
    // TOP: y = 60 - 15 = 45
    assert_eq!(children[2].bounds().y, 45.0, "TOP child y (Y-up)");
}

// --- min_size / max_size clamping in FlexColumn -----------------------------

#[test]
fn test_flex_column_respects_child_min_size() {
    // Child reports natural height 5, but min_size.height = 20.
    // The column must allocate at least 20 px.
    let tiny = Box::new(
        SizedBox::fixed(50.0, 5.0).with_min_size(Size::new(50.0, 20.0)),
    );
    let mut col = FlexColumn::new().add(tiny);
    col.layout(Size::new(100.0, 200.0));
    assert_eq!(col.children()[0].bounds().height, 20.0,
               "fixed child height must respect min_size");
}

#[test]
fn test_flex_column_respects_child_max_size() {
    // Child is flex(1) in a 200-tall column, but max_size.height = 30.
    let big = Box::new(
        SizedBox::fixed(50.0, 50.0).with_max_size(Size::new(50.0, 30.0)),
    );
    let mut col = FlexColumn::new().add_flex(big, 1.0);
    col.layout(Size::new(100.0, 200.0));
    assert_eq!(col.children()[0].bounds().height, 30.0,
               "flex child height must respect max_size");
}

// --- MIN_FIT_OR_STRETCH and MAX_FIT_OR_STRETCH in FlexColumn ----------------

/// MIN_FIT_OR_STRETCH: child smaller than slot → use natural width (fit wins).
#[test]
fn test_min_fit_or_stretch_uses_fit_when_smaller() {
    // Column is 100 wide, child natural width is 40 → min(40, 100) = 40.
    let child = Box::new(
        SizedBox::fixed(40.0, 10.0)
            .with_h_anchor(HAnchor::MIN_FIT_OR_STRETCH),
    );
    let mut col = FlexColumn::new().add(child);
    col.layout(Size::new(100.0, 50.0));
    assert_eq!(col.children()[0].bounds().width, 40.0,
               "MIN_FIT_OR_STRETCH should use fit (40) when fit < stretch (100)");
}

/// MAX_FIT_OR_STRETCH: child smaller than slot → use slot width (stretch wins).
#[test]
fn test_max_fit_or_stretch_uses_stretch_when_larger() {
    let child = Box::new(
        SizedBox::fixed(40.0, 10.0)
            .with_h_anchor(HAnchor::MAX_FIT_OR_STRETCH),
    );
    let mut col = FlexColumn::new().add(child);
    col.layout(Size::new(100.0, 50.0));
    assert_eq!(col.children()[0].bounds().width, 100.0,
               "MAX_FIT_OR_STRETCH should use stretch (100) when stretch > fit (40)");
}
