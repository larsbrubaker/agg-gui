//! Port of agg-sharp `Agg.GoldenImages/Golden2DPrimitiveTests.cs` with its
//! scenes from `Golden2DScenes.cs`: lines, filled paths, rounded rects,
//! gradients, image blits and the transform stack on a 512×384 white
//! surface (C#'s `WebGpuOffscreenCapture` default).
//!
//! Converted to a cross-backend test: C# compares its WebGPU render with PNGs
//! from its own renderer; here the wgpu backend is compared with agg-gui's
//! software `GfxCtx` (see `golden/harness.rs` for why). `Gradients` draws
//! its two ramps as linear gradient fills: C# drops to the immediate-mode GL
//! facade for per-vertex colour, which agg-gui's `DrawCtx` does not have, so
//! the three-colour triangle of that scene has no counterpart here.

use std::f64::consts::PI;

use agg_gui::color::Color;
use agg_gui::draw_ctx::{DrawCtx, GradientSpread, GradientStop, LinearGradientPaint};
use agg_rust::trans_affine::TransAffine;

use super::harness::render_both;

const W: u32 = 512;
const H: u32 = 384;

/// Byte levels allowed between the backends on a flat (non-edge) pixel.
const TOLERANCE: i32 = 2;

fn check(what: &str, draw: &dyn Fn(&mut dyn DrawCtx)) {
    let Some(pair) = render_both(W, H, Color::white(), draw) else {
        return;
    };
    pair.assert_matches(what, TOLERANCE);
}

fn line(ctx: &mut dyn DrawCtx, x1: f64, y1: f64, x2: f64, y2: f64, color: Color, width: f64) {
    ctx.set_stroke_color(color);
    ctx.set_line_width(width);
    ctx.begin_path();
    ctx.move_to(x1, y1);
    ctx.line_to(x2, y2);
    ctx.stroke();
}

fn rgb(r: i32, g: i32, b: i32) -> Color {
    Color::from_rgb8(r as u8, g as u8, b as u8)
}

fn lines(ctx: &mut dyn DrawCtx) {
    for step in 0..=12 {
        let angle = step as f64 * PI / 12.0;
        let width = 0.5 + (step as f64 * 0.35);
        let color = rgb(20 + step * 18, 60, 200 - step * 14);
        line(
            ctx,
            256.0 - (angle.cos() * 150.0),
            300.0 - (angle.sin() * 70.0),
            256.0 + (angle.cos() * 150.0),
            300.0 + (angle.sin() * 70.0),
            color,
            width,
        );
    }
    line(ctx, 20.0, 40.0, 492.0, 40.0, Color::black(), 1.0);
    line(ctx, 20.0, 60.5, 492.0, 60.5, Color::black(), 1.0);
    line(ctx, 40.0, 20.0, 40.0, 180.0, Color::black(), 1.0);
    line(ctx, 60.5, 20.0, 60.5, 180.0, Color::black(), 1.0);

    // A stroked open curve: joins and caps through the stroke generator.
    ctx.set_stroke_color(rgb(169, 169, 169));
    ctx.set_line_width(3.0);
    ctx.begin_path();
    ctx.move_to(120.0, 90.0);
    ctx.cubic_to(200.0, 200.0, 300.0, 30.0, 400.0, 150.0);
    ctx.stroke();
}

