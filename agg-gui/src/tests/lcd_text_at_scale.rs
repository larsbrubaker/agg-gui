//! LCD subpixel text is drawn at every effective scale once LCD is enabled,
//! not only up to 1.25: a 150% ClearType laptop gets subpixel text, as it does
//! in agg-sharp (which draws ClearType at any Windows scaling).  The rules
//! that always applied still hold at 1.5 - the destination must be opaque
//! (otherwise per-channel coverage collapses to one alpha) and an explicit
//! "LCD off" draws grayscale.
//!
//! Chroma is measured on black text over a white or transparent destination:
//! grayscale antialiasing keeps all three channels equal, subpixel coverage
//! does not.

use crate::device_scale::set_device_scale;
use crate::font_settings::{clear_lcd_enabled_override, set_lcd_enabled};
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::ux_scale::set_ux_scale;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::Label;
use crate::{Color, Rect, Size};
use std::sync::Arc;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

/// Largest R/G/B spread of any pixel: 0 for grayscale text, large on subpixel
/// glyph edges.
fn max_chroma(fb: &Framebuffer) -> u8 {
    fb.pixels()
        .chunks_exact(4)
        .filter(|p| p[3] > 0)
        .map(|p| p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2]))
        .max()
        .unwrap_or(0)
}

/// Paints a black "HHHH" `Label` at `device` x `ux` scale into a framebuffer
/// cleared to `background`, with LCD pinned to `lcd`, and returns the largest
/// channel spread and the ink-bearing pixel count.
fn paint_label(device: f64, ux: f64, lcd: bool, buffered: bool, background: Color) -> (u8, usize) {
    let _profile = crate::input_profile::profile_test_lock();
    set_device_scale(device);
    set_ux_scale(ux);
    set_lcd_enabled(lcd);

    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut label = Label::new("HHHH", font)
        .with_font_size(20.0)
        .with_color(Color::black())
        .with_has_backbuffer(buffered);
    let used = label.layout(Size::new(200.0, 100.0));
    label.set_bounds(Rect::new(0.0, 0.0, used.width, used.height));

    let scale = device * ux;
    let w = (used.width * scale).ceil() as u32 + 4;
    let h = (used.height * scale).ceil() as u32 + 4;
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(background);
        ctx.scale(scale, scale);
        paint_subtree(&mut label, &mut ctx);
    }

    clear_lcd_enabled_override();
    set_ux_scale(1.0);
    set_device_scale(1.0);

    let ink = fb
        .pixels()
        .chunks_exact(4)
        .filter(|p| p[3] > 0 && (p[0] as u32 + p[1] as u32 + p[2] as u32) < 3 * 128)
        .count();
    (max_chroma(&fb), ink)
}

/// Spread above which a pixel is subpixel coverage, well over rounding noise.
const SUBPIXEL_SPREAD: u8 = 24;

#[test]
fn buffered_label_is_subpixel_at_scale_1_5_when_lcd_is_enabled() {
    let (chroma, ink) = paint_label(1.5, 1.0, true, true, Color::white());
    assert!(ink > 0, "the label drew no ink");
    assert!(
        chroma > SUBPIXEL_SPREAD,
        "LCD on at 1.5x must draw subpixel text; max channel spread was {chroma}"
    );
}

/// An unbuffered label painted straight into an LCD target (what a software
/// LCD backbuffer is) at 1.5x.  The plain `GfxCtx` never draws subpixel text;
/// `LcdGfxCtx` is the target that does, and it must at any scale.
#[test]
fn direct_label_in_an_lcd_target_is_subpixel_at_scale_1_5_when_lcd_is_enabled() {
    use crate::draw_ctx::DrawCtx;
    use crate::lcd_coverage::LcdBuffer;
    use crate::lcd_gfx_ctx::LcdGfxCtx;

    let _profile = crate::input_profile::profile_test_lock();
    set_device_scale(1.5);
    set_lcd_enabled(true);
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut label = Label::new("HHHH", font)
        .with_font_size(20.0)
        .with_color(Color::black())
        .with_has_backbuffer(false);
    let used = label.layout(Size::new(200.0, 100.0));
    label.set_bounds(Rect::new(0.0, 0.0, used.width, used.height));
    let w = (used.width * 1.5).ceil() as u32 + 4;
    let h = (used.height * 1.5).ceil() as u32 + 4;
    let mut buffer = LcdBuffer::new(w, h);
    {
        let mut ctx = LcdGfxCtx::new(&mut buffer);
        ctx.clear(Color::white());
        ctx.scale(1.5, 1.5);
        paint_subtree(&mut label, &mut ctx);
    }
    clear_lcd_enabled_override();
    set_device_scale(1.0);

    let planes = buffer.color_plane_flipped();
    let darkest = planes.chunks_exact(3).map(|p| p[1]).min().unwrap_or(255);
    let chroma = planes
        .chunks_exact(3)
        .map(|p| p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2]))
        .max()
        .unwrap_or(0);
    assert!(darkest < 128, "the label drew no ink");
    assert!(
        chroma > SUBPIXEL_SPREAD,
        "LCD on at 1.5x must draw subpixel text; max channel spread was {chroma}"
    );
}

/// A UX zoom reaches the same effective scale as a device ratio and is treated
/// the same.
#[test]
fn buffered_label_is_subpixel_at_ux_zoom_1_5_when_lcd_is_enabled() {
    let (chroma, _) = paint_label(1.0, 1.5, true, true, Color::white());
    assert!(chroma > SUBPIXEL_SPREAD, "max channel spread was {chroma}");
}

/// And well above it, where the old gate refused too.
#[test]
fn buffered_label_is_subpixel_at_scale_2_when_lcd_is_enabled() {
    let (chroma, _) = paint_label(2.0, 1.0, true, true, Color::white());
    assert!(chroma > SUBPIXEL_SPREAD, "max channel spread was {chroma}");
}

#[test]
fn lcd_off_draws_grayscale_at_scale_1_5() {
    for buffered in [true, false] {
        let (chroma, ink) = paint_label(1.5, 1.0, false, buffered, Color::white());
        assert!(ink > 0, "the label drew no ink (buffered={buffered})");
        assert_eq!(
            chroma, 0,
            "LCD off must draw grayscale (buffered={buffered})"
        );
    }
}

/// The opaque-destination rule is unchanged at 1.5: over a transparent
/// destination the backbuffer's per-channel coverage collapses to one alpha,
/// so no pixel carries chroma.
#[test]
fn buffered_label_over_a_transparent_destination_collapses_at_scale_1_5() {
    let (chroma, ink) = paint_label(1.5, 1.0, true, true, Color::rgba(0.0, 0.0, 0.0, 0.0));
    assert!(ink > 0, "the label drew no ink");
    assert_eq!(
        chroma, 0,
        "LCD text over a transparent destination must collapse to grayscale"
    );
}
