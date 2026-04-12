//! WASM demo crate for agg-gui — Phase 3.
//!
//! Exports two render functions (one per tab) that return pixels in top-down
//! (Y-down) order for JS `CanvasRenderingContext2D.putImageData`.

use std::sync::Arc;

use wasm_bindgen::prelude::*;
use agg_gui::{Color, CompOp, Font, Framebuffer, GfxCtx};

// Embed the font at compile time.
// Path from demo-wasm/src/lib.rs → ../../demo/assets/CascadiaCode.ttf
const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");

fn make_font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("embedded font is valid"))
}

fn render(draw: impl FnOnce(&mut GfxCtx, u32, u32), width: u32, height: u32) -> Vec<u8> {
    let mut fb = Framebuffer::new(width, height);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        draw(&mut ctx, width, height);
    }
    fb.pixels_flipped()
}

// ---------------------------------------------------------------------------
// Tab: Basics (Phase 2 content)
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn render_basics(width: u32, height: u32) -> Vec<u8> {
    render(draw_basics, width, height)
}

fn draw_basics(ctx: &mut GfxCtx, width: u32, height: u32) {
    let w = width as f64;
    let h = height as f64;
    let font = make_font();
    ctx.set_font(font);

    ctx.clear(Color::rgb(0.94, 0.94, 0.96));

    let pad = (w.min(h) * 0.03).max(10.0);
    let gap = pad * 0.6;
    let col_w = (w - pad * 2.0 - gap) / 2.0;
    let row_h = (h - pad * 2.0 - gap) / 2.0;

    let panels = [
        (pad,               pad + row_h + gap, col_w, row_h),
        (pad + col_w + gap, pad + row_h + gap, col_w, row_h),
        (pad,               pad,               col_w, row_h),
        (pad + col_w + gap, pad,               col_w, row_h),
    ];

    for &(px, py, pw, ph) in &panels {
        draw_card(ctx, px, py, pw, ph);
    }

    { let (px, py, pw, ph) = panels[0]; draw_rounded_rects_panel(ctx, px, py, pw, ph); }
    { let (px, py, pw, ph) = panels[1]; draw_blend_modes_panel(ctx, px, py, pw, ph); }
    { let (px, py, pw, ph) = panels[2]; draw_clip_panel(ctx, px, py, pw, ph); }
    { let (px, py, pw, ph) = panels[3]; draw_transform_panel(ctx, px, py, pw, ph); }

    let lsize = (w * 0.012).clamp(9.0, 13.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.35));
    ctx.fill_text_gsv("agg-gui  Phase 3", pad, pad * 0.4, lsize);
}

// ---------------------------------------------------------------------------
// Tab: Text (Phase 3 content)
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn render_text(width: u32, height: u32) -> Vec<u8> {
    render(draw_text_tab, width, height)
}

fn draw_text_tab(ctx: &mut GfxCtx, width: u32, height: u32) {
    let w = width as f64;
    let h = height as f64;
    let font = make_font();
    ctx.set_font(font.clone());

    ctx.clear(Color::rgb(0.94, 0.94, 0.96));

    let pad = (w.min(h) * 0.03).max(10.0);
    let gap = pad * 0.6;
    let col_w = (w - pad * 2.0 - gap) / 2.0;
    let row_h = (h - pad * 2.0 - gap) / 2.0;

    let panels = [
        (pad,               pad + row_h + gap, col_w, row_h), // top-left
        (pad + col_w + gap, pad + row_h + gap, col_w, row_h), // top-right
        (pad,               pad,               col_w, row_h), // bottom-left
        (pad + col_w + gap, pad,               col_w, row_h), // bottom-right
    ];

    for &(px, py, pw, ph) in &panels {
        draw_card(ctx, px, py, pw, ph);
    }

    { let (px, py, pw, ph) = panels[0]; draw_sizes_panel(ctx, px, py, pw, ph, &font); }
    { let (px, py, pw, ph) = panels[1]; draw_measure_panel(ctx, px, py, pw, ph, &font); }
    { let (px, py, pw, ph) = panels[2]; draw_multiline_panel(ctx, px, py, pw, ph, &font); }
    { let (px, py, pw, ph) = panels[3]; draw_buttons_panel(ctx, px, py, pw, ph, &font); }

    let lsize = (w * 0.012).clamp(9.0, 13.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.35));
    ctx.fill_text_gsv("agg-gui  Phase 3 — Text", pad, pad * 0.4, lsize);
}

