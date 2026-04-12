//! WASM demo crate for agg-gui — Phase 4.
//!
//! Exports:
//! - `render_basics(w, h)` — interactive widget demo (buttons + text fields)
//! - `render_text(w, h)` — Phase 3 text showcase (stateless)
//! - `on_mouse_move`, `on_mouse_down`, `on_mouse_up`, `on_key_down` —
//!   event ingestion for the Basics tab widget tree

use std::cell::RefCell;
use std::sync::Arc;

use wasm_bindgen::prelude::*;
use agg_gui::{
    App, Button, Color, CompOp, Container, Font, Framebuffer, GfxCtx, Modifiers,
    MouseButton, Size, TextField, Widget,
};

// Embed the font at compile time.
const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");

fn make_font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("embedded font is valid"))
}

// ---------------------------------------------------------------------------
// Persistent widget tree for the interactive Basics tab
// ---------------------------------------------------------------------------

thread_local! {
    static BASICS_APP: RefCell<Option<App>> = RefCell::new(None);
    static VIEWPORT_H: RefCell<f64> = RefCell::new(1.0);
}

fn ensure_basics_app(width: u32, height: u32) {
    BASICS_APP.with(|cell| {
        if cell.borrow().is_none() {
            let font = make_font();
            *cell.borrow_mut() = Some(build_basics_ui(font, width, height));
        }
    });
}

fn build_basics_ui(font: Arc<Font>, _width: u32, _height: u32) -> App {
    let font2 = Arc::clone(&font);
    let font3 = Arc::clone(&font);
    let font4 = Arc::clone(&font);
    let font5 = Arc::clone(&font);

    let mut root = Container::new()
        .with_background(Color::rgb(0.94, 0.94, 0.96))
        .with_padding(24.0);

    root.children_mut().push(Box::new(
        Button::new("Primary Action", Arc::clone(&font))
            .with_font_size(14.0)
            .on_click(|| {}),
    ));

    root.children_mut().push(Box::new(
        Button::new("Secondary", Arc::clone(&font2))
            .with_font_size(14.0)
            .with_theme(agg_gui::widgets::button::ButtonTheme {
                background:         Color::rgba(0.22, 0.45, 0.88, 0.12),
                background_hovered: Color::rgba(0.22, 0.45, 0.88, 0.22),
                background_pressed: Color::rgba(0.22, 0.45, 0.88, 0.35),
                label_color:        Color::rgb(0.22, 0.45, 0.88),
                border_radius:      6.0,
                focus_ring_color:   Color::rgba(0.22, 0.45, 0.88, 0.55),
                focus_ring_width:   2.5,
            }),
    ));

    root.children_mut().push(Box::new(
        Button::new("Destructive", Arc::clone(&font3))
            .with_font_size(14.0)
            .with_theme(agg_gui::widgets::button::ButtonTheme {
                background:         Color::rgb(0.88, 0.25, 0.18),
                background_hovered: Color::rgb(0.95, 0.32, 0.24),
                background_pressed: Color::rgb(0.72, 0.18, 0.12),
                label_color:        Color::white(),
                border_radius:      6.0,
                focus_ring_color:   Color::rgba(0.88, 0.25, 0.18, 0.55),
                focus_ring_width:   2.5,
            }),
    ));

    root.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font4))
            .with_font_size(14.0)
            .with_placeholder("Type something…"),
    ));

    root.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font5))
            .with_font_size(14.0)
            .with_text("editable text")
            .with_placeholder("Another field"),
    ));

    App::new(Box::new(root))
}

// ---------------------------------------------------------------------------
// WASM event exports (called by JS before render_basics)
// ---------------------------------------------------------------------------

