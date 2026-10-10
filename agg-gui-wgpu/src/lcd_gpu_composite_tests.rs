//! agg-sharp `Tests/Agg.Tests/Agg.RenderGl/LcdGpuCompositeTests.cs` — the GPU
//! arms of the LCD composite: the three channel passes `WgpuGfxCtx` draws for
//! a two-plane backbuffer (`draw_lcd_backbuffer_arc`, the `LcbMask` command)
//! and for a single coverage mask (`draw_lcd_mask`, the `LcdMask` command).
//!
//! The C# tests record a fake GL context's command stream and replay GL's
//! specified blend arithmetic on the CPU, because nothing there has a live
//! device. Here the passes run on a real headless device and are read back,
//! so the comparison is against what the GPU actually blended — the same
//! expectations (byte identical to the software composite for the backbuffer
//! and an opaque mask, within one level for a translucent mask) against
//! agg-gui's software composites (`GfxCtx::draw_lcd_backbuffer_arc` /
//! `draw_lcd_mask`). The C# tests of the fixed-function GL machinery itself
//! (channel images, `glColorMask` / `glTexImage2D` recordings, the texture
//! cache stamp) have no wgpu counterpart: the channel split lives in the
//! `lcd_r` / `lcd_g` / `lcd_b` pipelines, and the upload cache is
//! `lcd_arc_cache_tests.rs`'s.
//!
//! Skipped (pass trivially) when no GPU adapter is available.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::{DrawCtx, FillRule};
use agg_gui::framebuffer::Framebuffer;
use agg_gui::gfx_ctx::GfxCtx;
use agg_gui::lcd_coverage::{build_bounded_mask, identity_xform, LcdBuffer, LcdMask};
use agg_rust::path_storage::PathStorage;

use crate::layer_text_readback_tests::{px, try_device, Target};
use crate::WgpuGfxCtx;

/// Surface the tests fill into; big enough to hold the fill with room around it.
const SURFACE_W: u32 = 24;
const SURFACE_H: u32 = 12;

/// An opaque mid gray destination — a neutral background where a per-channel
/// composite and a collapsed one visibly disagree.
fn mid_gray() -> Color {
    Color::from_rgb8(96, 112, 128)
}

/// A closed rectangle path, the plainest fill the LCD pipeline takes.
fn rectangle(left: f64, bottom: f64, right: f64, top: f64) -> PathStorage {
    let mut path = PathStorage::new();
    path.move_to(left, bottom);
    path.line_to(right, bottom);
    path.line_to(right, top);
    path.line_to(left, top);
    path.close_polygon(0);
    path
}

/// A buffer painted through the real LCD pipeline: a fractionally placed fill
/// on a transparent buffer, which leaves **both** planes diverging per channel
/// along the edges. Deliberately not cleared to an opaque background first,
/// which would leave the alpha plane uniform.
fn buffer_with_lcd_fill() -> LcdBuffer {
    let mut buffer = LcdBuffer::new(SURFACE_W, SURFACE_H);
    let mut path = rectangle(3.4, 2.7, 18.2, 8.35);
    buffer.fill_path(
        &mut path,
        Color::from_rgb8(220, 130, 40),
        &identity_xform(),
        None,
        FillRule::NonZero,
    );
    buffer
}

/// Fails unless the planes hold pixels whose three channels differ, without
/// which a composite that read the wrong channel would still be right.
fn require_per_channel_divergence(planes: &[&[u8]]) {
    for plane in planes {
        let diverges = plane.chunks_exact(3).any(|p| p[0] != p[1] || p[1] != p[2]);
        assert!(
            diverges,
            "a uniform plane cannot tell a per-channel composite from a single-alpha one"
        );
    }
}

/// Paint `draw` onto a mid-gray software framebuffer; Y-up RGBA.
fn software(draw: impl FnOnce(&mut GfxCtx)) -> Framebuffer {
    let mut fb = Framebuffer::new(SURFACE_W, SURFACE_H);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(mid_gray());
        draw(&mut ctx);
    }
    fb
}

