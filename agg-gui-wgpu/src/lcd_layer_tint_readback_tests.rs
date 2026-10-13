//! Headless GPU readback: a two-plane LCD backbuffer of **non-black** text
//! blitted inside a transparent layer collapses to a single grayscale-style
//! sample, as black text does (`layer_text_readback_tests`).
//!
//! The flatten pass (`lcb_flatten`) must emit the text's own colour at the
//! weighted coverage. The colour plane holds `text * a_i` per channel, so
//! taking it as the premultiplied sample keeps each channel's own coverage
//! and a 0.2-gray `Label` showed a channel spread of 29 inside the layer
//! (0 with LCD off). Over a white backdrop, `text * aa + white * (1 - aa)` is
//! the same in every channel, so any spread left is chroma that leaked
//! through the collapse.
//!
//! Skipped (pass trivially) when no GPU adapter is available.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::{DrawCtx, FillRule};
use agg_gui::lcd_coverage::{identity_xform, LcdBuffer};
use agg_rust::path_storage::PathStorage;

use crate::layer_text_readback_tests::{px, try_device, Target};
use crate::WgpuGfxCtx;

const W: u32 = 32;
const H: u32 = 16;

/// A fractionally placed rectangle of `ink` filled through the real LCD
/// pipeline onto a transparent buffer: its edges carry unequal per-channel
/// coverage in both planes.
fn tinted_buffer(ink: Color) -> LcdBuffer {
    let mut buffer = LcdBuffer::new(W, H);
    let mut path = PathStorage::new();
    path.move_to(3.4, 2.7);
    path.line_to(24.2, 2.7);
    path.line_to(24.2, 11.35);
    path.line_to(3.4, 11.35);
    path.close_polygon(0);
    buffer.fill_path(&mut path, ink, &identity_xform(), None, FillRule::NonZero);
    buffer
}

/// Blit `buffer` inside a `layer_alpha` layer over white and read the result
/// back (top row first), or `None` without an adapter.
fn blit_in_layer(buffer: &LcdBuffer, layer_alpha: f64) -> Option<Vec<u8>> {
    let (device, queue) = try_device()?;
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), W, H);
    let mut ctx = WgpuGfxCtx::new(
        device,
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        W as f32,
        H as f32,
    );
    ctx.reset(W as f32, H as f32);
    ctx.clear(Color::white());
    let color = Arc::new(buffer.color_plane_flipped());
    let alpha = Arc::new(buffer.alpha_plane_flipped());
    ctx.push_layer_with_alpha(W as f64, H as f64, layer_alpha);
    ctx.draw_lcd_backbuffer_arc(&color, &alpha, 1, W, H, 0.0, 0.0, W as f64, H as f64);
    ctx.pop_layer();
    ctx.flush_to_surface(&target.view);
    Some(target.read())
}

/// The widest gap between a pixel's channels, and how many pixels have
/// partial coverage (so the check proves something).
fn spread_and_edges(data: &[u8]) -> (u8, usize) {
    let mut spread = 0u8;
    let mut edges = 0;
    for y in 0..H {
        for x in 0..W {
            let [r, g, b, _] = px(data, W, x, y);
            spread = spread.max(r.max(g).max(b) - r.min(g).min(b));
            // Neither the white backdrop nor the fully covered interior.
            if r < 250 && r > 140 {
                edges += 1;
            }
        }
    }
    (spread, edges)
}

#[test]
fn dark_gray_lcd_text_in_a_layer_collapses_to_gray() {
    let buffer = tinted_buffer(Color::from_rgb8(51, 51, 51));
    let Some(data) = blit_in_layer(&buffer, 0.5) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let (spread, edges) = spread_and_edges(&data);
    assert!(edges > 0, "the fill has partially covered edge pixels");
    assert!(
        spread <= 3,
        "non-black text inside a layer must collapse to gray; channel spread {spread}"
    );
}

#[test]
fn coloured_lcd_text_in_a_layer_collapses_to_its_own_colour() {
    // The collapsed sample is the ink colour at the weighted coverage, so over
    // white a pixel is `ink * aa + 255 * (1 - aa)`; the colour plane's per-channel
    // coverage must not leak into it. A channel too faint to divide (the planes
    // are 8-bit) can only guess its colour, so on those edge pixels it may land
    // anywhere between that and the per-channel composite's own value.
    let ink = [255.0f64, 230.0, 40.0];
    let buffer = tinted_buffer(Color::from_rgb8(255, 230, 40));
    let Some(data) = blit_in_layer(&buffer, 1.0) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let (color, alpha) = (buffer.color_plane(), buffer.alpha_plane());
    let mut firm = 0;
    for y in 0..H {
        for x in 0..W {
            // The planes are Y-up, the readback top-row-first.
            let i = (((H - 1 - y) * W + x) * 3) as usize;
            let a = [alpha[i], alpha[i + 1], alpha[i + 2]];
            let aa = (0.2126 * a[0] as f64 + 0.7152 * a[1] as f64 + 0.0722 * a[2] as f64) / 255.0;
            let faint = a.iter().any(|&c| c > 0 && c < 64);
            if !faint && aa > 0.0 {
                firm += 1;
            }
            let got = px(&data, W, x, y);
            for c in 0..3 {
                let collapsed = ink[c] * aa + 255.0 * (1.0 - aa);
                let (lo, hi) = if faint {
                    let truth = color[i + c] as f64 + 255.0 - a[c] as f64;
                    (collapsed.min(truth) - 8.0, collapsed.max(truth) + 8.0)
                } else {
                    (collapsed - 3.0, collapsed + 3.0)
                };
                assert!(
                    (lo..=hi).contains(&(got[c] as f64)),
                    "pixel ({x},{y}) channel {c}: {got:?}, expected {collapsed:.1} for alpha {a:?}"
                );
            }
        }
    }
    assert!(firm > 0, "the fill has pixels whose channels all carry ink");
}
