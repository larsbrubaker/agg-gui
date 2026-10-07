//! Regression test: a CPU-backbuffered `Label` painted through the software
//! `GfxCtx` under a 2x scale transform must render its text at 2x ink size —
//! the backbuffer is rasterised at physical resolution and blitted 1:1.
//!
//! Companion to [`backbuffer_scale`](super::backbuffer_scale) (bitmap sizing)
//! and [`menu_hidpi_scale`](super::menu_hidpi_scale) (menu-bar re-raster);
//! this one measures the actual glyph ink extent of a buffered Label against
//! the unbuffered direct-paint path, in both grayscale and LCD modes.

use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::Label;
use crate::{Color, Rect, Size};
use std::sync::Arc;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

/// Lay out a 20 px "HHHH" label, paint it into a framebuffer through
/// `ctx.scale(scale)`, and return the (min, max) Y-up rows containing ink.
fn ink_rows(buffered: bool, scale: f64) -> (u32, u32) {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut label = Label::new("HHHH", font)
        .with_font_size(20.0)
        .with_color(Color::black())
        .with_has_backbuffer(buffered);
    let used = label.layout(Size::new(200.0, 100.0));
    label.set_bounds(Rect::new(0.0, 0.0, used.width, used.height));

    let w = (used.width * scale).ceil() as u32 + 4;
    let h = (used.height * scale).ceil() as u32 + 4;
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        ctx.scale(scale, scale);
        paint_subtree(&mut label, &mut ctx);
    }
    let px = fb.pixels();
    let (mut lo, mut hi) = (u32::MAX, 0u32);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            if (px[i] as u32 + px[i + 1] as u32 + px[i + 2] as u32) < 3 * 128 {
                lo = lo.min(y);
                hi = hi.max(y);
            }
        }
    }
    assert!(
        lo <= hi,
        "label (buffered={buffered}, scale={scale}) drew no ink"
    );
    (lo, hi)
}

fn check_mode(lcd: bool) {
    let _profile = crate::input_profile::profile_test_lock();
    crate::font_settings::set_lcd_enabled(lcd);
    crate::device_scale::set_device_scale(1.0);

    let direct_1x = ink_rows(false, 1.0);
    let direct_2x = ink_rows(false, 2.0);
    let buffered_1x = ink_rows(true, 1.0);
    let buffered_2x = ink_rows(true, 2.0);

    crate::font_settings::clear_lcd_enabled_override();

    let span = |r: (u32, u32)| (r.1 - r.0 + 1) as f64;
    let ratio = span(buffered_2x) / span(buffered_1x);
    assert!(
        (ratio - 2.0).abs() < 0.25,
        "lcd={lcd}: buffered Label ink at 2x must be twice 1x: \
         1x rows {buffered_1x:?}, 2x rows {buffered_2x:?} (direct 1x {direct_1x:?}, \
         direct 2x {direct_2x:?})"
    );
    // Same placement and size as the unbuffered direct-paint path.
    assert!(
        (span(buffered_2x) - span(direct_2x)).abs() <= 2.0
            && (buffered_2x.0 as i64 - direct_2x.0 as i64).abs() <= 2,
        "lcd={lcd}: buffered 2x rows {buffered_2x:?} must match direct 2x rows {direct_2x:?}"
    );
}

#[test]
fn buffered_label_ink_doubles_at_scale_2_grayscale() {
    check_mode(false);
}

#[test]
fn buffered_label_ink_doubles_at_scale_2_lcd() {
    check_mode(true);
}

/// The blit every CPU backbuffer uses: a logical dst rect under
/// `translate + scale(2)` must cover exactly its device-pixel footprint,
/// 1:1 with a 2x-rasterised source image.
#[test]
fn draw_image_rgba_maps_dst_rect_through_ctm_scale() {
    use crate::DrawCtx;
    // 4x4 opaque red image, logical dst 2x2 at (1, 1); CTM = translate(3, 0)
    // then scale(2) ⇒ device rect x ∈ [5, 9), y ∈ [2, 6).
    let img = vec![255u8, 0, 0, 255].repeat(16);
    let mut fb = Framebuffer::new(16, 16);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        ctx.translate(3.0, 0.0);
        ctx.scale(2.0, 2.0);
        ctx.draw_image_rgba(&img, 4, 4, 1.0, 1.0, 2.0, 2.0);
    }
    let px = fb.pixels();
    for y in 0..16u32 {
        for x in 0..16u32 {
            let i = ((y * 16 + x) * 4) as usize;
            let red = px[i] > 200 && px[i + 1] < 50;
            let inside = (5..9).contains(&x) && (2..6).contains(&y);
            assert_eq!(red, inside, "pixel ({x}, {y}) red={red}, expected {inside}");
        }
    }
}
