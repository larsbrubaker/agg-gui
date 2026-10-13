//! `GfxCtx::draw_lcd_backbuffer_arc` composites per channel only onto opaque
//! pixels, as agg-sharp's `ImageGraphics2D.CompositeLcdBuffer` does
//! (`IsOpaqueUnder`): one alpha per pixel cannot carry three channel
//! coverages, so a non-opaque destination (a transparent compositing layer, a
//! half-faded backbuffer) takes the collapsed single-alpha draw
//! ([`crate::lcd_coverage::collapse_lcd_pixel`]) and keeps premultiplied
//! consistency for the later composite onto its real background.
//!
//! Two tests port `SvgIconLcdTests`' LCD-buffer cases onto agg-gui's
//! backbuffer (a transparent compositing layer stands in for C#'s
//! straight-held `ImageBuffer`); the rest pin the gate's edges.

use super::*;
use crate::draw_ctx::DrawCtx;
use crate::lcd_coverage::LcdBuffer;
use crate::lcd_gfx_ctx::LcdGfxCtx;
use std::sync::Arc;

const CANVAS_SIZE: u32 = 48;

/// The LCD buffer of `SvgIconLcdTests`: an ellipse (24, 24, 15.3, 11.7) of
/// `ink` over `background`, returned as top-row-first planes.
fn ellipse_planes(background: Color, ink: Color) -> (Arc<Vec<u8>>, Arc<Vec<u8>>, LcdBuffer) {
    let mut buffer = LcdBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
    {
        let mut g = LcdGfxCtx::new(&mut buffer);
        g.clear(background);
        g.set_fill_color(ink);
        g.translate(24.0, 24.0);
        g.scale(15.3, 11.7);
        g.begin_path();
        g.circle(0.0, 0.0, 1.0);
        g.fill();
    }
    let color = Arc::new(buffer.color_plane_flipped());
    let alpha = Arc::new(buffer.alpha_plane_flipped());
    (color, alpha, buffer)
}

fn blit(ctx: &mut GfxCtx, color: &Arc<Vec<u8>>, alpha: &Arc<Vec<u8>>) {
    let n = CANVAS_SIZE;
    ctx.draw_lcd_backbuffer_arc(color, alpha, 0, n, n, 0.0, 0.0, n as f64, n as f64);
}

fn worst_channel_difference(a: &Framebuffer, b: &Framebuffer) -> i32 {
    a.pixels()
        .iter()
        .zip(b.pixels())
        .map(|(x, y)| (*x as i32 - *y as i32).abs())
        .max()
        .unwrap_or(0)
}

fn surface(color: Color) -> Framebuffer {
    let mut fb = Framebuffer::new(CANVAS_SIZE, CANVAS_SIZE);
    GfxCtx::new(&mut fb).clear(color);
    fb
}

/// The reproduction: black LCD ink over transparent pixels.  The per-channel
/// composite wrote the strongest channel's alpha into the pixel (255 where
/// the pixel's luminance coverage is 146), so the edge came out far too dark
/// once the buffer was composited onto its background.
#[test]
fn an_lcd_buffer_over_transparent_pixels_stores_the_collapsed_alpha() {
    // One pixel, channel coverages 255 / 128 / 0 of black ink.
    let color = Arc::new(vec![0u8, 0, 0]);
    let alpha = Arc::new(vec![255u8, 128, 0]);
    let mut fb = Framebuffer::new(4, 4);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.draw_lcd_backbuffer_arc(&color, &alpha, 0, 1, 1, 1.0, 1.0, 1.0, 1.0);
    }
    // collapse_lcd_pixel: weighted alpha 0.2126 * 255 + 0.7152 * 128 = 145.8.
    assert_eq!(sample(&fb, 1, 1), [0, 0, 0, 146]);
}

/// Port of `AnLcdBufferInAStraightBackbufferCompositesLikeItsCollapsedDraw`:
/// the LCD buffer drawn into a transparent compositing layer (C#'s
/// straight-held backbuffer) and the layer composited onto an opaque surface
/// match the buffer's collapsed draw straight onto that surface.
#[test]
fn an_lcd_buffer_in_a_straight_backbuffer_composites_like_its_collapsed_draw() {
    let (color, alpha, buffer) = ellipse_planes(Color::rgba(0.0, 0.0, 0.0, 0.0), {
        Color::rgb(1.0, 230.0 / 255.0, 40.0 / 255.0)
    });

    let mut buffered = surface(Color::black());
    {
        let mut ctx = GfxCtx::new(&mut buffered);
        ctx.push_layer(CANVAS_SIZE as f64, CANVAS_SIZE as f64);
        blit(&mut ctx, &color, &alpha);
        ctx.pop_layer();
    }

    let mut direct = surface(Color::black());
    {
        let collapsed = buffer.to_rgba8_top_down_collapsed();
        let mut ctx = GfxCtx::new(&mut direct);
        let n = CANVAS_SIZE;
        ctx.draw_image_rgba(&collapsed, n, n, 0.0, 0.0, n as f64, n as f64);
    }

    assert!(
        buffered.pixels() != surface(Color::black()).pixels(),
        "the ellipse has to have painted, or the comparison proves nothing"
    );
    let worst = worst_channel_difference(&buffered, &direct);
    assert!(worst <= 2, "worst channel difference {worst}");
}

