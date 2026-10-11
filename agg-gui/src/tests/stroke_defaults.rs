//! Default stroke caps and joins on the software contexts (`GfxCtx`,
//! `LcdGfxCtx`): butt caps and miter joins, as agg-sharp's `StrokeMath`
//! defaults (and the wgpu backend's `WgpuGfxCtx`) have them, so a line ends
//! at its endpoint on every backend. Found by the ported
//! `Golden2DPrimitiveTests.Lines` (agg-gui-wgpu `golden/`).

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::lcd_coverage::LcdBuffer;
use crate::lcd_gfx_ctx::LcdGfxCtx;

/// A 6-wide horizontal stroke from x = 10 to x = 20 at y = 10.
fn stroke_line(ctx: &mut dyn DrawCtx) {
    ctx.clear(Color::white());
    ctx.set_stroke_color(Color::black());
    ctx.set_line_width(6.0);
    ctx.begin_path();
    ctx.move_to(10.0, 10.0);
    ctx.line_to(20.0, 10.0);
    ctx.stroke();
}

#[test]
fn rust_only_software_stroke_defaults_to_butt_caps() {
    let mut fb = Framebuffer::new(32, 20);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        stroke_line(&mut ctx);
    }
    let px = |x: u32, y: u32| fb.pixels()[((y * 32 + x) * 4) as usize];
    assert_eq!(px(15, 10), 0, "the stroke's body is painted");
    // A round or square cap would reach 3 px past each endpoint.
    assert_eq!(px(8, 10), 255, "nothing is painted before the start point");
    assert_eq!(px(21, 10), 255, "nothing is painted past the end point");
}

#[test]
fn rust_only_lcd_stroke_defaults_to_butt_caps() {
    let mut buf = LcdBuffer::new(32, 20);
    {
        let mut ctx = LcdGfxCtx::new(&mut buf);
        stroke_line(&mut ctx);
    }
    let rgb = buf.color_plane();
    let px = |x: u32, y: u32| rgb[((y * 32 + x) * 3) as usize];
    assert_eq!(px(15, 10), 0, "the stroke's body is painted");
    assert_eq!(px(8, 10), 255, "nothing is painted before the start point");
    assert_eq!(px(21, 10), 255, "nothing is painted past the end point");
}
