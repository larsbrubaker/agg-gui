//! Headless GPU readback tests for `DrawCtx::save()`/`restore()` on the wgpu
//! backend: a restore brings back the whole paint state the software
//! `GfxCtx` saves (`saved_paint_state.rs`), so a
//! `save(); set_global_alpha(..); draw; restore()` block — the way a menu
//! fades a disabled row's icon — leaves later draws opaque.
//!
//! Skipped (pass trivially) when no GPU adapter is available.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::framebuffer::Framebuffer;
use agg_gui::gfx_ctx::GfxCtx;

use crate::layer_text_readback_tests::{px, try_device, Target};
use crate::WgpuGfxCtx;

const W: u32 = 48;
const H: u32 = 64;

fn red() -> Color {
    Color::from_rgba8(255, 0, 0, 255)
}

/// Run `draw` on a white `W`x`H` GPU context; the top-row-first readback.
fn gpu(draw: impl FnOnce(&mut dyn DrawCtx)) -> Option<Vec<u8>> {
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
    draw(&mut ctx);
    ctx.flush_to_surface(&target.view);
    Some(target.read())
}

fn fill_rect(ctx: &mut dyn DrawCtx, x: f64, y: f64, w: f64, h: f64) {
    ctx.begin_path();
    ctx.rect(x, y, w, h);
    ctx.fill();
}

#[test]
fn restore_brings_back_global_alpha() {
    let Some(data) = gpu(|ctx| {
        ctx.save();
        ctx.set_global_alpha(0.3);
        ctx.restore();
        ctx.set_fill_color(red());
        fill_rect(ctx, 8.0, 8.0, 32.0, 32.0);
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_eq!(px(&data, W, 24, H - 24), [255, 0, 0, 255]);
}

/// The fill colour, blend mode and alpha set inside a save/restore block are
/// all undone, as in the software `GfxCtx`.
#[test]
fn restore_brings_back_fill_color_and_blend_mode() {
    let Some(data) = gpu(|ctx| {
        ctx.set_fill_color(red());
        ctx.save();
        ctx.set_fill_color(Color::from_rgba8(0, 0, 255, 255));
        ctx.set_global_alpha(0.5);
        ctx.set_blend_mode(agg_gui::CompOp::Clear);
        ctx.restore();
        fill_rect(ctx, 8.0, 8.0, 32.0, 32.0);
    }) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    assert_eq!(px(&data, W, 24, H - 24), [255, 0, 0, 255]);
}

/// Three menu-like rows, the middle one faded as a disabled row's icon is
/// (`popup_paint.rs`): the GPU frame matches the software frame pixel for
/// pixel, so the rows after the faded one are not faded.
fn draw_rows(ctx: &mut dyn DrawCtx) {
    for row in 0..3 {
        let y = (H - 20 * (row + 1)) as f64;
        let disabled = row == 1;
        if disabled {
            ctx.save();
            ctx.set_global_alpha(0.3);
        }
        ctx.set_fill_color(red());
        fill_rect(ctx, 4.0, y + 2.0, 16.0, 16.0);
        if disabled {
            ctx.restore();
        }
        ctx.set_fill_color(Color::from_rgba8(30, 60, 200, 255));
        fill_rect(ctx, 24.0, y + 2.0, 20.0, 16.0);
    }
}

#[test]
fn rows_after_a_faded_row_match_software() {
    let mut fb = Framebuffer::new(W, H);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        draw_rows(&mut ctx);
    }
    let Some(got) = gpu(draw_rows) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let want = fb.pixels();
    let mut off = Vec::new();
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 4) as usize;
            // Readback is top-row-first; the framebuffer is Y-up.
            let have = px(&got, W, x, H - 1 - y);
            if (0..4).any(|c| (have[c] as i32 - want[i + c] as i32).abs() > 1) {
                off.push(format!(
                    "({x}, {y}): GPU {have:?} vs software {:?}",
                    &want[i..i + 4]
                ));
            }
        }
    }
    assert!(
        off.is_empty(),
        "{} pixel(s) off: {:#?}",
        off.len(),
        &off[..off.len().min(10)]
    );
    // The last row's icon is opaque red on both.
    let last = px(&got, W, 12, H - 1 - 10);
    assert_eq!(last, [255, 0, 0, 255]);
}
