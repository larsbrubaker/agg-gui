//! Key painting for the on-screen keyboard — face fills, caps, the
//! procedural glyphs (shift, backspace, the Enter actions, …), the
//! hide-keyboard chevron and the preview bubble over a pressed letter.
//!
//! Split out of [`super::layouts`] so the layout tables stay a pure
//! data description; this module owns everything visual about a single
//! key, and [`super::layout_paint`] calls it for each one. Style tokens
//! come from [`super::style::Style`].

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect};

use super::key::{KeyCap, KeyGlyph};
use super::layout_paint::PaintState;
use super::layouts::{KeyKind, KeySpec, Layer};
use super::style::Style;

/// Paint one key: its face, and `cap` (the label for the keyboard's
/// current state) centred on it.
pub(super) fn paint_key(
    ctx: &mut dyn DrawCtx,
    rect: Rect,
    spec: &KeySpec,
    cap: &KeyCap,
    pressed: bool,
    style: &Style,
    state: &PaintState,
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

    match cap {
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
            // Centre the label on its measured width. `measure_text` is
            // `None` only when the context has no font, and then no text
            // draws at all; the estimate just keeps the arithmetic whole.
            let measure = |ctx: &mut dyn DrawCtx, size: f64| {
                ctx.measure_text(text)
                    .map(|m| m.width)
                    .unwrap_or(text.chars().count() as f64 * size * 0.55)
            };
            let mut width = measure(ctx, font_size);
            // A label wider than its key ("1234" on a one-letter key)
            // shrinks to fit rather than spill over its neighbours.
            let room = rect.width - 6.0;
            let font_size = if width > room && width > 0.0 {
                let fitted = font_size * room / width;
                ctx.set_font_size(fitted);
                width = measure(ctx, fitted);
                fitted
            } else {
                font_size
            };
            ctx.fill_text(text, center.x - width / 2.0, center.y - font_size * 0.3);
        }
        KeyCap::Glyph(glyph) => {
            paint_glyph(ctx, center, style, *glyph, text_color, state);
        }
    }
}

fn paint_glyph(
    ctx: &mut dyn DrawCtx,
    center: Point,
    style: &Style,
    glyph: KeyGlyph,
    color: Color,
    state: &PaintState,
) {
    let r = style.utility_font_size * 0.6;
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
            // Gboard fills the shift arrow while upper case is armed, and
            // underlines it while Caps Lock holds it.
            if state.layer == Layer::Shifted {
                ctx.fill();
            } else {
                ctx.stroke();
            }
            if state.caps_lock {
                ctx.begin_path();
                ctx.rect(center.x - r * 0.6, center.y - r * 1.05, r * 1.2, r * 0.22);
                ctx.fill();
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
        KeyGlyph::Search => {
            // Magnifier: a ring up and left, a handle down and right.
            let ring = r * 0.55;
            let (cx, cy) = (center.x - r * 0.18, center.y + r * 0.18);
            ctx.begin_path();
            ctx.circle(cx, cy, ring);
            ctx.stroke();
            ctx.begin_path();
            let d = ring * std::f64::consts::FRAC_1_SQRT_2;
            ctx.move_to(cx + d, cy - d);
            ctx.line_to(center.x + r * 0.85, center.y - r * 0.85);
            ctx.stroke();
        }
        KeyGlyph::Done => {
            ctx.begin_path();
            ctx.move_to(center.x - r * 0.8, center.y);
            ctx.line_to(center.x - r * 0.25, center.y - r * 0.55);
            ctx.line_to(center.x + r * 0.85, center.y + r * 0.55);
            ctx.stroke();
        }
        KeyGlyph::Next | KeyGlyph::Previous => {
            // Arrow pointing right (Next) or left (Previous).
            let dir = if matches!(glyph, KeyGlyph::Next) {
                1.0
            } else {
                -1.0
            };
            ctx.begin_path();
            ctx.move_to(center.x - dir * r * 0.85, center.y);
            ctx.line_to(center.x + dir * r * 0.85, center.y);
            ctx.move_to(center.x + dir * r * 0.25, center.y + r * 0.6);
            ctx.line_to(center.x + dir * r * 0.85, center.y);
            ctx.line_to(center.x + dir * r * 0.25, center.y - r * 0.6);
            ctx.stroke();
        }
        KeyGlyph::Go => {
            // Arrow into a bar: "go there".
            ctx.begin_path();
            ctx.move_to(center.x - r * 0.9, center.y);
            ctx.line_to(center.x + r * 0.45, center.y);
            ctx.move_to(center.x - r * 0.1, center.y + r * 0.55);
            ctx.line_to(center.x + r * 0.45, center.y);
            ctx.line_to(center.x - r * 0.1, center.y - r * 0.55);
            ctx.move_to(center.x + r * 0.85, center.y + r * 0.7);
            ctx.line_to(center.x + r * 0.85, center.y - r * 0.7);
            ctx.stroke();
        }
        KeyGlyph::Send => {
            // Paper plane: a dart pointing right with a fold to its tail.
            ctx.begin_path();
            ctx.move_to(center.x - r * 0.85, center.y + r * 0.8);
            ctx.line_to(center.x + r * 0.9, center.y);
            ctx.line_to(center.x - r * 0.85, center.y - r * 0.8);
            ctx.line_to(center.x - r * 0.55, center.y);
            ctx.close_path();
            ctx.fill();
        }
    }
}

/// The hide-keyboard chevron below the keys: a small downward chevron on
/// the panel (no key face), with a round highlight while pressed.
pub(super) fn paint_chevron(ctx: &mut dyn DrawCtx, face: Rect, pressed: bool, style: &Style) {
    let center = Point::new(face.x + face.width / 2.0, face.y + face.height / 2.0);
    if pressed {
        ctx.set_fill_color(style.util_key_bg_pressed);
        ctx.begin_path();
        ctx.circle(center.x, center.y, face.width.min(face.height) / 2.0);
        ctx.fill();
    }
    let r = face.height * 0.18;
    ctx.set_stroke_color(style.util_key_text);
    ctx.set_line_width(2.0);
    ctx.begin_path();
    ctx.move_to(center.x - r, center.y + r * 0.5);
    ctx.line_to(center.x, center.y - r * 0.5);
    ctx.line_to(center.x + r, center.y + r * 0.5);
    ctx.stroke();
}

/// The bubble the phone shows over a pressed letter, so the finger doesn't
/// hide which key it is on: the letter, larger, on a raised key face just
/// above the pressed key.
pub(super) fn paint_preview(ctx: &mut dyn DrawCtx, key: Rect, text: &str, style: &Style) {
    let width = key.width + style.key_h_gap * 2.0;
    let height = key.height * 1.1;
    let x = key.x + key.width / 2.0 - width / 2.0;
    let y = key.y + key.height + style.key_v_gap * 0.5;
    let radius = style.key_corner_radius * 1.4;
    ctx.set_fill_color(style.panel_top_border);
    ctx.begin_path();
    ctx.rounded_rect(x - 1.0, y - 1.5, width + 2.0, height + 2.5, radius + 1.0);
    ctx.fill();
    ctx.set_fill_color(style.preview_bg);
    ctx.begin_path();
    ctx.rounded_rect(x, y, width, height, radius);
    ctx.fill();
    let size = style.letter_font_size * 1.35;
    ctx.set_font_size(size);
    ctx.set_fill_color(style.key_face_text);
    let w = ctx
        .measure_text(text)
        .map(|m| m.width)
        .unwrap_or(size * 0.55);
    ctx.fill_text(
        text,
        x + width / 2.0 - w / 2.0,
        y + height / 2.0 - size * 0.3,
    );
}