// ---------------------------------------------------------------------------
// Text panel 1: Size spectrum
// ---------------------------------------------------------------------------

fn draw_sizes_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, font: &Arc<Font>) {
    panel_title_gsv(ctx, px, py, pw, ph, "Font Sizes");

    let margin = pw * 0.06;
    let sizes: &[(f64, &str)] = &[
        (10.0, "Caption — 10px  The quick brown fox"),
        (13.0, "Body — 13px  The quick brown fox"),
        (18.0, "Subhead — 18px  The quick"),
        (24.0, "Heading — 24px  agg-gui"),
        (34.0, "Display — 34px  Aa"),
    ];

    let mut y = py + ph * 0.82;
    let baseline_adv = ph * 0.155;

    ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.85));
    for &(size, label) in sizes.iter() {
        ctx.set_font_size(size);
        ctx.fill_text(label, px + margin, y);
        y -= baseline_adv;
    }
    let _ = font;
}

// ---------------------------------------------------------------------------
// Text panel 2: measure_text visualization
// ---------------------------------------------------------------------------

fn draw_measure_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, font: &Arc<Font>) {
    panel_title_gsv(ctx, px, py, pw, ph, "Measure Text");

    let margin = pw * 0.06;
    let font_size = (pw * 0.08).clamp(14.0, 26.0);
    ctx.set_font_size(font_size);

    let samples = ["Hello", "World!", "agg-gui", "Rust"];
    let col_w = (pw - margin * 2.0) / samples.len() as f64;
    let base_y = py + ph * 0.5;

    ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.88));

    for (i, &word) in samples.iter().enumerate() {
        let x = px + margin + col_w * i as f64;

        // Measure
        let m = ctx.measure_text(word).unwrap_or_default();

        // Baseline tick
        ctx.set_stroke_color(Color::rgba(0.6, 0.6, 0.65, 0.5));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(x, base_y - 2.0);
        ctx.line_to(x + m.width, base_y - 2.0);
        ctx.stroke();

        // Ascent line
        ctx.set_stroke_color(Color::rgba(0.2, 0.5, 0.9, 0.35));
        ctx.begin_path();
        ctx.move_to(x, base_y + m.ascent);
        ctx.line_to(x + m.width, base_y + m.ascent);
        ctx.stroke();

        // Descent line
        ctx.set_stroke_color(Color::rgba(0.9, 0.3, 0.3, 0.35));
        ctx.begin_path();
        ctx.move_to(x, base_y - m.descent);
        ctx.line_to(x + m.width, base_y - m.descent);
        ctx.stroke();

        // Bounding box
        ctx.set_fill_color(Color::rgba(0.2, 0.5, 0.9, 0.07));
        ctx.begin_path();
        ctx.rect(x, base_y - m.descent, m.width, m.ascent + m.descent);
        ctx.fill();

        // The word
        ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.88));
        ctx.fill_text(word, x, base_y);
    }

    // Legend
    let lsize = (pw * 0.032).clamp(7.0, 10.0);
    let ly = py + ph * 0.22;
    let lx = px + margin;
    ctx.set_font_size(lsize);

    ctx.set_fill_color(Color::rgba(0.2, 0.5, 0.9, 0.7));
    ctx.fill_text("— ascent", lx, ly);
    ctx.set_fill_color(Color::rgba(0.9, 0.3, 0.3, 0.7));
    ctx.fill_text("— descent", lx, ly - lsize * 1.5);
    ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.55, 0.7));
    ctx.fill_text("— baseline", lx, ly - lsize * 3.0);

    let _ = font;
}