/// Paint `draw` onto a mid-gray GPU target; top-row-first RGBA, or `None`
/// without an adapter.
fn gpu(draw: impl FnOnce(&mut WgpuGfxCtx)) -> Option<Vec<u8>> {
    let (device, queue) = try_device()?;
    let target = Target::new(
        Arc::clone(&device),
        Arc::clone(&queue),
        SURFACE_W,
        SURFACE_H,
    );
    let mut ctx = WgpuGfxCtx::new(
        device,
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        SURFACE_W as f32,
        SURFACE_H as f32,
    );
    ctx.reset(SURFACE_W as f32, SURFACE_H as f32);
    ctx.clear(mid_gray());
    draw(&mut ctx);
    ctx.flush_to_surface(&target.view);
    Some(target.read())
}

/// Compare the color channels of every pixel within `tolerance` levels. The
/// passes never write destination alpha, so alpha is not compared.
fn assert_matches(expected: &Framebuffer, got: &[u8], tolerance: i32, what: &str) {
    let pixels = expected.pixels();
    for y in 0..SURFACE_H {
        for x in 0..SURFACE_W {
            let i = ((y * SURFACE_W + x) * 4) as usize;
            let want = &pixels[i..i + 3];
            // Readback is top-row-first; the framebuffer is Y-up.
            let have = px(got, SURFACE_W, x, SURFACE_H - 1 - y);
            for c in 0..3 {
                let diff = (have[c] as i32 - want[c] as i32).abs();
                assert!(
                    diff <= tolerance,
                    "{what} at {x}, {y} channel {c}: GPU {have:?} against software {want:?}"
                );
            }
        }
    }
}

