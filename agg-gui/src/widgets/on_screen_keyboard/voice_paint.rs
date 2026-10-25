//! Where the microphone key sits, and how it and the dictation status line
//! draw, for the keyboard's voice input (`voice.rs`, `voice_session.rs`).
//!
//! The key goes where each platform's keyboard has it:
//!
//! - **Gboard**: at the right end of the toolbar row above the keys
//!   (`docs/Android Keyboard/standard.png`): a white circle 34 dp across
//!   with the microphone in the key-label colour. The row exists only while
//!   a provider is registered and the field isn't a password field
//!   ([`super::layouts::Layout::toolbar_height`]);
//!   its six equal slots are Gboard's, the microphone owning the last, and
//!   the rest of the row holds the status line ("Speak now", progress, an
//!   error) and takes touches without typing.
//! - **iPhone**: in the strip below the keys at the right, under Return,
//!   opposite the emoji key, drawn without a key face like the emoji key;
//!   the status line sits in the strip between them, which takes touches
//!   and types nothing.
//!
//! The microphone is Font Awesome's (`icon_font`), slashed when voice input
//! is unavailable or failed. Called from [`super::layout_paint`], which
//! records the keys this module places as painted keys for the hit-tester.

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect};
use crate::icon_font;

use super::style::Style;
use super::voice::VoiceState;
use super::voice_session::VoiceView;

/// Gboard's toolbar has six equal slots; the microphone takes the last.
const TOOLBAR_SLOTS: f64 = 6.0;

/// The microphone glyph's height, as a fraction of its key's diameter
/// (Gboard: an 18.9 dp glyph in a 34 dp circle).
const GLYPH_OF_FACE: f64 = 0.56;

/// The microphone key and the status line in Gboard's toolbar row, the top
/// `toolbar_h` of `panel`.
pub(super) struct ToolbarRects {
    /// The microphone's drawn circle.
    pub mic_face: Rect,
    /// The microphone's slot: the toolbar's last sixth.
    pub mic_hit: Rect,
    /// The rest of the toolbar, which takes touches and types nothing.
    pub status_hit: Rect,
    /// Where the status line draws.
    pub status_text: Rect,
}

pub(super) fn toolbar_rects(panel: Rect, toolbar_h: f64, style: &Style) -> ToolbarRects {
    let y = panel.y + panel.height - toolbar_h;
    let slot = panel.width / TOOLBAR_SLOTS;
    let mic_hit = Rect::new(panel.x + panel.width - slot, y, slot, toolbar_h);
    let d = style.voice_key_diameter.min(toolbar_h).min(slot);
    let right = panel.x + panel.width - style.panel_padding_horizontal;
    let cx = (right - d / 2.0).max(mic_hit.x + d / 2.0);
    let mic_face = Rect::new(cx - d / 2.0, y + (toolbar_h - d) / 2.0, d, d);
    let status_hit = Rect::new(panel.x, y, panel.width - slot, toolbar_h);
    let status_text = Rect::new(
        panel.x + style.panel_padding_horizontal,
        y,
        (status_hit.width - style.panel_padding_horizontal).max(0.0),
        toolbar_h,
    );
    ToolbarRects {
        mic_face,
        mic_hit,
        status_hit,
        status_text,
    }
}

/// The iPhone microphone's drawn face and hit area in the strip below the
/// keys, under the bottom row's last key (the emoji key's mirror image, see
/// `layout_paint::chevron_rects`). Every bottom-row key's hit area is raised
/// to the strip's top: the strip between the emoji key and the microphone
/// holds the status line and types nothing. `None` without a strip or a
/// row.
pub(super) fn strip_mic_rects(
    faces: &[Vec<Rect>],
    hits: &mut [Vec<Rect>],
    panel: Rect,
    style: &Style,
) -> Option<(Rect, Rect)> {
    let strip = style.bottom_strip_height;
    if strip <= 0.0 {
        return None;
    }
    let last_face = *faces.last()?.last()?;
    let last_hit = *hits.last()?.last()?;
    let hit = Rect::new(
        last_hit.x,
        panel.y,
        panel.x + panel.width - last_hit.x,
        strip,
    );
    let strip_top = panel.y + strip;
    for row_hit in hits.last_mut()?.iter_mut() {
        let top = row_hit.y + row_hit.height;
        row_hit.y = row_hit.y.max(strip_top);
        row_hit.height = top - row_hit.y;
    }
    let size = strip * 0.8;
    let face = Rect::new(
        last_face.x + last_face.width / 2.0 - size / 2.0,
        panel.y + (strip - size) / 2.0,
        size,
        size,
    );
    Some((face, hit))
}

