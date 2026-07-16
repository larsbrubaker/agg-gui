//! Software-renderer layer composite tests.
//!
//! Verify that `GfxCtx::push_layer` / `pop_layer` produce the expected
//! Porter-Duff `SrcOver` blend onto the parent framebuffer, that a
//! composited subtree lands at the correct physical rect under HiDPI
//! device scale, and that the backbuffer blit paths honor `global_alpha`.

use super::*;
use crate::draw_ctx::DrawCtx;
use std::sync::Arc;

#[test]
fn test_push_pop_layer_solid_composites_correctly() {
    let mut fb = Framebuffer::new(20, 20);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());

    ctx.push_layer(20.0, 20.0);
    ctx.set_fill_color(Color::rgba(1.0, 0.0, 0.0, 1.0));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, 20.0, 20.0);
    ctx.fill();
    ctx.pop_layer();

    drop(ctx);

    let center = sample(&fb, 10, 10);
    assert!(
        is_red(center),
        "After layer composite, centre must be red; got {center:?}"
    );
}

#[test]
fn test_push_pop_layer_alpha_blends_into_parent() {
    let mut fb = Framebuffer::new(20, 20);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());

    ctx.push_layer(20.0, 20.0);
    ctx.set_fill_color(Color::rgba(1.0, 0.0, 0.0, 0.5));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, 20.0, 20.0);
    ctx.fill();
    ctx.pop_layer();

    drop(ctx);

    let [r, g, b, _] = sample(&fb, 10, 10);
    assert!(r > 200, "Red channel must be high; got {r}");
    assert!(
        g > 80 && g < 200,
        "Green channel must be mid-tone (pink); got {g}"
    );
    assert!(
        b > 80 && b < 200,
        "Blue channel must be mid-tone (pink); got {b}"
    );
}

/// Regression: a compositing layer pushed while the CTM carries a HiDPI
/// device scale must render the subtree at *physical* resolution and land
/// at the correct physical rect.  Before the fix, `push_layer` reset the CTM
/// to identity and sized the layer fb in logical pixels, so a device-scale-2
/// subtree rendered at half size into the physical target.
#[test]
fn test_push_layer_preserves_device_scale_hidpi() {
    let mut fb = Framebuffer::new(40, 40);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());

    // Simulate a 2× HiDPI display: the CTM maps logical → physical pixels.
    ctx.scale(2.0, 2.0);
    // Logical 10×10 layer → must cover 20×20 physical pixels.
    ctx.push_layer(10.0, 10.0);
    ctx.set_fill_color(Color::rgba(1.0, 0.0, 0.0, 1.0));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, 10.0, 10.0);
    ctx.fill();
    ctx.pop_layer();

    drop(ctx);

    // Deep inside the physical extent (which must be 20×20, not 10×10).
    assert!(
        is_red(sample(&fb, 1, 1)),
        "near-origin must be red; got {:?}",
        sample(&fb, 1, 1)
    );
    assert!(
        is_red(sample(&fb, 18, 18)),
        "physical (18,18) inside the 20px layer must be red; got {:?}",
        sample(&fb, 18, 18)
    );
    // Just outside the 20px physical extent stays background white.
    assert!(
        is_white(sample(&fb, 30, 30)),
        "outside the physical extent must stay white; got {:?}",
        sample(&fb, 30, 30)
    );
}

/// Regression: a layer pushed at a **non-zero translated origin** under HiDPI
/// scale must land at the correct *physical* rect.  The origin-at-(0,0) case
/// cannot catch an origin off-by-scale bug (0·s == 0/s == 0); this case pins
/// the exact classic failure — origin recorded in logical units, or the buffer
/// placed at the wrong physical offset.
#[test]
fn test_push_layer_translated_origin_hidpi() {
    let mut fb = Framebuffer::new(40, 40);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());

    // Device scale 2, then a logical (5,5) translate.  Composition puts the
    // CTM origin at physical (10,10) with scale 2, so a logical 10×10 layer
    // must cover physical [10,30) in both axes.
    ctx.scale(2.0, 2.0);
    ctx.translate(5.0, 5.0);
    ctx.push_layer(10.0, 10.0);
    ctx.set_fill_color(Color::rgba(1.0, 0.0, 0.0, 1.0));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, 10.0, 10.0);
    ctx.fill();
    ctx.pop_layer();

    drop(ctx);

    // Inside the physical [10,30) rect.
    assert!(
        is_red(sample(&fb, 11, 11)),
        "just inside the physical origin (10,10) must be red; got {:?}",
        sample(&fb, 11, 11)
    );
    assert!(
        is_red(sample(&fb, 28, 28)),
        "near the far physical corner (30,30) must be red; got {:?}",
        sample(&fb, 28, 28)
    );
    // Before the physical origin: content must NOT have landed at the
    // logical origin (5,5) or the unscaled logical extent.
    assert!(
        is_white(sample(&fb, 6, 6)),
        "before the physical origin must stay white; got {:?}",
        sample(&fb, 6, 6)
    );
    // Beyond the physical extent.
    assert!(
        is_white(sample(&fb, 34, 34)),
        "beyond the physical extent must stay white; got {:?}",
        sample(&fb, 34, 34)
    );
}