// ---------------------------------------------------------------------------
// Text panel 3: Multi-line paragraph
// ---------------------------------------------------------------------------

fn draw_multiline_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, font: &Arc<Font>) {
    panel_title_gsv(ctx, px, py, pw, ph, "Multi-line");

    let margin = pw * 0.06;
    let font_size = (pw * 0.055).clamp(11.0, 16.0);
    ctx.set_font_size(font_size);
    ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.85));

    let line_h = font.line_height_px(font_size) * 1.25;
    let x = px + margin;

    // Simple word-wrap: pre-broken for this demo
    let lines = [
        "agg-gui renders text by",
        "shaping with rustybuzz,",
        "extracting outlines via",
        "ttf-parser, and feeding",
        "Bezier curves into AGG.",
        "",
        "No glyph atlas. Kerning",
        "and hinting are preserved.",
    ];

    let mut y = py + ph * 0.82;
    for line in lines.iter() {
        if !line.is_empty() {
            ctx.fill_text(line, x, y);
        }
        y -= line_h;
    }

    let _ = font;
}

// ---------------------------------------------------------------------------
// Text panel 4: Button-like elements (text + graphics integration)
// ---------------------------------------------------------------------------

fn draw_buttons_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, font: &Arc<Font>) {
    panel_title_gsv(ctx, px, py, pw, ph, "Text + Graphics");

    let margin = pw * 0.07;
    let btn_h = ph * 0.16;
    let btn_r = btn_h * 0.35;
    let bx = px + margin;
    let bw = pw - margin * 2.0;

    // Button definitions
    let buttons: &[(&str, Color, Color)] = &[
        ("Primary Action",  Color::rgb(0.22, 0.45, 0.88), Color::white()),
        ("Secondary",       Color::rgba(0.22, 0.45, 0.88, 0.12), Color::rgb(0.22, 0.45, 0.88)),
        ("Destructive",     Color::rgb(0.88, 0.25, 0.18), Color::white()),
        ("Disabled",        Color::rgba(0.0, 0.0, 0.0, 0.08), Color::rgba(0.0,0.0,0.0,0.3)),
    ];

    let spacing = (ph * 0.74) / buttons.len() as f64;
    let font_size = (btn_h * 0.38).clamp(10.0, 16.0);
    ctx.set_font_size(font_size);

    for (i, &(label, bg, fg)) in buttons.iter().enumerate() {
        let by = py + ph * 0.78 - i as f64 * spacing;

        // Button background
        ctx.set_fill_color(bg);
        ctx.set_blend_mode(CompOp::SrcOver);
        ctx.begin_path();
        ctx.rounded_rect(bx, by - btn_h * 0.5, bw, btn_h, btn_r);
        ctx.fill();

        // Button label — centered
        if let Some(m) = ctx.measure_text(label) {
            let tx = bx + (bw - m.width) * 0.5;
            let ty = by - m.ascent * 0.45 + m.descent * 0.45;
            ctx.set_fill_color(fg);
            ctx.fill_text(label, tx, ty);
        }
    }
    let _ = font;
}

// ---------------------------------------------------------------------------
// Basics panel helpers (unchanged from Phase 2)
// ---------------------------------------------------------------------------

fn draw_card(ctx: &mut GfxCtx, x: f64, y: f64, w: f64, h: f64) {
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.08));
    ctx.set_blend_mode(CompOp::Multiply);
    ctx.begin_path();
    ctx.rounded_rect(x + 2.0, y - 2.0, w, h, 10.0);
    ctx.fill();
    ctx.set_blend_mode(CompOp::SrcOver);
    ctx.set_fill_color(Color::rgb(1.0, 1.0, 1.0));
    ctx.begin_path();
    ctx.rounded_rect(x, y, w, h, 10.0);
    ctx.fill();
}

fn panel_title_gsv(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, title: &str) {
    let size = (pw * 0.055).clamp(10.0, 16.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.55));
    ctx.fill_text_gsv(title, px + pw * 0.05, py + ph * 0.86, size);
}

