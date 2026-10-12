//! Rust-only check of `ssaa.rs`'s composite: 2-D content drawn into a
//! transparent-cleared [`SsaaFramebuffer`] through the standard pipelines
//! (`BLEND_STANDARD`, colour `SrcAlpha / OneMinusSrcAlpha`) ends up
//! premultiplied in the texture, so the composite onto the target must blend
//! it `One / OneMinusSrcAlpha`. Blending it straight-alpha again multiplies
//! the colour by alpha twice and a translucent shape comes out washed out.
//! The expected colour is the software `GfxCtx` drawing the same shape
//! directly onto the background (`harness::software`).

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::geometry::Rect;

use super::harness::{at, diff, software};
use crate::layer_text_readback_tests::{try_device, Target};
use crate::{SsaaFramebuffer, WgpuGfxCtx};

const W: u32 = 64;
const H: u32 = 48;

/// Half-transparent red square, covering whole pixels so no edge AA is
/// involved in the comparison.
fn square(ctx: &mut dyn DrawCtx, scale: f64) {
    ctx.set_fill_color(Color::from_rgba8(255, 0, 0, 128));
    ctx.begin_path();
    ctx.rect(16.0 * scale, 12.0 * scale, 32.0 * scale, 24.0 * scale);
    ctx.fill();
}

/// The square drawn into a `factor`× SSAA target and composited with
/// [`SsaaFramebuffer::blit_downsample_to`] over a white target.
fn composite(factor: u32) -> Option<Vec<u8>> {
    let (device, queue) = try_device()?;
    let (fw, fh) = (W * factor, H * factor);
    let fb = SsaaFramebuffer::new(&device, fw, fh, wgpu::TextureFormat::Rgba8Unorm, false);
    let mut inner = WgpuGfxCtx::new(
        Arc::clone(&device),
        Arc::clone(&queue),
        wgpu::TextureFormat::Rgba8Unorm,
        fw as f32,
        fh as f32,
    );
    inner.reset(fw as f32, fh as f32);
    inner.clear(Color::transparent());
    square(&mut inner, factor as f64);
    inner.flush_to_surface(fb.render_view());

    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), W, H);
    let mut outer = WgpuGfxCtx::new(
        Arc::clone(&device),
        Arc::clone(&queue),
        wgpu::TextureFormat::Rgba8Unorm,
        W as f32,
        H as f32,
    );
    outer.reset(W as f32, H as f32);
    outer.clear(Color::white());
    outer.flush_to_surface(&target.view);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("ssaa_composite_test"),
    });
    fb.blit_downsample_to(
        &device,
        &mut encoder,
        &target.view,
        (W, H),
        Rect::new(0.0, 0.0, W as f64, H as f64),
        None,
        outer.pipelines(),
        factor,
    );
    queue.submit(Some(encoder.finish()));
    Some(target.read())
}

#[test]
fn rust_only_translucent_ssaa_content_composites_once() {
    let expected = software(W, H, Color::white(), &|ctx| square(ctx, 1.0));
    for factor in [1, 2, 3, 4] {
        let Some(gpu) = composite(factor) else {
            eprintln!("SKIP: no GPU adapter");
            return;
        };
        // Inside the square, outside it, both well clear of the edges.
        for (x, y) in [(32, 24), (20, 16), (4, 4), (60, 44)] {
            let g = at(&gpu, W, x, y);
            let s = at(&expected, W, x, y);
            assert!(
                diff(g, s) <= 2,
                "factor {factor}: pixel {x},{y} GPU {g:?} against software {s:?}"
            );
        }
    }
}