/// Parse a JS KeyboardEvent.key string into our Key type.
fn parse_js_key(key: &str) -> Option<agg_gui::Key> {
    use agg_gui::Key;
    Some(match key {
        "Backspace"  => Key::Backspace,
        "Delete"     => Key::Delete,
        "ArrowLeft"  => Key::ArrowLeft,
        "ArrowRight" => Key::ArrowRight,
        "Home"       => Key::Home,
        "End"        => Key::End,
        "Tab"        => Key::Tab,
        "Enter"      => Key::Enter,
        "Escape"     => Key::Escape,
        " "          => Key::Char(' '),
        s if s.chars().count() == 1 => Key::Char(s.chars().next()?),
        s => Key::Other(s.to_string()),
    })
}

#[wasm_bindgen]
pub fn on_mouse_move(x: f64, y: f64) {
    BASICS_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_move(x, y);
        }
    });
}

#[wasm_bindgen]
pub fn on_mouse_down(x: f64, y: f64, button: u8) {
    let btn = match button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        n => MouseButton::Other(n),
    };
    BASICS_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_down(x, y, btn, Modifiers::default());
        }
    });
}

#[wasm_bindgen]
pub fn on_mouse_up(x: f64, y: f64, button: u8) {
    let btn = match button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        n => MouseButton::Other(n),
    };
    BASICS_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_up(x, y, btn, Modifiers::default());
        }
    });
}

#[wasm_bindgen]
pub fn on_key_down(key_str: &str, shift: bool, ctrl: bool, alt: bool) {
    if let Some(key) = parse_js_key(key_str) {
        let mods = Modifiers { shift, ctrl, alt };
        BASICS_APP.with(|cell| {
            if let Some(app) = cell.borrow_mut().as_mut() {
                app.on_key_down(key, mods);
            }
        });
    }
}

#[wasm_bindgen]
pub fn on_mouse_leave() {
    BASICS_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_leave();
        }
    });
}

// ---------------------------------------------------------------------------
// Tab: Basics — Phase 4 interactive widget demo
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn render_basics(width: u32, height: u32) -> Vec<u8> {
    ensure_basics_app(width, height);

    BASICS_APP.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let app = borrow.as_mut().unwrap();
        app.layout(Size::new(width as f64, height as f64));

        let mut fb = Framebuffer::new(width, height);
        {
            let mut ctx = GfxCtx::new(&mut fb);
            app.paint(&mut ctx);

            // Status label in the bottom-left corner
            let lsize = (width as f64 * 0.012).clamp(9.0, 13.0);
            let pad = 12.0;
            ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.3));
            ctx.fill_text_gsv("agg-gui  Phase 4 — Widgets", pad, pad * 0.5, lsize);
        }
        fb.pixels_flipped()
    })
}

// ---------------------------------------------------------------------------
// Tab: Text — Phase 3 content (unchanged, stateless)
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn render_text(width: u32, height: u32) -> Vec<u8> {
    let font = make_font();
    let mut fb = Framebuffer::new(width, height);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        draw_text_tab(&mut ctx, width, height, &font);
    }
    fb.pixels_flipped()
}

fn draw_text_tab(ctx: &mut GfxCtx, width: u32, height: u32, font: &Arc<Font>) {
    let w = width as f64;
    let h = height as f64;
    ctx.set_font(Arc::clone(font));

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

    { let (px, py, pw, ph) = panels[0]; draw_sizes_panel(ctx, px, py, pw, ph); }
    { let (px, py, pw, ph) = panels[1]; draw_measure_panel(ctx, px, py, pw, ph, font); }
    { let (px, py, pw, ph) = panels[2]; draw_multiline_panel(ctx, px, py, pw, ph, font); }
    { let (px, py, pw, ph) = panels[3]; draw_buttons_panel(ctx, px, py, pw, ph, font); }

    let lsize = (w * 0.012).clamp(9.0, 13.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.35));
    ctx.fill_text_gsv("agg-gui  Phase 3 — Text", pad, pad * 0.4, lsize);
}

// ---------------------------------------------------------------------------
// Text panel helpers (Phase 3, unchanged)
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