fn draw_rounded_rects_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64) {
    ctx.set_blend_mode(CompOp::SrcOver);
    let margin = pw * 0.07;
    let inner_x = px + margin;
    let inner_w = pw - margin * 2.0;
    let row_h = (ph - margin) / 3.0 - margin * 0.3;
    let radii = [4.0_f64, 12.0, row_h * 0.5];
    let colors = [
        Color::rgb(0.27, 0.53, 0.91),
        Color::rgb(0.22, 0.76, 0.55),
        Color::rgb(0.88, 0.42, 0.27),
    ];
    for (i, (&r, &col)) in radii.iter().zip(colors.iter()).enumerate() {
        let iy = py + ph - (i + 1) as f64 * (row_h + margin * 0.5) - margin * 0.3;
        ctx.set_fill_color(col.with_alpha(0.18));
        ctx.begin_path();
        ctx.rounded_rect(inner_x, iy, inner_w, row_h, r);
        ctx.fill();
        ctx.set_stroke_color(col);
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.rounded_rect(inner_x, iy, inner_w, row_h, r);
        ctx.stroke();
        let label = format!("r = {}", r as i32);
        let lsize = (pw * 0.04).clamp(8.0, 12.0);
        ctx.set_fill_color(col);
        ctx.fill_text_gsv(&label, inner_x + inner_w * 0.03, iy + row_h * 0.28, lsize);
    }
    panel_title_gsv(ctx, px, py, pw, ph, "Rounded Rects");
}

fn draw_blend_modes_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64) {
    let cy = py + ph * 0.5;
    let col_w = pw / 3.0;
    let lsize = (pw * 0.032).clamp(7.0, 10.0);
    let modes: [(CompOp, &str); 3] = [
        (CompOp::Multiply, "Multiply"),
        (CompOp::Screen,   "Screen"),
        (CompOp::Overlay,  "Overlay"),
    ];
    for (i, &(mode, label)) in modes.iter().enumerate() {
        let ccx = px + col_w * (i as f64 + 0.5);
        let small_r = pw.min(ph) * 0.15;
        ctx.set_blend_mode(CompOp::SrcOver);
        ctx.set_fill_color(Color::rgba(0.22, 0.45, 0.87, 0.9));
        ctx.begin_path();
        ctx.circle(ccx - small_r * 0.35, cy - small_r * 0.2, small_r);
        ctx.fill();
        ctx.set_blend_mode(mode);
        ctx.set_fill_color(Color::rgba(0.91, 0.28, 0.18, 0.9));
        ctx.begin_path();
        ctx.circle(ccx + small_r * 0.35, cy + small_r * 0.2, small_r);
        ctx.fill();
        ctx.set_fill_color(Color::rgba(0.14, 0.76, 0.39, 0.85));
        ctx.begin_path();
        ctx.circle(ccx, cy - small_r * 0.55, small_r);
        ctx.fill();
        ctx.set_blend_mode(CompOp::SrcOver);
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.5));
        let lx = ccx - lsize * label.len() as f64 * 0.35;
        ctx.fill_text_gsv(label, lx, py + ph * 0.08, lsize);
    }
    panel_title_gsv(ctx, px, py, pw, ph, "Blend Modes");
}

