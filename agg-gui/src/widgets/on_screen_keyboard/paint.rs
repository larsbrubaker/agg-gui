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

use super::key::KeyAction;
use super::key::{KeyCap, KeyGlyph};
use super::layout_paint::PaintState;
use super::layouts::{KeyKind, KeySpec, Layer, LayoutFamily};
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
    // iOS lights the Shift key (a letter-key face, dark arrow) while upper
    // case is on.
    let ios_shift_lit = state.family == LayoutFamily::Ios
        && state.layer == Layer::Shifted
        && matches!(spec.action, KeyAction::Switch(Layer::Letters));
    let (bg, text_color) = if ios_shift_lit {
        (style.key_face_bg, style.key_face_text)
    } else {
        (bg, text_color)
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

    // The iPhone's trackpad fades the labels off the keys.
    let text_color = text_color.with_alpha(text_color.a * state.label_alpha as f32);
    if text_color.a <= 0.0 {
        return;
    }
    ctx.set_fill_color(text_color);
    let center = Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);

    match cap {
        KeyCap::Text(text) => {
            if text.is_empty() {
                return; // blank spacebar
            }
            let font_size = cap_font_size(ctx, text, spec.kind, rect, style);
            ctx.set_font_size(font_size);
            // `measure_text` is `None` only when the context has no font,
            // and then no text draws at all; the estimate just keeps the
            // arithmetic whole.
            let width = ctx
                .measure_text(text)
                .map(|m| m.width)
                .unwrap_or(text.chars().count() as f64 * font_size * 0.55);
            ctx.fill_text(text, center.x - width / 2.0, center.y - font_size * 0.3);
        }
        KeyCap::Glyph(glyph) => {
            paint_glyph(ctx, center, style, *glyph, text_color, state);
        }
    }
}

/// The font size a key's text label draws at. Every one-character label
/// (letters, digits, symbols) uses the one letter size, so the row reads
/// evenly whatever each glyph's width; only a word ("?123", "space",
/// "return") shrinks, as a whole, when it is wider than its key.
pub(super) fn cap_font_size(
    ctx: &mut dyn DrawCtx,
    text: &str,
    kind: KeyKind,
    rect: Rect,
    style: &Style,
) -> f64 {
    let single = text.chars().count() == 1;
    if single && matches!(kind, KeyKind::Letter) {
        return style.letter_font_size;
    }
    let size = style.utility_font_size;
    if single {
        return size;
    }
    ctx.set_font_size(size);
    let width = ctx
        .measure_text(text)
        .map(|m| m.width)
        .unwrap_or(text.chars().count() as f64 * size * 0.55);
    let room = rect.width - style.key_h_gap;
    if width > room && width > 0.0 {
        size * room / width
    } else {
        size
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
        KeyGlyph::Emoji => paint_smiley(ctx, center, r * 1.15),
        // The microphone has no key of this shape: `voice_paint.rs` draws it
        // with its state.
        KeyGlyph::Microphone => {}
    }
}

/// An outlined smiling face of radius `r`: the iPhone's emoji key.
fn paint_smiley(ctx: &mut dyn DrawCtx, center: Point, r: f64) {
    ctx.set_line_width(1.6);
    ctx.begin_path();
    ctx.circle(center.x, center.y, r);
    ctx.stroke();
    let eye = r * 0.13;
    for dx in [-0.35, 0.35] {
        ctx.begin_path();
        ctx.circle(center.x + r * dx, center.y + r * 0.28, eye);
        ctx.fill();
    }
    ctx.begin_path();
    ctx.move_to(center.x - r * 0.5, center.y - r * 0.12);
    ctx.quad_to(
        center.x,
        center.y - r * 0.75,
        center.x + r * 0.5,
        center.y - r * 0.12,
    );
    ctx.stroke();
}

/// The key in the strip below the keys, drawn on the panel with no key
/// face and a round highlight while pressed: Gboard's small downward
/// hide-keyboard chevron, or the iPhone's emoji face.
pub(super) fn paint_chevron(
    ctx: &mut dyn DrawCtx,
    face: Rect,
    glyph: KeyGlyph,
    pressed: bool,
    style: &Style,
) {
    let center = Point::new(face.x + face.width / 2.0, face.y + face.height / 2.0);
    if pressed {
        ctx.set_fill_color(style.util_key_bg_pressed);
        ctx.begin_path();
        ctx.circle(center.x, center.y, face.width.min(face.height) / 2.0);
        ctx.fill();
    }
    if matches!(glyph, KeyGlyph::Emoji) {
        ctx.set_stroke_color(style.util_key_text);
        ctx.set_fill_color(style.util_key_text);
        paint_smiley(ctx, center, face.height * 0.32);
        return;
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

/// The bubble Gboard shows over a pressed letter, so the finger doesn't
/// hide which key it is on: the letter, larger, on a round face with a soft
/// shadow just above the pressed key. One letter fits a circle 1.4 key
/// widths across, centred over the key; a label too wide for it stretches
/// the circle into a pill of the same height.
pub(super) fn paint_preview(ctx: &mut dyn DrawCtx, key: Rect, text: &str, style: &Style) {
    let size = style.letter_font_size * 1.35;
    ctx.set_font_size(size);
    let text_w = ctx
        .measure_text(text)
        .map(|m| m.width)
        .unwrap_or(size * 0.55);
    let height = key.width * PREVIEW_DIAMETER_KEYS;
    let radius = height / 2.0;
    // Room for the label with a radius-sized cap at each end.
    let width = height.max(text_w + radius);
    let x = key.x + key.width / 2.0 - width / 2.0;
    let y = key.y + key.height + style.key_v_gap * 0.5;
    paint_preview_shadow(ctx, Rect::new(x, y, width, height), style);
    ctx.set_fill_color(style.preview_bg);
    ctx.begin_path();
    ctx.rounded_rect(x, y, width, height, radius);
    ctx.fill();
    ctx.set_fill_color(style.key_face_text);
    ctx.fill_text(
        text,
        x + width / 2.0 - text_w / 2.0,
        y + height / 2.0 - size * 0.3,
    );
}

/// The Gboard preview's diameter, in key widths.
const PREVIEW_DIAMETER_KEYS: f64 = 1.4;

/// A soft shadow under the preview's `face`: faint rings that grow past its
/// edge, a little lower than the face so it reads as raised above the keys.
fn paint_preview_shadow(ctx: &mut dyn DrawCtx, face: Rect, style: &Style) {
    const RINGS: usize = 4;
    const SPREAD: f64 = 3.0;
    const DROP: f64 = 1.5;
    ctx.set_fill_color(style.preview_shadow);
    for ring in (1..=RINGS).rev() {
        let grow = SPREAD * ring as f64 / RINGS as f64;
        ctx.begin_path();
        ctx.rounded_rect(
            face.x - grow,
            face.y - DROP - grow,
            face.width + grow * 2.0,
            face.height + grow * 2.0,
            face.height / 2.0 + grow,
        );
        ctx.fill();
    }
}