fn draw_sizes_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64) {
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
}

fn draw_measure_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, font: &Arc<Font>) {
    panel_title_gsv(ctx, px, py, pw, ph, "Measure Text");
    let margin = pw * 0.06;
    let font_size = (pw * 0.08).clamp(14.0, 26.0);
    ctx.set_font_size(font_size);
    let samples = ["Hello", "World!", "agg-gui", "Rust"];
    let col_w = (pw - margin * 2.0) / samples.len() as f64;
    let base_y = py + ph * 0.5;
    for (i, &word) in samples.iter().enumerate() {
        let x = px + margin + col_w * i as f64;
        let m = ctx.measure_text(word).unwrap_or_default();
        ctx.set_stroke_color(Color::rgba(0.6, 0.6, 0.65, 0.5));
        ctx.set_line_width(1.0);
        ctx.begin_path(); ctx.move_to(x, base_y - 2.0); ctx.line_to(x + m.width, base_y - 2.0); ctx.stroke();
        ctx.set_stroke_color(Color::rgba(0.2, 0.5, 0.9, 0.35));
        ctx.begin_path(); ctx.move_to(x, base_y + m.ascent); ctx.line_to(x + m.width, base_y + m.ascent); ctx.stroke();
        ctx.set_stroke_color(Color::rgba(0.9, 0.3, 0.3, 0.35));
        ctx.begin_path(); ctx.move_to(x, base_y - m.descent); ctx.line_to(x + m.width, base_y - m.descent); ctx.stroke();
        ctx.set_fill_color(Color::rgba(0.2, 0.5, 0.9, 0.07));
        ctx.begin_path(); ctx.rect(x, base_y - m.descent, m.width, m.ascent + m.descent); ctx.fill();
        ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.88));
        ctx.fill_text(word, x, base_y);
    }
    let lsize = (pw * 0.032).clamp(7.0, 10.0);
    let ly = py + ph * 0.22;
    let lx = px + margin;
    ctx.set_font_size(lsize);
    ctx.set_fill_color(Color::rgba(0.2, 0.5, 0.9, 0.7)); ctx.fill_text("— ascent", lx, ly);
    ctx.set_fill_color(Color::rgba(0.9, 0.3, 0.3, 0.7)); ctx.fill_text("— descent", lx, ly - lsize * 1.5);
    ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.55, 0.7)); ctx.fill_text("— baseline", lx, ly - lsize * 3.0);
    let _ = font;
}

fn draw_multiline_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, font: &Arc<Font>) {
    panel_title_gsv(ctx, px, py, pw, ph, "Multi-line");
    let margin = pw * 0.06;
    let font_size = (pw * 0.055).clamp(11.0, 16.0);
    ctx.set_font_size(font_size);
    ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.85));
    let line_h = font.line_height_px(font_size) * 1.25;
    let x = px + margin;
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
        if !line.is_empty() { ctx.fill_text(line, x, y); }
        y -= line_h;
    }
}

fn draw_buttons_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, font: &Arc<Font>) {
    panel_title_gsv(ctx, px, py, pw, ph, "Text + Graphics");
    let margin = pw * 0.07;
    let btn_h = ph * 0.16;
    let btn_r = btn_h * 0.35;
    let bx = px + margin;
    let bw = pw - margin * 2.0;
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
        ctx.set_fill_color(bg);
        ctx.set_blend_mode(CompOp::SrcOver);
        ctx.begin_path(); ctx.rounded_rect(bx, by - btn_h * 0.5, bw, btn_h, btn_r); ctx.fill();
        if let Some(m) = ctx.measure_text(label) {
            let tx = bx + (bw - m.width) * 0.5;
            let ty = by - m.ascent * 0.45 + m.descent * 0.45;
            ctx.set_fill_color(fg);
            ctx.fill_text(label, tx, ty);
        }
    }
    let _ = font;
}