/// The three GPU passes must land the software per-channel backbuffer
/// composite's pixels exactly.
#[test]
fn three_pass_blend_reproduces_the_software_composite() {
    let buffer = buffer_with_lcd_fill();
    require_per_channel_divergence(&[buffer.color_plane(), buffer.alpha_plane()]);
    let color = Arc::new(buffer.color_plane_flipped());
    let alpha = Arc::new(buffer.alpha_plane_flipped());
    let (w, h) = (buffer.width(), buffer.height());

    let expected = software(|ctx| {
        ctx.draw_lcd_backbuffer_arc(&color, &alpha, 1, w, h, 0.0, 0.0, w as f64, h as f64);
    });
    let Some(got) = gpu(|ctx| {
        ctx.draw_lcd_backbuffer_arc(&color, &alpha, 1, w, h, 0.0, 0.0, w as f64, h as f64);
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_matches(&expected, &got, 0, "backbuffer composite");
}

/// The mask built for the C# fill, and its whole-pixel origin.
fn fill_mask() -> (LcdMask, i32, i32) {
    let mut path = rectangle(3.4, 2.7, 18.2, 8.35);
    let built = build_bounded_mask(
        SURFACE_W,
        SURFACE_H,
        &mut path,
        &identity_xform(),
        None,
        FillRule::NonZero,
    );
    built.expect("the fill rasterizes into a mask")
}

/// Draws one masked fill on the GPU and requires the result to match the
/// software mask composite within `tolerance` levels.
fn assert_mask_passes_match_software_composite(color: Color, tolerance: i32) {
    let (mask, origin_x, origin_y) = fill_mask();
    require_per_channel_divergence(&[&mask.data]);
    let (w, h) = (mask.width, mask.height);
    let (x, y) = (origin_x as f64, origin_y as f64);

    let expected = software(|ctx| ctx.draw_lcd_mask(&mask.data, w, h, color, x, y));
    let Some(got) = gpu(|ctx| ctx.draw_lcd_mask(&mask.data, w, h, color, x, y)) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_matches(&expected, &got, tolerance, "mask composite");
}

/// The mask arm's byte-exactness pin, for an opaque color.
#[test]
fn mask_passes_reproduce_the_software_composite() {
    assert_mask_passes_match_software_composite(Color::from_rgb8(220, 130, 40), 0);
}

/// The same comparison for a **translucent** color, within one byte level:
/// the color's alpha has to reach every channel's coverage, or the fill
/// would paint at roughly `1 / alpha` times its ink.
#[test]
fn mask_passes_match_the_software_composite_for_a_translucent_color() {
    assert_mask_passes_match_software_composite(Color::from_rgba8(220, 130, 40, 137), 1);
}

/// The wire-up, as a behaviour test: agg-gui widgets choose their backbuffer
/// mode themselves (`Widget::backbuffer_mode`, following the LCD setting)
/// rather than asking the destination, so what the GPU destination has to do
/// is report the capability and composite an `LcdCoverage` widget's planes
/// through the three-pass `LcbMask` — at a whole-pixel translation too — while
/// an `Rgba` widget stays on the image blit. (The C# unit-scale gate and the
/// device-less `Graphics2DGpu` have no counterpart: the wgpu blit snaps both
/// corners through the CTM, and a `WgpuGfxCtx` always has a device.)
#[test]
fn gpu_destination_engages_the_lcd_backbuffer_mode() {
    use agg_gui::event::{Event, EventResult};
    use agg_gui::geometry::{Rect, Size};
    use agg_gui::widget::{paint_subtree, BackbufferCache, BackbufferMode, Widget};

    struct ModeWidget {
        bounds: Rect,
        cache: BackbufferCache,
        mode: BackbufferMode,
        children: Vec<Box<dyn Widget>>,
    }
    impl Widget for ModeWidget {
        fn type_name(&self) -> &'static str {
            "ModeWidget"
        }
        fn bounds(&self) -> Rect {
            self.bounds
        }
        fn set_bounds(&mut self, b: Rect) {
            self.bounds = b;
        }
        fn children(&self) -> &[Box<dyn Widget>] {
            &self.children
        }
        fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
            &mut self.children
        }
        fn layout(&mut self, available: Size) -> Size {
            available
        }
        fn paint(&mut self, ctx: &mut dyn DrawCtx) {
            ctx.set_fill_color(Color::white());
            ctx.begin_path();
            ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
            ctx.fill();
            ctx.set_fill_color(Color::black());
            ctx.begin_path();
            ctx.rect(3.4, 2.7, 10.3, 4.6);
            ctx.fill();
        }
        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }
        fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
            Some(&mut self.cache)
        }
        fn backbuffer_mode(&self) -> BackbufferMode {
            self.mode
        }
    }

    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    // Paint a widget in `mode` onto a fresh GPU destination translated by
    // `offset`; count the `LcbMask` commands it queued (and whether any
    // flattened to a single pass).
    let lcb_masks = |mode: BackbufferMode, offset: (f64, f64)| {
        let mut ctx = WgpuGfxCtx::new(
            Arc::clone(&device),
            Arc::clone(&queue),
            wgpu::TextureFormat::Rgba8Unorm,
            64.0,
            32.0,
        );
        ctx.reset(64.0, 32.0);
        ctx.clear(Color::white());
        assert!(
            ctx.has_lcd_mask_composite(),
            "the GPU destination must report it can composite LCD coverage"
        );
        ctx.translate(offset.0, offset.1);
        let mut widget = ModeWidget {
            bounds: Rect::new(0.0, 0.0, 20.0, 10.0),
            cache: BackbufferCache::default(),
            mode,
            children: Vec::new(),
        };
        widget.cache.invalidate();
        paint_subtree(&mut widget, &mut ctx);
        ctx.commands
            .iter()
            .filter_map(|c| match c {
                crate::DrawCommand::LcbMask { flatten, .. } => Some(*flatten),
                _ => None,
            })
            .collect::<Vec<bool>>()
    };

    assert!(
        lcb_masks(BackbufferMode::Rgba, (0.0, 0.0)).is_empty(),
        "an Rgba backbuffer stays on the image blit"
    );
    assert_eq!(
        lcb_masks(BackbufferMode::LcdCoverage, (0.0, 0.0)),
        vec![false],
        "an LcdCoverage backbuffer composites through the three subpixel passes"
    );
    assert_eq!(
        lcb_masks(BackbufferMode::LcdCoverage, (3.0, 7.0)),
        vec![false],
        "a whole pixel translation is exactly what the composite places by"
    );
}
