use super::*;

#[test]
fn test_ignored_event_request_draw_advances_invalidation_epoch() {
    use crate::widget::{dispatch_event, BackbufferState};
    use crate::{DrawCtx, Event, EventResult, Point, Rect};

    struct VisualChangeProbe {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
    }

    impl Widget for VisualChangeProbe {
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
            crate::animation::request_draw();
            EventResult::Ignored
        }
    }

    struct RetainedParent {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        backbuffer: BackbufferState,
    }

    impl Widget for RetainedParent {
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
            if let Some(child) = self.children.first_mut() {
                let size = child.layout(available);
                child.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
            }
            available
        }

        fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

        fn on_event(&mut self, _event: &Event) -> EventResult {
            EventResult::Ignored
        }

        fn backbuffer_state_mut(&mut self) -> Option<&mut BackbufferState> {
            Some(&mut self.backbuffer)
        }
    }

    let child = VisualChangeProbe {
        bounds: Rect::default(),
        children: Vec::new(),
    };
    let mut root: Box<dyn Widget> = Box::new(RetainedParent {
        bounds: Rect::default(),
        children: vec![Box::new(child)],
        backbuffer: BackbufferState::new(),
    });
    root.layout(Size::new(120.0, 40.0));
    root.backbuffer_state_mut().unwrap().dirty = false;
    let before = crate::animation::invalidation_epoch();

    let event = Event::MouseMove {
        pos: Point::new(10.0, 10.0),
    };
    assert_eq!(
        dispatch_event(&mut root, &[0], &event, Point::new(10.0, 10.0)),
        EventResult::Ignored
    );
    assert!(root.backbuffer_state_mut().unwrap().dirty);
    assert!(
        crate::animation::invalidation_epoch() != before,
        "request_draw should advance the dispatch-observed invalidation epoch"
    );
}

#[test]
fn test_tween_tick_requests_draw_via_invalidation_epoch() {
    let mut tween = crate::animation::Tween::new(0.0, 1.0);
    tween.set_target(1.0);
    let before = crate::animation::invalidation_epoch();
    let _ = tween.tick();
    assert!(
        crate::animation::invalidation_epoch() != before,
        "in-flight tweens should request a draw through the central invalidation path"
    );
}

#[test]
fn test_request_draw_alone_does_not_invalidate_software_backbuffer_cache() {
    use crate::widget::{paint_subtree, BackbufferCache};
    use crate::{DrawCtx, Event, EventResult, Rect};
    use std::cell::Cell;
    use std::rc::Rc;

    struct CachedProbe {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        cache: BackbufferCache,
        paints: Rc<Cell<usize>>,
    }

    impl Widget for CachedProbe {
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

        fn paint(&mut self, ctx: &mut dyn DrawCtx) {
            self.paints.set(self.paints.get() + 1);
            ctx.set_fill_color(Color::white());
            ctx.begin_path();
            ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
            ctx.fill();
        }

        fn on_event(&mut self, _event: &Event) -> EventResult {
            EventResult::Ignored
        }

        fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
            Some(&mut self.cache)
        }
    }

    let paints = Rc::new(Cell::new(0));
    let mut widget = CachedProbe {
        bounds: Rect::default(),
        children: Vec::new(),
        cache: BackbufferCache::new(),
        paints: Rc::clone(&paints),
    };
    widget.layout(Size::new(20.0, 20.0));

    let mut fb = Framebuffer::new(20, 20);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        paint_subtree(&mut widget, &mut ctx);
        paint_subtree(&mut widget, &mut ctx);
    }
    assert_eq!(paints.get(), 1, "clean software cache should be reused");

    crate::animation::request_draw();
    {
        let mut ctx = GfxCtx::new(&mut fb);
        paint_subtree(&mut widget, &mut ctx);
    }
    assert_eq!(
        paints.get(),
        1,
        "request_draw should schedule a frame, not globally reraster every software cache"
    );
}

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
    assert!(
        is_white(center),
        "Y=10 should be near the bottom of the buffer (Y-up); got {center:?}"
    );

    let top_center = sample(&fb, 50, 90);
    assert!(
        is_dark(top_center),
        "Y=90 should be dark (nothing drawn there); got {top_center:?}"
    );
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
    assert!(
        is_white(above_center),
        "+90° CCW rotation should produce upward bar; pixel above center is {above_center:?}"
    );

    let right_of_center = sample(&fb, cx as u32 + 25, cy as u32);
    assert!(
        is_dark(right_of_center),
        "After +90° rotation, horizontal should be gone; pixel to right is {right_of_center:?}"
    );
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
    assert!(
        is_red(center),
        "Bottom-left origin test: (10,10) should be red; got {center:?}"
    );

    let top_right = sample(&fb, 190, 190);
    assert!(
        is_dark(top_right),
        "Top-right should be empty; got {top_right:?}"
    );
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
            pixels[i] = 255;
            pixels[i + 1] = 0;
            pixels[i + 2] = 0;
            pixels[i + 3] = 255;
        }
        let base = 3 * w as usize * 4;
        for x in 0..w as usize {
            let i = base + x * 4;
            pixels[i] = 0;
            pixels[i + 1] = 0;
            pixels[i + 2] = 255;
            pixels[i + 3] = 255;
        }
    }

    let flipped = fb.pixels_flipped();
    assert_eq!(
        &flipped[0..4],
        &[0u8, 0, 255, 255],
        "Flipped[0] should be blue"
    );
    let last = (h as usize - 1) * w as usize * 4;
    assert_eq!(
        &flipped[last..last + 4],
        &[255u8, 0, 0, 255],
        "Flipped last row should be red"
    );
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
    assert!(
        is_dark(left),
        "Left half should be clipped out; got {left:?}"
    );

    // Right half (x=75, y=50) must be white — inside clip.
    let right = sample(&fb, 75, 50);
    assert!(
        is_white(right),
        "Right half should be white (inside clip); got {right:?}"
    );
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
    assert!(
        is_white(left),
        "After restore, clip should be gone; got {left:?}"
    );
}

