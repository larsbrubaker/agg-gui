//! Software blits placed at an enormous offset must cull, not overflow.
//!
//! A backbuffered widget scrolled far off-screen (or measured inside a
//! `ScrollView`'s `f64::MAX / 2` content height) reaches `GfxCtx`'s pixel
//! blits with a translation far outside `i32`. The `as i32` conversion
//! saturates, and adding the row index to the saturated origin panicked with
//! "attempt to add with overflow". These draw the LCD backbuffer, the LCD
//! mask, and a composited layer at huge offsets and check that nothing panics
//! and nothing lands on the framebuffer.

use super::*;
use crate::draw_ctx::DrawCtx;
use std::sync::Arc;

const OFFSETS: [(f64, f64); 6] = [
    (0.0, 1.0e12),
    (0.0, -1.0e12),
    (1.0e12, 0.0),
    (-1.0e12, 0.0),
    (0.0, f64::MAX / 2.0),
    (f64::MAX / 2.0, f64::MAX / 2.0),
];

/// Assert the 8x8 framebuffer is still the white it was cleared to.
fn assert_untouched(fb: &Framebuffer, what: &str, offset: (f64, f64)) {
    for y in 0..8 {
        for x in 0..8 {
            let px = sample(fb, x, y);
            assert_eq!(
                px,
                [255, 255, 255, 255],
                "{what} at {offset:?} painted ({x},{y})"
            );
        }
    }
}

/// Opaque red LCD planes (`w * h * 3` bytes each).
fn red_planes(w: usize, h: usize) -> (Arc<Vec<u8>>, Arc<Vec<u8>>) {
    let color = [255u8, 0, 0].repeat(w * h);
    let alpha = vec![255u8; w * h * 3];
    (Arc::new(color), Arc::new(alpha))
}

#[test]
fn lcd_backbuffer_blit_at_a_huge_offset_culls() {
    let (color, alpha) = red_planes(4, 4);
    for offset in OFFSETS {
        let mut fb = Framebuffer::new(8, 8);
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        ctx.translate(offset.0, offset.1);
        ctx.draw_lcd_backbuffer_arc(&color, &alpha, 0, 4, 4, 0.0, 0.0, 4.0, 4.0);
        // The same blit with the offset in the destination coordinates.
        ctx.translate(-offset.0, -offset.1);
        ctx.draw_lcd_backbuffer_arc(&color, &alpha, 0, 4, 4, offset.0, offset.1, 4.0, 4.0);
        drop(ctx);
        assert_untouched(&fb, "LCD backbuffer", offset);
    }
}

#[test]
fn lcd_mask_at_a_huge_offset_culls() {
    let mask = vec![255u8; 4 * 4 * 3];
    for offset in OFFSETS {
        let mut fb = Framebuffer::new(8, 8);
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        ctx.draw_lcd_mask(&mask, 4, 4, Color::rgb(1.0, 0.0, 0.0), offset.0, offset.1);
        drop(ctx);
        assert_untouched(&fb, "LCD mask", offset);
    }
}

#[test]
fn layer_composited_at_a_huge_offset_culls() {
    for offset in OFFSETS {
        let mut fb = Framebuffer::new(8, 8);
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        ctx.translate(offset.0, offset.1);
        ctx.push_layer(4.0, 4.0);
        ctx.set_fill_color(Color::rgb(1.0, 0.0, 0.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 4.0, 4.0);
        ctx.fill();
        ctx.pop_layer();
        drop(ctx);
        assert_untouched(&fb, "layer", offset);
    }
}

/// The cull must not reject an in-range blit: a sanity check that the
/// LCD backbuffer still lands where it is drawn.
#[test]
fn lcd_backbuffer_blit_in_range_still_paints() {
    let (color, alpha) = red_planes(4, 4);
    let mut fb = Framebuffer::new(8, 8);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::white());
    ctx.draw_lcd_backbuffer_arc(&color, &alpha, 0, 4, 4, 2.0, 2.0, 4.0, 4.0);
    drop(ctx);
    let [r, g, b, _] = sample(&fb, 3, 3);
    assert!(
        r > 200 && g < 50 && b < 50,
        "expected red at (3,3); got {r},{g},{b}"
    );
    assert_eq!(sample(&fb, 0, 0), [255, 255, 255, 255]);
}