/// The same composite with dark ink onto a light surface, where the
/// per-channel composite's inflated edge alpha shows most.
#[test]
fn rust_only_a_dark_lcd_buffer_in_a_transparent_layer_composites_like_its_collapsed_draw() {
    let (color, alpha, buffer) = ellipse_planes(Color::rgba(0.0, 0.0, 0.0, 0.0), Color::black());

    let mut buffered = surface(Color::white());
    {
        let mut ctx = GfxCtx::new(&mut buffered);
        ctx.push_layer(CANVAS_SIZE as f64, CANVAS_SIZE as f64);
        blit(&mut ctx, &color, &alpha);
        ctx.pop_layer();
    }

    let mut direct = surface(Color::white());
    {
        let collapsed = buffer.to_rgba8_top_down_collapsed();
        let mut ctx = GfxCtx::new(&mut direct);
        let n = CANVAS_SIZE;
        ctx.draw_image_rgba(&collapsed, n, n, 0.0, 0.0, n as f64, n as f64);
    }

    assert!(buffered.pixels() != surface(Color::white()).pixels());
    let worst = worst_channel_difference(&buffered, &direct);
    assert!(worst <= 2, "worst channel difference {worst}");
}

/// Port of `AnLcdBufferOpaqueInsideTheClipKeepsItsChroma`: the opacity check
/// covers only the pixels the clip leaves, so a destination opaque inside the
/// clip but transparent outside it keeps the per-channel composite, and
/// nothing lands outside the clip.
#[test]
fn an_lcd_buffer_opaque_inside_the_clip_keeps_its_chroma() {
    let (color, alpha, _) = ellipse_planes(Color::white(), Color::black());
    let half = CANVAS_SIZE / 2;
    let mut destination = Framebuffer::new(CANVAS_SIZE, CANVAS_SIZE);
    {
        let mut ctx = GfxCtx::new(&mut destination);
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, half as f64, CANVAS_SIZE as f64);
        ctx.fill();
        ctx.clip_rect(0.0, 0.0, half as f64, CANVAS_SIZE as f64);
        blit(&mut ctx, &color, &alpha);
    }
    let chroma = (0..CANVAS_SIZE)
        .flat_map(|y| (0..CANVAS_SIZE).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = sample(&destination, x, y);
            p[0] != p[2]
        })
        .count();
    assert!(
        chroma > 0,
        "pixels outside the clip are never touched, so their transparency must not refuse the LCD path"
    );
    assert_eq!(
        sample(&destination, CANVAS_SIZE - 1, half)[3],
        0,
        "nothing lands outside the clip"
    );
}

/// The gate decides on the destination, so an opaque one still takes the
/// per-channel composite: the same planes keep their chroma on white.
#[test]
fn an_lcd_buffer_over_opaque_pixels_keeps_its_chroma() {
    let color = Arc::new(vec![0u8, 0, 0]);
    let alpha = Arc::new(vec![255u8, 128, 0]);
    let mut fb = surface(Color::white());
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.draw_lcd_backbuffer_arc(&color, &alpha, 0, 1, 1, 1.0, 1.0, 1.0, 1.0);
    }
    // Per channel: white * (1 - a_c) = 0, 127, 255.
    assert_eq!(sample(&fb, 1, 1), [0, 127, 255, 255]);
}

/// One non-opaque pixel under the footprint is enough to refuse the
/// per-channel composite for the whole buffer.
#[test]
fn one_non_opaque_pixel_under_the_footprint_collapses_the_whole_draw() {
    let color = Arc::new(vec![0u8; 2 * 3]);
    let alpha = Arc::new(vec![255u8, 128, 0, 255, 128, 0]);
    let mut fb = surface(Color::white());
    // Punch a transparent hole under the second source pixel.
    let hole = ((fb.width() + 2) * 4) as usize;
    fb.pixels_mut()[hole..hole + 4].copy_from_slice(&[0, 0, 0, 0]);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.draw_lcd_backbuffer_arc(&color, &alpha, 0, 2, 1, 1.0, 1.0, 2.0, 1.0);
    }
    // The first pixel is on opaque white but still takes the collapsed
    // straight-over draw: black at alpha 146 over white is 255 - 146 = 109.
    assert_eq!(sample(&fb, 1, 1), [109, 109, 109, 255]);
}

/// A composite the destination or the clip removes entirely draws nothing.
#[test]
fn a_composite_the_clip_removes_draws_nothing() {
    let color = Arc::new(vec![0u8; 3]);
    let alpha = Arc::new(vec![255u8; 3]);
    let mut fb = Framebuffer::new(4, 4);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clip_rect(2.0, 2.0, 2.0, 2.0);
        ctx.draw_lcd_backbuffer_arc(&color, &alpha, 0, 1, 1, 0.0, 0.0, 1.0, 1.0);
    }
    assert!(fb.pixels().iter().all(|&b| b == 0));
}