/// Regression: a **fractional** device scale exercises the layer-buffer
/// `.ceil()` sizing against the blit origin.  At scale 1.5 a logical 10×10
/// layer needs a 15×15 physical buffer; placed at physical origin (6,6) it
/// must tile contiguously to [6,21) with no gap or overshoot at the boundary.
#[test]
fn test_push_layer_fractional_scale() {
    let mut fb = Framebuffer::new(40, 40);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());

    // Device scale 1.5, logical (4,4) translate → CTM origin physical (6,6),
    // scale 1.5.  Logical 10 × 1.5 = 15 physical px → content spans [6,21).
    ctx.scale(1.5, 1.5);
    ctx.translate(4.0, 4.0);
    ctx.push_layer(10.0, 10.0);
    ctx.set_fill_color(Color::rgba(1.0, 0.0, 0.0, 1.0));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, 10.0, 10.0);
    ctx.fill();
    ctx.pop_layer();

    drop(ctx);

    // Just inside the origin.
    assert!(
        is_red(sample(&fb, 7, 7)),
        "just inside physical origin (6,6) must be red; got {:?}",
        sample(&fb, 7, 7)
    );
    // Last covered row/col of the 15px extent (index 20; [6,21) exclusive).
    assert!(
        is_red(sample(&fb, 20, 20)),
        "boundary pixel (20,20) inside [6,21) must be red — no gap from ceil \
         sizing; got {:?}",
        sample(&fb, 20, 20)
    );
    // Before the origin and beyond the extent stay background.
    assert!(
        is_white(sample(&fb, 4, 4)),
        "before the physical origin must stay white; got {:?}",
        sample(&fb, 4, 4)
    );
    assert!(
        is_white(sample(&fb, 22, 22)),
        "beyond the [6,21) extent must stay white; got {:?}",
        sample(&fb, 22, 22)
    );
}

/// Regression: `draw_image_rgba` (the RGBA backbuffer blit lane) must honor
/// the active `global_alpha`.  Before the fix it composited at a hardcoded
/// alpha of 1.0, so backbuffered Labels/buttons inside a faded subtree stayed
/// fully opaque.
#[test]
fn test_backbuffer_rgba_blit_honors_global_alpha() {
    let mut fb = Framebuffer::new(10, 10);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());

    // Opaque red source image, top-row-first straight-alpha RGBA8.
    let mut data = Vec::with_capacity(10 * 10 * 4);
    for _ in 0..(10 * 10) {
        data.extend_from_slice(&[255, 0, 0, 255]);
    }
    let data = Arc::new(data);

    ctx.set_global_alpha(0.5);
    ctx.draw_image_rgba_arc(&data, 10, 10, 0.0, 0.0, 10.0, 10.0);

    drop(ctx);

    // Red at 50% over white → pink (mid-tone green/blue), not pure red.
    let [r, g, b, _] = sample(&fb, 5, 5);
    assert!(r > 200, "Red channel must stay high; got {r}");
    assert!(
        g > 80 && g < 200,
        "Green channel must be mid-tone (fade over white); got {g}"
    );
    assert!(
        b > 80 && b < 200,
        "Blue channel must be mid-tone (fade over white); got {b}"
    );
}

/// A minimal opacity-scope wrapper that mirrors the Widget Gallery's
/// `GalleryScope`: it requests a whole-subtree compositing layer at a fixed
/// alpha so its child renders into an offscreen buffer that is composited back
/// once at that alpha.
struct AlphaScope {
    bounds: crate::Rect,
    children: Vec<Box<dyn crate::widget::Widget>>,
    alpha: f64,
}

impl crate::widget::Widget for AlphaScope {
    fn type_name(&self) -> &'static str {
        "AlphaScope"
    }
    fn bounds(&self) -> crate::Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: crate::Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn crate::widget::Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn crate::widget::Widget>> {
        &mut self.children
    }
    fn layout(&mut self, avail: crate::Size) -> crate::Size {
        if let Some(c) = self.children.first_mut() {
            let s = c.layout(avail);
            c.set_bounds(crate::Rect::new(0.0, 0.0, s.width, s.height));
        }
        // Reserve the full extent so the compositing layer is sized correctly
        // (a zero-sized scope would collapse the layer to 1×1 and clip the child).
        self.bounds = crate::Rect::new(0.0, 0.0, avail.width, avail.height);
        avail
    }
    fn paint(&mut self, _ctx: &mut dyn crate::draw_ctx::DrawCtx) {}
    fn on_event(&mut self, _e: &crate::event::Event) -> crate::event::EventResult {
        crate::event::EventResult::Ignored
    }
    fn compositing_layer(&mut self) -> Option<crate::widget::CompositingLayer> {
        Some(crate::widget::CompositingLayer::new(0.0, 0.0, 0.0, 0.0, self.alpha))
    }
}