/// Draw the microphone key in `face` for `view`. `with_face` draws Gboard's
/// round key face; without it (the iPhone strip) the glyph sits on the
/// panel, with a round highlight while pressed or listening.
pub(super) fn paint_voice_key(
    ctx: &mut dyn DrawCtx,
    face: Rect,
    view: &VoiceView,
    pressed: bool,
    with_face: bool,
    style: &Style,
) {
    let c = Point::new(face.x + face.width / 2.0, face.y + face.height / 2.0);
    let r = face.width.min(face.height) / 2.0;
    let lit = view.state.is_active();
    if lit {
        // The level swells a halo behind the lit key.
        ctx.set_fill_color(style.alt_highlight_bg.with_alpha(0.25));
        ctx.begin_path();
        ctx.circle(c.x, c.y, r * (1.0 + 0.3 * view.pulse));
        ctx.fill();
    }
    let disc = if lit {
        Some(style.alt_highlight_bg)
    } else if pressed {
        Some(style.key_face_bg_pressed)
    } else if with_face {
        Some(style.key_face_bg)
    } else {
        None
    };
    if let Some(color) = disc {
        ctx.set_fill_color(color);
        ctx.begin_path();
        ctx.circle(c.x, c.y, r);
        ctx.fill();
    }
    if let VoiceState::Preparing {
        progress: Some(progress),
    } = view.state
    {
        paint_progress_ring(ctx, c, r, progress as f64, style);
    }
    let ink = if lit {
        style.alt_highlight_text
    } else if with_face {
        style.key_face_text
    } else {
        style.util_key_text
    };
    let (glyph, ink) = match view.state {
        VoiceState::Unavailable => (icon_font::MICROPHONE_SLASH, dimmed(ink)),
        VoiceState::Error { .. } => (icon_font::MICROPHONE_SLASH, ink),
        VoiceState::Preparing { .. } => (icon_font::MICROPHONE, dimmed(ink)),
        _ => (icon_font::MICROPHONE, ink),
    };
    let glyph_h = if with_face {
        2.0 * r * GLYPH_OF_FACE
    } else {
        2.0 * r * 0.5
    };
    paint_icon(ctx, glyph, c, glyph_h, ink);
}

/// A disabled key's ink.
fn dimmed(color: Color) -> Color {
    color.with_alpha(color.a * 0.35)
}

/// The preparing key's progress: an arc round its rim from twelve o'clock,
/// clockwise.
fn paint_progress_ring(ctx: &mut dyn DrawCtx, c: Point, r: f64, progress: f64, style: &Style) {
    let p = progress.clamp(0.0, 1.0);
    if p <= 0.0 {
        return;
    }
    let width = (r * 0.14).max(1.5);
    let start = std::f64::consts::FRAC_PI_2;
    let end = start - std::f64::consts::TAU * p;
    ctx.set_stroke_color(style.alt_highlight_bg);
    ctx.set_line_width(width);
    ctx.begin_path();
    ctx.arc_to(c.x, c.y, r - width / 2.0, start, end, false);
    ctx.stroke();
}

/// Draw Font Awesome `glyph` `height` tall, centred on `c`.
fn paint_icon(ctx: &mut dyn DrawCtx, glyph: char, c: Point, height: f64, color: Color) {
    let face = icon_font::type_face();
    let Some((x0, y0, x1, y1)) = face.glyph_visual_rect(glyph, 1.0) else {
        return;
    };
    if y1 - y0 <= 0.0 {
        return;
    }
    let size = height / (y1 - y0);
    ctx.save();
    ctx.set_font(face);
    ctx.set_font_size(size);
    ctx.set_fill_color(color);
    let text = glyph.to_string();
    ctx.fill_text(
        &text,
        c.x - size * (x0 + x1) / 2.0,
        c.y - size * (y0 + y1) / 2.0,
    );
    ctx.restore();
}

/// The status line's text for `view`, if it has one: the error message on
/// show, the preparing progress, or the listening prompt.
pub(super) fn status_text(view: &VoiceView) -> Option<String> {
    if let Some(message) = &view.banner {
        return Some(message.clone());
    }
    match view.state {
        VoiceState::Preparing { progress: Some(p) } => Some(format!(
            "Preparing voice input\u{2026} {}%",
            (p.clamp(0.0, 1.0) * 100.0).round()
        )),
        // A dictation waiting for the provider to start listening (its
        // model in, the microphone opening): the cue to speak comes later.
        VoiceState::Preparing { progress: None } if view.dictating => {
            Some("Starting\u{2026}".to_string())
        }
        VoiceState::Preparing { progress: None } => {
            Some("Preparing voice input\u{2026}".to_string())
        }
        VoiceState::Listening { .. } => Some("Speak now".to_string()),
        VoiceState::Finishing => Some("Finishing\u{2026}".to_string()),
        _ => None,
    }
}

/// Draw the status line left-aligned in `area` (centred when `centred`),
/// shrunk to fit its width. Uses the key labels' font.
pub(super) fn paint_status(
    ctx: &mut dyn DrawCtx,
    area: Rect,
    view: &VoiceView,
    centred: bool,
    style: &Style,
) {
    let Some(text) = status_text(view) else {
        return;
    };
    if area.width <= 0.0 {
        return;
    }
    let mut size = style.utility_font_size;
    ctx.set_font_size(size);
    let Some(width) = ctx.measure_text(&text).map(|m| m.width) else {
        return; // no font: nothing would draw
    };
    let width = if width > area.width && width > 0.0 {
        size *= area.width / width;
        ctx.set_font_size(size);
        area.width
    } else {
        width
    };
    let x = if centred {
        area.x + (area.width - width) / 2.0
    } else {
        area.x
    };
    let color = if view.banner.is_some() {
        style.key_face_text
    } else {
        style.util_key_text
    };
    ctx.set_fill_color(color);
    ctx.fill_text(&text, x, area.y + area.height / 2.0 - size * 0.3);
}
