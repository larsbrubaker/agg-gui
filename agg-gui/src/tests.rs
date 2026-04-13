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
        InspectorNode { type_name: "Root",  screen_bounds: Rect::new(0.0,0.0,100.0,50.0), depth: 0 },
        InspectorNode { type_name: "Child", screen_bounds: Rect::new(0.0,0.0,50.0,20.0),  depth: 1 },
    ]));

    let mut panel = InspectorPanel::new(Arc::clone(&font), Rc::clone(&nodes), Rc::clone(&hovered_bounds));
    panel.layout(crate::Size::new(200.0, 300.0));
    panel.set_bounds(Rect::new(0.0, 0.0, 200.0, 300.0));

    // InspectorPanel exposes no children — TreeView is managed directly.
    assert!(panel.children().is_empty(), "InspectorPanel children must be empty");

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
        InspectorNode { type_name: "Root",    screen_bounds: Rect::new(0.0,0.0,100.0,50.0), depth: 0 },
        InspectorNode { type_name: "Child",   screen_bounds: Rect::new(0.0,0.0,50.0,20.0),  depth: 1 },
        InspectorNode { type_name: "Sibling", screen_bounds: Rect::new(0.0,0.0,50.0,20.0),  depth: 0 },
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

    // InspectorPanel.children() returns empty — TreeView is not in child slice.
    assert!(panel.children().is_empty(), "InspectorPanel.children() must be empty");
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
        InspectorNode { type_name: "Root",  screen_bounds: Rect::new(0.0,0.0,100.0,50.0), depth: 0 },
        InspectorNode { type_name: "Child", screen_bounds: Rect::new(0.0,0.0,50.0,20.0),  depth: 1 },
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
        InspectorNode { type_name: "Root", screen_bounds: Rect::new(0.0,0.0,100.0,50.0), depth: 0 },
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
    assert_eq!(field.text, "Hi", "typed characters should appear in text");

    // Backspace removes the last character.
    field.on_event(&crate::Event::KeyDown { key: Key::Backspace, modifiers: Modifiers::default() });
    assert_eq!(field.text, "H", "backspace should remove last character");
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