fn filled_paths(ctx: &mut dyn DrawCtx) {
    ctx.begin_path();
    for point in 0..10 {
        let angle = point as f64 * PI * 2.0 / 10.0;
        let radius = if point % 2 == 0 { 80.0 } else { 32.0 };
        let x = 110.0 + (angle.cos() * radius);
        let y = 270.0 + (angle.sin() * radius);
        if point == 0 {
            ctx.move_to(x, y);
        } else {
            ctx.line_to(x, y);
        }
    }
    ctx.close_path();
    ctx.set_fill_color(rgb(220, 60, 40));
    ctx.fill();

    ctx.begin_path();
    ctx.move_to(240.0, 210.0);
    ctx.line_to(370.0, 210.0);
    ctx.line_to(370.0, 340.0);
    ctx.line_to(240.0, 340.0);
    ctx.close_path();
    ctx.move_to(270.0, 240.0);
    ctx.line_to(270.0, 310.0);
    ctx.line_to(340.0, 310.0);
    ctx.line_to(340.0, 240.0);
    ctx.close_path();
    ctx.set_fill_color(rgb(40, 110, 200));
    ctx.fill();

    ctx.begin_path();
    ctx.ellipse(430.0, 275.0, 60.0, 40.0, 0.0, 0.0, 2.0 * PI, false);
    ctx.close_path();
    ctx.set_fill_color(Color::from_rgba8(30, 160, 90, 160));
    ctx.fill();

    // Sub-pixel placement: the same small circle stepped by a quarter pixel.
    for step in 0..8 {
        ctx.begin_path();
        ctx.circle(
            60.0 + (step as f64 * 30.0) + (step as f64 * 0.25),
            90.0,
            11.0,
        );
        ctx.set_fill_color(rgb(80, 80, 80));
        ctx.fill();
    }
}

fn rounded_rects(ctx: &mut dyn DrawCtx) {
    let radii = [0.0, 2.0, 6.0, 14.0, 30.0];
    for (index, r) in radii.iter().enumerate() {
        let i = index as i32;
        let top = 350.0 - (index as f64 * 66.0);
        ctx.begin_path();
        ctx.rounded_rect(30.0, top - 50.0, 200.0, 50.0, *r);
        ctx.set_fill_color(rgb(60 + i * 35, 90, 190 - i * 25));
        ctx.fill();

        ctx.begin_path();
        ctx.rounded_rect(270.0, top - 50.0, 210.0, 50.0, *r);
        ctx.set_stroke_color(rgb(20, 20, 20));
        ctx.set_line_width(2.0);
        ctx.stroke();
    }
}

fn ramp(ctx: &mut dyn DrawCtx, y: f64, from: Color, to: Color) {
    ctx.set_fill_linear_gradient(LinearGradientPaint {
        x1: 40.0,
        y1: 0.0,
        x2: 472.0,
        y2: 0.0,
        transform: TransAffine::new(),
        spread: GradientSpread::Pad,
        stops: vec![
            GradientStop {
                offset: 0.0,
                color: from,
            },
            GradientStop {
                offset: 1.0,
                color: to,
            },
        ],
    });
    ctx.begin_path();
    ctx.rect(40.0, y, 432.0, 130.0);
    ctx.fill();
}

fn gradients(ctx: &mut dyn DrawCtx) {
    // Opaque horizontal ramp.
    ramp(ctx, 220.0, rgb(255, 0, 0), rgb(0, 80, 255));
    // Alpha ramp over the white background, so the blend is captured.
    ramp(
        ctx,
        60.0,
        Color::from_rgba8(10, 140, 40, 0),
        Color::from_rgba8(10, 140, 40, 255),
    );
}

/// C#'s `BuildTestPattern`: a deterministic checker, top-row-first RGBA as
/// `draw_image_rgba` takes it (C#'s buffer is BGRA, bottom row first).
fn test_pattern(width: u32, height: u32, alpha: u8) -> Vec<u8> {
    let mut data = vec![0u8; (width * height * 4) as usize];
    for y in 0..height as i32 {
        let row = height as i32 - 1 - y;
        for x in 0..width as i32 {
            let checker = ((x / 4) + (y / 4)) % 2 == 0;
            let b = if checker { 255 - (x * 8) } else { 30 };
            let g = if checker { y * 8 } else { 200 };
            let r = if checker { 60 } else { x * 8 };
            let i = ((row * width as i32 + x) * 4) as usize;
            data[i] = r as u8;
            data[i + 1] = g as u8;
            data[i + 2] = b as u8;
            data[i + 3] = alpha;
        }
    }
    data
}

/// C#'s `Render(image, x, y, angle, scaleX, scaleY)`: the image's origin at
/// `(x, y)`, rotated and scaled about it.
fn blit(ctx: &mut dyn DrawCtx, data: &[u8], x: f64, y: f64, angle: f64, sx: f64, sy: f64) {
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(angle);
    ctx.scale(sx, sy);
    ctx.draw_image_rgba(data, 32, 32, 0.0, 0.0, 32.0, 32.0);
    ctx.restore();
}

