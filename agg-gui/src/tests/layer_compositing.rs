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