// ---------------------------------------------------------------------------
// Phase 2 — rounded rect
// ---------------------------------------------------------------------------

/// A rounded_rect with radius 0 behaves identically to a plain rect.
#[test]
fn test_rounded_rect_zero_radius() {
    let size = 100u32;
    let mut fb_rr = Framebuffer::new(size, size);
    let mut fb_r = Framebuffer::new(size, size);

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
    assert!(
        is_white(sample(&fb_rr, 50, 50)),
        "rounded_rect center should be white"
    );
    assert!(
        is_white(sample(&fb_r, 50, 50)),
        "rect center should be white"
    );
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
    assert!(
        is_dark(corner),
        "Corner should be clipped by radius; got {corner:?}"
    );

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
    assert!(
        p[0] > 100 && p[0] < 160,
        "50% black over white should be mid-gray; got {p:?}"
    );
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
    assert!(
        p[1] > 100,
        "Green channel should be non-zero (blended with white); got {p:?}"
    );
}

// ---------------------------------------------------------------------------
// Phase 3 — text rendering
// ---------------------------------------------------------------------------

/// `measure_text` returns a wider advance for a longer string.
#[test]
fn test_measure_text_longer_is_wider() {
    use crate::text::Font;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut fb = Framebuffer::new(400, 100);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.set_font(font);
    ctx.set_font_size(20.0);

    let short = ctx.measure_text("Hi").unwrap();
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
    use crate::text::Font;
    use std::sync::Arc;

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
    assert!(
        dark_count > 10,
        "fill_text should paint dark pixels; got {dark_count}"
    );
}

/// `measure_text` returns positive ascent and line_height values.
#[test]
fn test_measure_text_metrics_positive() {
    use crate::text::Font;
    use std::sync::Arc;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let mut fb = Framebuffer::new(200, 60);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.set_font(font);
    ctx.set_font_size(16.0);

    let m = ctx.measure_text("Ag").unwrap();
    assert!(m.ascent > 0.0, "ascent must be positive; got {}", m.ascent);
    assert!(
        m.descent > 0.0,
        "descent must be positive; got {}",
        m.descent
    );
    assert!(
        m.line_height >= m.ascent + m.descent,
        "line_height ({}) should be >= ascent + descent ({})",
        m.line_height,
        m.ascent + m.descent
    );
}

// ---------------------------------------------------------------------------
// Phase 4 — widget system
// ---------------------------------------------------------------------------

/// Y-down → Y-up flip: a point at screen y=10 in a 100px viewport becomes y=90.
#[test]
fn test_y_flip_at_ingestion() {
    use crate::text::Font;
    use std::sync::Arc;

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
