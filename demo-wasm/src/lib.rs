//! WASM demo crate for agg-gui.
//!
//! Exports `render_frame(width, height) -> Vec<u8>` which renders the Phase 1
//! demo and returns pixels in **top-down (Y-down)** order, ready for JS
//! `CanvasRenderingContext2D.putImageData`.
//!
//! Internally the framebuffer uses bottom-up (Y-up) layout. The flip is
//! applied once via `Framebuffer::pixels_flipped()` before returning.

use wasm_bindgen::prelude::*;
use agg_gui::{Color, Framebuffer, GfxCtx};

/// Render the agg-gui Phase 1 demo into an RGBA pixel buffer.
///
/// Returns `width * height * 4` bytes in top-down row order (ready for
/// `putImageData`). The internal framebuffer is Y-up; a Y-flip is applied
/// on output so the canvas displays correctly.
#[wasm_bindgen]
pub fn render_frame(width: u32, height: u32) -> Vec<u8> {
    let mut fb = Framebuffer::new(width, height);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        draw_phase1_demo(&mut ctx, width, height);
    }
    // Flip to top-down for JS canvas (putImageData treats row 0 as the top)
    fb.pixels_flipped()
}

/// Draw the Phase 1 demo scene.
///
/// Demonstrates:
/// 1. Coordinate system — shapes positioned in Y-up space
/// 2. CCW rotation — arrow rotated +90° should point UP
/// 3. Basic primitives — circles, rectangles, stroked paths
pub fn draw_phase1_demo(ctx: &mut GfxCtx, width: u32, height: u32) {
    let w = width as f64;
    let h = height as f64;
    let cx = w / 2.0;
    let cy = h / 2.0;

    // --- Background ---
    ctx.clear(Color::rgb(0.12, 0.12, 0.14));

    // --- Coordinate grid (subtle) ---
    ctx.set_stroke_color(Color::rgba(1.0, 1.0, 1.0, 0.08));
    ctx.set_line_width(1.0);
    for i in 1..8 {
        let x = w * i as f64 / 8.0;
        ctx.begin_path();
        ctx.move_to(x, 0.0);
        ctx.line_to(x, h);
        ctx.stroke();
        let y = h * i as f64 / 8.0;
        ctx.begin_path();
        ctx.move_to(0.0, y);
        ctx.line_to(w, y);
        ctx.stroke();
    }

    // --- Y-axis indicator: arrow pointing UP at x=80 ---
    // This demonstrates that +Y goes upward (Y-up convention)
    let ax = 80.0;
    let ay_base = h * 0.2;
    let ay_tip = h * 0.8;

    ctx.set_stroke_color(Color::rgb(0.3, 0.9, 0.3));
    ctx.set_line_width(2.5);
    ctx.begin_path();
    ctx.move_to(ax, ay_base);
    ctx.line_to(ax, ay_tip);
    ctx.stroke();
    // Arrowhead
    ctx.set_fill_color(Color::rgb(0.3, 0.9, 0.3));
    ctx.begin_path();
    ctx.move_to(ax, ay_tip + 14.0);
    ctx.line_to(ax - 9.0, ay_tip - 2.0);
    ctx.line_to(ax + 9.0, ay_tip - 2.0);
    ctx.close_path();
    ctx.fill();
    // Label: Y-up
    ctx.set_fill_color(Color::rgb(0.3, 0.9, 0.3));
    ctx.fill_text_gsv("+Y", ax - 14.0, ay_tip + 20.0, 14.0);

    // --- X-axis indicator: arrow pointing RIGHT at y=80 ---
    let bx_base = w * 0.15;
    let bx_tip = w * 0.45;
    let by = 80.0;

    ctx.set_stroke_color(Color::rgb(0.9, 0.3, 0.3));
    ctx.set_line_width(2.5);
    ctx.begin_path();
    ctx.move_to(bx_base, by);
    ctx.line_to(bx_tip, by);
    ctx.stroke();
    // Arrowhead
    ctx.set_fill_color(Color::rgb(0.9, 0.3, 0.3));
    ctx.begin_path();
    ctx.move_to(bx_tip + 14.0, by);
    ctx.line_to(bx_tip - 2.0, by - 9.0);
    ctx.line_to(bx_tip - 2.0, by + 9.0);
    ctx.close_path();
    ctx.fill();
    ctx.set_fill_color(Color::rgb(0.9, 0.3, 0.3));
    ctx.fill_text_gsv("+X", bx_tip + 18.0, by - 7.0, 14.0);

    // --- Origin dot at (0,0) —-
    // In Y-up space the origin is the bottom-left corner.
    // We draw a small dot near it to mark it.
    ctx.set_fill_color(Color::rgb(1.0, 1.0, 0.3));
    ctx.begin_path();
    ctx.circle(18.0, 18.0, 8.0);
    ctx.fill();
    ctx.set_fill_color(Color::rgb(0.9, 0.9, 0.2));
    ctx.fill_text_gsv("(0,0)", 4.0, 30.0, 11.0);

    // --- CCW rotation proof ---
    // A right-pointing arrow at center, rotated +90° should point UP.
    ctx.save();
    ctx.translate(cx, cy);
    ctx.rotate(std::f64::consts::FRAC_PI_2); // +90° CCW

    // Draw the original arrow (points right in local space).
    // After +90° rotation in Y-up → points UP in world space.
    let arrow_len = w.min(h) * 0.18;
    let arrow_half_w = arrow_len * 0.08;
    ctx.set_fill_color(Color::rgb(0.4, 0.6, 1.0));
    ctx.begin_path();
    ctx.move_to(-arrow_len * 0.5, -arrow_half_w);
    ctx.line_to(arrow_len * 0.3, -arrow_half_w);
    ctx.line_to(arrow_len * 0.3, -arrow_half_w * 2.5);
    ctx.line_to(arrow_len * 0.5, 0.0);
    ctx.line_to(arrow_len * 0.3, arrow_half_w * 2.5);
    ctx.line_to(arrow_len * 0.3, arrow_half_w);
    ctx.line_to(-arrow_len * 0.5, arrow_half_w);
    ctx.close_path();
    ctx.fill();
    ctx.restore();

    // Label for rotation proof
    ctx.set_fill_color(Color::rgba(0.4, 0.6, 1.0, 0.9));
    ctx.fill_text_gsv("rotate(+90deg) -> points UP", cx - 90.0, cy + arrow_len * 0.55 + 18.0, 12.0);

    // --- Circle at center (unit circle reference) ---
    let r = w.min(h) * 0.12;
    ctx.set_stroke_color(Color::rgba(1.0, 1.0, 1.0, 0.25));
    ctx.set_line_width(1.5);
    ctx.begin_path();
    ctx.circle(cx, cy, r);
    ctx.stroke();

    // --- Corner dots: prove bottom-left is Y=0 ---
    let pad = 30.0;
    let dot_r = 6.0;
    // Bottom-left corner (Y=0 = low Y value = near origin)
    ctx.set_fill_color(Color::rgb(0.9, 0.9, 0.3));
    ctx.begin_path();
    ctx.circle(pad, pad, dot_r);
    ctx.fill();
    // Top-left corner (Y=max = high Y value)
    ctx.set_fill_color(Color::rgb(0.3, 0.9, 0.9));
    ctx.begin_path();
    ctx.circle(pad, h - pad, dot_r);
    ctx.fill();
    // Bottom-right
    ctx.set_fill_color(Color::rgb(0.9, 0.3, 0.9));
    ctx.begin_path();
    ctx.circle(w - pad, pad, dot_r);
    ctx.fill();
    // Top-right
    ctx.set_fill_color(Color::rgb(1.0, 1.0, 1.0));
    ctx.begin_path();
    ctx.circle(w - pad, h - pad, dot_r);
    ctx.fill();

    // --- Title ---
    ctx.set_fill_color(Color::rgba(1.0, 1.0, 1.0, 0.9));
    ctx.fill_text_gsv("agg-gui  Phase 1", cx - 60.0, h - 36.0, 18.0);
    ctx.set_fill_color(Color::rgba(0.6, 0.6, 0.6, 0.7));
    ctx.fill_text_gsv("Y-up coordinates  |  CCW rotations  |  AGG rasterization", cx - 145.0, h - 56.0, 11.0);
}