fn fill_rect(ctx: &mut dyn DrawCtx, l: f64, b: f64, r: f64, t: f64, color: Color) {
    ctx.begin_path();
    ctx.rect(l, b, r - l, t - b);
    ctx.set_fill_color(color);
    ctx.fill();
}

fn image_blits(ctx: &mut dyn DrawCtx) {
    let source = test_pattern(32, 32, 255);
    fill_rect(ctx, 20.0, 210.0, 250.0, 360.0, rgb(230, 230, 190));
    blit(ctx, &source, 30.0, 300.0, 0.0, 1.0, 1.0);
    blit(ctx, &source, 90.0, 300.0, 0.0, 3.0, 3.0);
    blit(ctx, &source, 30.0, 230.0, 0.0, 0.5, 0.5);
    blit(ctx, &source, 200.0, 300.0, PI / 6.0, 2.0, 2.0);

    let translucent = test_pattern(32, 32, 110);
    fill_rect(ctx, 300.0, 210.0, 480.0, 360.0, rgb(40, 40, 120));
    blit(ctx, &translucent, 320.0, 300.0, 0.0, 2.0, 2.0);

    // Fractional destination placement, where the sampler's half-texel
    // convention shows.
    blit(ctx, &source, 40.5, 100.0, 0.0, 2.0, 2.0);
    blit(ctx, &source, 160.25, 100.0, 0.0, 2.0, 2.0);
    blit(ctx, &source, 280.75, 100.0, 0.0, 2.0, 2.0);
}

fn arrow(ctx: &mut dyn DrawCtx) {
    ctx.begin_path();
    ctx.move_to(0.0, -18.0);
    ctx.line_to(70.0, -18.0);
    ctx.line_to(70.0, -34.0);
    ctx.line_to(104.0, 0.0);
    ctx.line_to(70.0, 34.0);
    ctx.line_to(70.0, 18.0);
    ctx.line_to(0.0, 18.0);
    ctx.close_path();
}

fn transforms(ctx: &mut dyn DrawCtx) {
    for step in 0..8 {
        let s = step as f64;
        ctx.save();
        // C#'s `scale * rotate * translate` with row vectors: translate,
        // then rotate, then scale in DrawCtx's pre-multiplying calls.
        ctx.translate(256.0, 192.0);
        ctx.rotate(s * PI / 4.0);
        ctx.scale(0.4 + (s * 0.09), 0.4 + (s * 0.09));
        arrow(ctx);
        ctx.set_fill_color(rgb(30 + step * 25, 140, 220 - step * 22));
        ctx.fill();
        ctx.restore();
    }

    // A nested push, to prove the stack restores rather than merely resets.
    ctx.save();
    ctx.translate(60.0, 60.0);
    ctx.save();
    ctx.scale(0.5, 0.5);
    ctx.begin_path();
    ctx.rounded_rect(0.0, 0.0, 120.0, 60.0, 8.0);
    ctx.set_fill_color(Color::from_rgba8(0, 0, 0, 120));
    ctx.fill();
    ctx.restore();
    ctx.begin_path();
    ctx.rounded_rect(0.0, 0.0, 120.0, 60.0, 8.0);
    ctx.set_stroke_color(Color::black());
    ctx.set_line_width(2.0);
    ctx.stroke();
    ctx.restore();
}

#[test]
fn lines_test() {
    check("Primitives2D.Lines", &lines);
}

#[test]
fn filled_paths_test() {
    check("Primitives2D.FilledPaths", &filled_paths);
}

#[test]
fn rounded_rects_test() {
    check("Primitives2D.RoundedRects", &rounded_rects);
}

#[test]
fn gradients_test() {
    check("Primitives2D.Gradients", &gradients);
}

#[test]
fn image_blits_test() {
    check("Primitives2D.ImageBlits", &image_blits);
}

#[test]
fn transforms_test() {
    check("Primitives2D.Transforms", &transforms);
}
