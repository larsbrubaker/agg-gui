use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::geometry::Rect;
use agg_gui::text::Font;

use crate::GlGfxCtx;

/// Draw the inspector hover overlay: teal fill + inset stroke + size label.
///
/// Called after every `App::paint` on both native and WASM so the Chrome-style
/// widget highlight is identical on both platforms.
pub fn draw_hover_overlay(ctx: &mut GlGfxCtx, rect: Rect) {
    if rect.width < 1.0 || rect.height < 1.0 {
        return;
    }
    let sw = 1.5_f64;
    let half = sw * 0.5;
    // Teal fill — full widget bounds.
    ctx.set_fill_color(Color::rgba(0.05, 0.65, 0.85, 0.18));
    ctx.begin_path();
    ctx.rect(rect.x, rect.y, rect.width, rect.height);
    ctx.fill();
    // Teal border — inset by half stroke-width so the outer edge never falls
    // below x=0 / y=0 (which would be clipped by the GL viewport).
    ctx.set_stroke_color(Color::rgba(0.05, 0.65, 0.85, 0.80));
    ctx.set_line_width(sw);
    ctx.begin_path();
    ctx.rect(
        rect.x + half,
        rect.y + half,
        (rect.width - sw).max(0.0),
        (rect.height - sw).max(0.0),
    );
    ctx.stroke();
    // Size label
    let label = format!("{:.0} × {:.0}", rect.width, rect.height);
    ctx.set_fill_color(Color::rgba(0.05, 0.65, 0.85, 1.00));
    ctx.fill_text_gsv(&label, rect.x + 2.0, rect.y + rect.height + 2.0, 9.0);
}

/// Draw a "WxH  X.Xms" status bar in the bottom-left corner of the viewport.
///
/// `frame_ms` is the render time of the *previous* frame (so the display does
/// not include its own drawing cost).  Both native and WASM use this function
/// to keep the status overlay visually identical.
pub fn draw_status_overlay(ctx: &mut GlGfxCtx, font: Arc<Font>, w: u32, h: u32, frame_ms: f64) {
    let status = format!("{}×{}   {:.1}ms", w, h, frame_ms);
    ctx.set_font(font);
    ctx.set_font_size(11.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.30));
    ctx.fill_text(&status, 12.0, 6.0);
}
