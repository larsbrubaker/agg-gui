//! Key painting for the on-screen keyboard — face fills, caps, and the
//! procedural glyphs (shift, backspace, return, …).
//!
//! Split out of [`super::layouts`] so the layout tables stay a pure
//! data description; this module owns everything visual about a single
//! key. Style tokens come from [`super::style::Style`].

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect};

use super::key::{KeyCap, KeyGlyph};
use super::layouts::{KeyKind, KeySpec, Layer};
use super::style::Style;

pub(super) fn paint_key(
    ctx: &mut dyn DrawCtx,
    rect: Rect,
    spec: &KeySpec,
    pressed: bool,
    style: &Style,
    active_layer: Layer,
) {
    let (bg, text_color) = match (spec.kind, pressed) {
        (KeyKind::Spacer, _) => return,
        (KeyKind::Letter, false) => (style.key_face_bg, style.key_face_text),
        (KeyKind::Letter, true) => (style.key_face_bg_pressed, style.key_face_text_pressed),
        (KeyKind::Utility, false) => (style.util_key_bg, style.util_key_text),
        (KeyKind::Utility, true) => (style.util_key_bg_pressed, style.key_face_text_pressed),
        (KeyKind::Return, false) => (style.return_key_bg, style.return_key_text),
        (KeyKind::Return, true) => (style.return_key_bg_pressed, style.return_key_text),
    };

    // Gboard corner keys are pills; everything else uses the shared radius.
    let radius = if spec.pill {
        (rect.height.min(rect.width)) / 2.0
    } else {
        style.key_corner_radius
    };

    // Faux 1-pixel drop shadow (Y-up: shadow_offset_y is negative).
    // Styles that want flat keys (Gboard) ship a fully transparent
    // shadow color.
    if style.key_shadow.a > 0.0 {
        ctx.set_fill_color(style.key_shadow);
        ctx.begin_path();
        ctx.rounded_rect(
            rect.x,
            rect.y + style.key_shadow_offset_y,
            rect.width,
            rect.height,
            radius,
        );
        ctx.fill();
    }

    ctx.set_fill_color(bg);
    ctx.begin_path();
    ctx.rounded_rect(rect.x, rect.y, rect.width, rect.height, radius);
    ctx.fill();

    ctx.set_fill_color(text_color);
    let center = Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);

    match &spec.cap {
        KeyCap::Text(text) => {
            if text.is_empty() {
                return; // blank spacebar
            }
            let font_size = if matches!(spec.kind, KeyKind::Letter) && text.chars().count() == 1 {
                style.letter_font_size
            } else {
                style.utility_font_size
            };
            ctx.set_font_size(font_size);
            // Approximate text width: agg-gui's `measure_text` needs an
            // active font set, which the host installs at startup. If
            // none is set the text falls back to GSV outlines. Either
            // way, we just need to draw "near the center" — exact
            // centering can come once we wire up `measure_text` for
            // real.
            let approx_width = text.chars().count() as f64 * font_size * 0.55;
            ctx.fill_text(
                text,
                center.x - approx_width / 2.0,
                center.y - font_size * 0.3,
            );
        }
        KeyCap::Glyph(glyph) => {
            paint_glyph(ctx, center, style, *glyph, text_color, active_layer);
        }
    }
}

fn paint_glyph(
    ctx: &mut dyn DrawCtx,
    center: Point,
    style: &Style,
    glyph: KeyGlyph,
    color: Color,
    active_layer: Layer,
) {
    let r = style.utility_font_size * 0.55;
    ctx.set_stroke_color(color);
    ctx.set_fill_color(color);
    ctx.set_line_width(2.0);
    match glyph {
        KeyGlyph::Backspace => {
            ctx.begin_path();
            // Tag shape: rectangle with a triangular notch on the left.
            ctx.move_to(center.x - r, center.y);
            ctx.line_to(center.x - r * 0.4, center.y + r * 0.7);
            ctx.line_to(center.x + r * 0.9, center.y + r * 0.7);
            ctx.line_to(center.x + r * 0.9, center.y - r * 0.7);
            ctx.line_to(center.x - r * 0.4, center.y - r * 0.7);
            ctx.close_path();
            ctx.stroke();
            // X inside.
            ctx.begin_path();
            ctx.move_to(center.x - r * 0.05, center.y - r * 0.35);
            ctx.line_to(center.x + r * 0.55, center.y + r * 0.35);
            ctx.move_to(center.x - r * 0.05, center.y + r * 0.35);
            ctx.line_to(center.x + r * 0.55, center.y - r * 0.35);
            ctx.stroke();
        }
        KeyGlyph::Shift => {
            ctx.begin_path();
            ctx.move_to(center.x, center.y + r);
            ctx.line_to(center.x - r, center.y);
            ctx.line_to(center.x - r * 0.4, center.y);
            ctx.line_to(center.x - r * 0.4, center.y - r * 0.6);
            ctx.line_to(center.x + r * 0.4, center.y - r * 0.6);
            ctx.line_to(center.x + r * 0.4, center.y);
            ctx.line_to(center.x + r, center.y);
            ctx.close_path();
            // Gboard fills the shift arrow while upper-case is armed.
            if active_layer == Layer::Shifted {
                ctx.fill();
            } else {
                ctx.stroke();
            }
        }
        KeyGlyph::DismissDown => {
            ctx.begin_path();
            ctx.move_to(center.x - r, center.y + r * 0.3);
            ctx.line_to(center.x, center.y - r * 0.3);
            ctx.line_to(center.x + r, center.y + r * 0.3);
            ctx.stroke();
            ctx.begin_path();
            ctx.move_to(center.x - r, center.y - r * 0.6);
            ctx.line_to(center.x + r, center.y - r * 0.6);
            ctx.stroke();
        }
        KeyGlyph::Return => {
            ctx.begin_path();
            ctx.move_to(center.x + r, center.y + r * 0.6);
            ctx.line_to(center.x + r, center.y - r * 0.2);
            ctx.line_to(center.x - r * 0.5, center.y - r * 0.2);
            ctx.stroke();
            ctx.begin_path();
            ctx.move_to(center.x - r * 0.5, center.y + r * 0.3);
            ctx.line_to(center.x - r, center.y - r * 0.2);
            ctx.line_to(center.x - r * 0.5, center.y - r * 0.7);
            ctx.stroke();
        }
        KeyGlyph::Space => {
            // ⌴ bracket — the numpad's space key label.
            ctx.begin_path();
            ctx.move_to(center.x - r, center.y + r * 0.25);
            ctx.line_to(center.x - r, center.y - r * 0.25);
            ctx.line_to(center.x + r, center.y - r * 0.25);
            ctx.line_to(center.x + r, center.y + r * 0.25);
            ctx.stroke();
        }
    }
}
