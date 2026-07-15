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