fn draw_clip_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64) {
    ctx.set_blend_mode(CompOp::SrcOver);
    let margin = pw * 0.08;
    let cx = px + pw * 0.5;
    let cy = py + ph * 0.5;
    let clip_x = px + margin * 1.5;
    let clip_y = py + margin * 1.5;
    let clip_w = pw - margin * 3.0;
    let clip_h = ph - margin * 3.5;

    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.06));
    ctx.begin_path();
    ctx.rounded_rect(px + margin * 0.3, py + margin * 0.3,
                     pw - margin * 0.6, ph - margin * 0.6, 6.0);
    ctx.fill();

    ctx.save();
    ctx.clip_rect(clip_x, clip_y, clip_w, clip_h);

    let n = 8;
    let ring_r = pw.min(ph) * 0.28;
    let dot_r  = pw.min(ph) * 0.09;
    let colors = [
        Color::rgb(0.27, 0.53, 0.91), Color::rgb(0.91, 0.35, 0.22),
        Color::rgb(0.22, 0.76, 0.42), Color::rgb(0.88, 0.65, 0.10),
        Color::rgb(0.62, 0.28, 0.88), Color::rgb(0.10, 0.72, 0.88),
        Color::rgb(0.95, 0.38, 0.62), Color::rgb(0.38, 0.82, 0.12),
    ];
    for i in 0..n {
        let angle = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
        ctx.set_fill_color(colors[i % colors.len()]);
        ctx.begin_path();
        ctx.circle(cx + angle.cos() * ring_r, cy + angle.sin() * ring_r, dot_r);
        ctx.fill();
    }
    ctx.set_fill_color(Color::rgba(0.27, 0.53, 0.91, 0.25));
    ctx.begin_path();
    ctx.circle(cx, cy, ring_r * 0.55);
    ctx.fill();
    ctx.set_stroke_color(Color::rgba(0.27, 0.53, 0.91, 0.6));
    ctx.set_line_width(2.0);
    ctx.begin_path();
    ctx.circle(cx, cy, ring_r * 0.55);
    ctx.stroke();
    ctx.restore();

    ctx.set_stroke_color(Color::rgba(0.3, 0.3, 0.3, 0.4));
    ctx.set_line_width(1.5);
    ctx.begin_path();
    ctx.rounded_rect(clip_x, clip_y, clip_w, clip_h, 4.0);
    ctx.stroke();
    panel_title_gsv(ctx, px, py, pw, ph, "Clip Rect");
}

fn draw_transform_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64) {
    ctx.set_blend_mode(CompOp::SrcOver);
    let cx = px + pw * 0.5;
    let cy = py + ph * 0.5;
    let unit = pw.min(ph) * 0.12;
    let levels = [
        (unit * 2.8, 0.0_f64,                    Color::rgba(0.27, 0.53, 0.91, 0.25), Color::rgba(0.27, 0.53, 0.91, 0.8)),
        (unit * 2.0, std::f64::consts::PI / 6.0, Color::rgba(0.22, 0.76, 0.42, 0.25), Color::rgba(0.22, 0.76, 0.42, 0.8)),
        (unit * 1.2, std::f64::consts::PI / 4.0, Color::rgba(0.91, 0.42, 0.22, 0.3),  Color::rgba(0.91, 0.42, 0.22, 0.9)),
    ];
    for &(size, rot, fill, stroke) in &levels {
        ctx.save();
        ctx.translate(cx, cy);
        ctx.rotate(rot);
        ctx.set_fill_color(fill);
        ctx.begin_path();
        ctx.rounded_rect(-size * 0.5, -size * 0.5, size, size, size * 0.12);
        ctx.fill();
        ctx.set_stroke_color(stroke);
        ctx.set_line_width(1.8);
        ctx.begin_path();
        ctx.rounded_rect(-size * 0.5, -size * 0.5, size, size, size * 0.12);
        ctx.stroke();
        ctx.restore();
    }
    ctx.set_fill_color(Color::rgb(0.2, 0.2, 0.25));
    ctx.begin_path();
    ctx.circle(cx, cy, unit * 0.18);
    ctx.fill();
    let ax_len = unit * 1.5;
    ctx.set_stroke_color(Color::rgba(0.85, 0.2, 0.2, 0.7));
    ctx.set_line_width(1.5);
    ctx.begin_path(); ctx.move_to(cx, cy); ctx.line_to(cx + ax_len, cy); ctx.stroke();
    ctx.set_stroke_color(Color::rgba(0.1, 0.7, 0.2, 0.7));
    ctx.begin_path(); ctx.move_to(cx, cy); ctx.line_to(cx, cy + ax_len); ctx.stroke();
    panel_title_gsv(ctx, px, py, pw, ph, "Transform Stack");
}
