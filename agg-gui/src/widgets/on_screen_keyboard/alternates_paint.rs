//! Drawing the long-press alternates popup and the iPhone's key balloon.
//!
//! Gboard opens the alternates as a white pill over the row above the held
//! key, the highlighted one on a round accent, with the rest of the
//! keyboard dimmed behind it. The iPhone grows the held key into a balloon:
//! the key's own face rises through a curved neck into a wider bubble, which
//! shows the pressed letter (the key preview) or the row of alternates with
//! the highlighted one on a blue rounded square.
//!
//! The popup's geometry and state are in `alternates.rs`; the paint pass
//! (`layout_paint.rs`) calls these after the keys.

use crate::draw_ctx::DrawCtx;
use crate::geometry::Rect;

use super::alternates::AlternatesPopup;
use super::layouts::LayoutFamily;
use super::style::Style;

/// Paint the open alternates popup over the keys in `panel`.
pub(super) fn paint_alternates(
    ctx: &mut dyn DrawCtx,
    panel: Rect,
    popup: &AlternatesPopup,
    style: &Style,
    family: LayoutFamily,
) {
    if style.alt_dim.a > 0.0 {
        ctx.set_fill_color(style.alt_dim);
        ctx.begin_path();
        ctx.rect(panel.x, panel.y, panel.width, panel.height);
        ctx.fill();
    }
    let row = popup.row();
    match family {
        LayoutFamily::Gboard => {
            let body = Rect::new(row.x - 8.0, row.y - 4.0, row.width + 16.0, row.height + 8.0);
            let radius = body.height / 2.0;
            ctx.set_fill_color(style.panel_top_border);
            ctx.begin_path();
            ctx.rounded_rect(body.x, body.y - 1.5, body.width, body.height, radius);
            ctx.fill();
            ctx.set_fill_color(style.preview_bg);
            ctx.begin_path();
            ctx.rounded_rect(body.x, body.y, body.width, body.height, radius);
            ctx.fill();
        }
        LayoutFamily::Ios => {
            let bubble = Rect::new(row.x - 4.0, row.y - 2.0, row.width + 8.0, row.height + 6.0);
            fill_balloon(ctx, popup.key_rect, bubble, style);
        }
    }
    let size = style.letter_font_size * 1.1;
    for (i, (cell, c)) in popup.cells.iter().zip(&popup.chars).enumerate() {
        let lit = popup.highlighted == Some(i);
        let (cx, cy) = (cell.x + cell.width / 2.0, cell.y + cell.height / 2.0);
        if lit {
            ctx.set_fill_color(style.alt_highlight_bg);
            ctx.begin_path();
            match family {
                LayoutFamily::Gboard => {
                    ctx.circle(cx, cy, cell.width.min(cell.height) / 2.0 * 0.98);
                }
                LayoutFamily::Ios => ctx.rounded_rect(
                    cell.x + 1.5,
                    cell.y + 1.5,
                    cell.width - 3.0,
                    cell.height - 3.0,
                    style.key_corner_radius,
                ),
            }
            ctx.fill();
        }
        let text_color = if lit {
            style.alt_highlight_text
        } else {
            style.key_face_text
        };
        draw_centered(ctx, &c.to_string(), cx, cy, size, text_color);
    }
}

/// The iPhone's key preview: the pressed key grown into a balloon showing
/// its letter, kept inside `panel`'s width.
pub(super) fn paint_ios_balloon(
    ctx: &mut dyn DrawCtx,
    panel: Rect,
    key: Rect,
    text: &str,
    style: &Style,
) {
    let width = key.width * 1.6;
    let height = key.height * 1.3;
    let x = (key.x + key.width / 2.0 - width / 2.0)
        .min(panel.x + panel.width - width - 2.0)
        .max(panel.x + 2.0);
    let bubble = Rect::new(x, key.y + key.height + 8.0, width, height);
    fill_balloon(ctx, key, bubble, style);
    draw_centered(
        ctx,
        text,
        bubble.x + bubble.width / 2.0,
        bubble.y + bubble.height / 2.0,
        style.letter_font_size * 1.6,
        style.key_face_text,
    );
}

/// Fill the balloon from `key` up into `bubble`, with the key shadow under
/// it.
fn fill_balloon(ctx: &mut dyn DrawCtx, key: Rect, bubble: Rect, style: &Style) {
    ctx.save();
    ctx.translate(0.0, -1.0);
    ctx.set_fill_color(style.key_shadow);
    balloon_path(ctx, key, bubble, style.key_corner_radius);
    ctx.fill();
    ctx.restore();
    ctx.set_fill_color(style.preview_bg);
    balloon_path(ctx, key, bubble, style.key_corner_radius);
    ctx.fill();
}

/// The balloon's outline (Y-up): the key's rounded bottom, its sides rising
/// to its top and curving out into the bubble's bottom edge, and the
/// bubble's rounded corners. Where the bubble had to slide to stay on
/// screen and leaves no room for a rounded corner beside the key, that
/// corner is square.
fn balloon_path(ctx: &mut dyn DrawCtx, key: Rect, bubble: Rect, radius: f64) {
    let (kx0, kx1, ky0, ky1) = (key.x, key.x + key.width, key.y, key.y + key.height);
    let (bx0, bx1, by0, by1) = (
        bubble.x,
        bubble.x + bubble.width,
        bubble.y,
        bubble.y + bubble.height,
    );
    let rk = radius;
    let rb = radius * 1.6;
    // How far each shoulder reaches out from the key along the bubble's
    // bottom before the flat edge takes over.
    let left_shoulder = (kx0 - bx0).clamp(0.0, 6.0);
    let right_shoulder = (bx1 - kx1).clamp(0.0, 6.0);
    ctx.begin_path();
    ctx.move_to(kx0, ky0 + rk);
    ctx.line_to(kx0, ky1);
    ctx.quad_to(kx0, by0, kx0 - left_shoulder, by0);
    if kx0 - left_shoulder > bx0 + rb {
        ctx.line_to(bx0 + rb, by0);
        ctx.quad_to(bx0, by0, bx0, by0 + rb);
    } else {
        ctx.line_to(bx0, by0);
    }
    ctx.line_to(bx0, by1 - rb);
    ctx.quad_to(bx0, by1, bx0 + rb, by1);
    ctx.line_to(bx1 - rb, by1);
    ctx.quad_to(bx1, by1, bx1, by1 - rb);
    if kx1 + right_shoulder < bx1 - rb {
        ctx.line_to(bx1, by0 + rb);
        ctx.quad_to(bx1, by0, bx1 - rb, by0);
    } else {
        ctx.line_to(bx1, by0);
    }
    ctx.line_to(kx1 + right_shoulder, by0);
    ctx.quad_to(kx1, by0, kx1, ky1);
    ctx.line_to(kx1, ky0 + rk);
    ctx.quad_to(kx1, ky0, kx1 - rk, ky0);
    ctx.line_to(kx0 + rk, ky0);
    ctx.quad_to(kx0, ky0, kx0, ky0 + rk);
    ctx.close_path();
}

/// Draw `text` at `size` centred on `(cx, cy)`.
fn draw_centered(
    ctx: &mut dyn DrawCtx,
    text: &str,
    cx: f64,
    cy: f64,
    size: f64,
    color: crate::color::Color,
) {
    ctx.set_font_size(size);
    ctx.set_fill_color(color);
    let w = ctx
        .measure_text(text)
        .map(|m| m.width)
        .unwrap_or(size * 0.55);
    ctx.fill_text(text, cx - w / 2.0, cy - size * 0.3);
}
