//! Headless GPU readback: LCD subpixel text through the real [`WgpuGfxCtx`]
//! pipeline is drawn at a CTM scale of 1.5 (a 150% Windows display), not only
//! up to 1.25, and the opaque-destination rule still collapses it to grayscale
//! over a transparent layer.  Chroma is the R/G/B spread of black text on
//! white: subpixel coverage has it on glyph edges, grayscale never does.
//!
//! Skipped (passes trivially) when no GPU adapter is available, like the
//! sibling `layer_text_readback_tests`.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::text::Font;

use crate::layer_text_readback_tests::{px, try_device, Target};
use crate::WgpuGfxCtx;

const TEST_FONT: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");

/// Largest channel spread over the frame after painting black "HHHH" under
/// `scale(scale)`, optionally inside a still-transparent layer.
fn text_chroma(scale: f64, lcd: bool, in_transparent_layer: bool) -> Option<u8> {
    let (device, queue) = try_device()?;
    let (w, h) = (96u32, 48u32);
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), w, h);
    let mut ctx = WgpuGfxCtx::new(
        Arc::clone(&device),
        Arc::clone(&queue),
        wgpu::TextureFormat::Rgba8Unorm,
        w as f32,
        h as f32,
    );
    ctx.reset(w as f32, h as f32);
    ctx.set_lcd_mode(lcd);
    ctx.clear(Color::white());
    ctx.set_font(Arc::new(Font::from_slice(TEST_FONT).unwrap()));
    ctx.set_font_size(18.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 1.0));
    ctx.scale(scale, scale);
    if in_transparent_layer {
        ctx.push_layer_with_alpha(w as f64 / scale, h as f64 / scale, 1.0);
    }
    ctx.fill_text("HHHH", 4.0, 10.0);
    if in_transparent_layer {
        ctx.pop_layer();
    }
    ctx.flush_to_surface(&target.view);

    let data = target.read();
    let mut spread = 0u8;
    let mut darkest = u16::MAX;
    for y in 0..h {
        for x in 0..w {
            let p = px(&data, w, x, y);
            darkest = darkest.min(p[0] as u16 + p[1] as u16 + p[2] as u16);
            spread = spread.max(p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2]));
        }
    }
    assert!(
        darkest < 300,
        "the text drew no ink (darkest sum {darkest})"
    );
    Some(spread)
}

#[test]
fn lcd_text_is_subpixel_at_ctm_scale_1_5() {
    let Some(spread) = text_chroma(1.5, true, false) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert!(
        spread > 24,
        "LCD text at 1.5x must be subpixel; max channel spread was {spread}"
    );
}

#[test]
fn lcd_off_is_grayscale_at_ctm_scale_1_5() {
    let Some(spread) = text_chroma(1.5, false, false) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_eq!(spread, 0, "LCD off must keep equal channels");
}

#[test]
fn lcd_text_in_a_transparent_layer_stays_grayscale_at_ctm_scale_1_5() {
    let Some(spread) = text_chroma(1.5, true, true) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert!(
        spread <= 4,
        "over a still-transparent layer the text must collapse to grayscale; spread {spread}"
    );
}