/// Regression for the Widget Gallery "Opacity" fade: LCD-backbuffered text
/// inside a compositing layer must fade by the layer alpha **exactly once**.
///
/// The bug this guards against is a double alpha application — where the
/// per-draw `global_alpha` multiply in `draw_lcd_backbuffer_arc` stacks on top
/// of the whole-layer composite alpha, giving text `alpha²` while vector shapes
/// get `alpha¹` (text renders visibly washed out relative to its widgets).
///
/// Contract: content painted inside an alpha compositing layer paints at full
/// opacity into the offscreen buffer (`global_alpha == 1.0` there); the layer
/// composite applies the alpha once on `pop_layer`.  Black text at layer
/// alpha 0.5 over white must land at mid-gray (~128), never ~191 (0.25).
#[test]
fn test_lcd_text_in_alpha_layer_fades_once() {
    use crate::widget::{paint_subtree, Widget};
    use crate::widgets::Label;
    use crate::{Rect, Size};

    crate::theme::set_visuals(crate::theme::Visuals::light());
    crate::font_settings::set_lcd_enabled(true);

    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());
    let mk_label = || {
        let mut l = Label::new("HHHH", Arc::clone(&font))
            .with_font_size(20.0)
            .with_color(Color::rgba(0.0, 0.0, 0.0, 1.0));
        let _ = l.layout(Size::new(80.0, 30.0));
        l.set_bounds(Rect::new(0.0, 0.0, 80.0, 30.0));
        l
    };

    // Faded render: label inside a 0.5 compositing layer.
    let mut scope = AlphaScope {
        bounds: Rect::default(),
        children: vec![Box::new(mk_label())],
        alpha: 0.5,
    };
    let _ = scope.layout(Size::new(80.0, 30.0));
    let mut fb = Framebuffer::new(80, 30);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());
    paint_subtree(&mut scope, &mut ctx);
    drop(ctx);

    // Full-opacity reference render for comparison.
    let mut label_ref = mk_label();
    let mut fb_ref = Framebuffer::new(80, 30);
    let mut ctx_ref = GfxCtx::new(&mut fb_ref);
    ctx_ref.clear(Color::white());
    paint_subtree(&mut label_ref, &mut ctx_ref);
    drop(ctx_ref);

    let mut darkest_faded = 255u8;
    let mut darkest_full = 255u8;
    for y in 0..30 {
        for x in 0..80 {
            darkest_faded = darkest_faded.min(sample(&fb, x, y)[0]);
            darkest_full = darkest_full.min(sample(&fb_ref, x, y)[0]);
        }
    }

    crate::font_settings::clear_lcd_enabled_override();

    // Sanity: full-opacity black text is near-black.
    assert!(
        darkest_full < 40,
        "full-opacity black text must be near-black; got {darkest_full}"
    );
    // Single alpha over white: 0.5 → ~128.  Double alpha (0.25) → ~191.
    assert!(
        (100..=150).contains(&darkest_faded),
        "text in a 0.5 alpha layer must fade ONCE (mid-gray ~128); got \
         {darkest_faded} (≥160 would indicate a double alpha application)"
    );
}

/// Regression: the two-plane `draw_lcd_backbuffer_arc` blit must honor the
/// active `global_alpha` so LCD-cached text inside a faded subtree fades with
/// the rest of the group.
#[test]
fn test_lcd_backbuffer_blit_honors_global_alpha() {
    let mut fb = Framebuffer::new(4, 4);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());

    // Fully-covered opaque red, top-row-first: premultiplied colour plane is
    // (255,0,0) and the per-channel alpha plane is (255,255,255).
    let mut color = Vec::with_capacity(4 * 4 * 3);
    let mut alpha = Vec::with_capacity(4 * 4 * 3);
    for _ in 0..(4 * 4) {
        color.extend_from_slice(&[255, 0, 0]);
        alpha.extend_from_slice(&[255, 255, 255]);
    }
    let color = Arc::new(color);
    let alpha = Arc::new(alpha);

    ctx.set_global_alpha(0.5);
    ctx.draw_lcd_backbuffer_arc(&color, &alpha, 4, 4, 0.0, 0.0, 4.0, 4.0);

    drop(ctx);

    // Red at 50% over white → pink.
    let [r, g, b, _] = sample(&fb, 2, 2);
    assert!(r > 200, "Red channel must stay high; got {r}");
    assert!(
        g > 80 && g < 200,
        "Green channel must be mid-tone (fade over white); got {g}"
    );
    assert!(
        b > 80 && b < 200,
        "Blue channel must be mid-tone (fade over white); got {b}"
    );
}
