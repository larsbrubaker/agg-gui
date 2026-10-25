//! The paint pass of a keyboard [`Layout`]: places every key, gives each its
//! hit area, draws the keys, the key in the strip below them (Gboard's
//! hide-keyboard chevron, the iPhone's emoji key), the microphone key and
//! the dictation status while a voice provider is registered (placed and
//! drawn by [`super::voice_paint`]), the preview over a pressed letter and
//! the long-press alternates popup.
//!
//! Split out of [`super::layouts`] so the layout tables stay pure data. The
//! hit areas come from [`super::hit`]; one key's look from [`super::paint`].
//! The returned [`PaintedKey`]s are what [`super::pointer`] hit-tests.

use crate::draw_ctx::DrawCtx;
use crate::geometry::Rect;

use super::alternates::AlternatesPopup;
use super::alternates_paint::{paint_alternates, paint_ios_balloon};
use super::enter_action::EnterAction;
use super::hit::assign_row_hits;
use super::key::{KeyAction, KeyCap, KeyGlyph, PaintedKey};
use super::layouts::{KeyKind, KeySpec, Layer, Layout, LayoutFamily};
use super::paint::{paint_chevron, paint_key, paint_preview};
use super::style::Style;
use super::voice_paint::{paint_status, paint_voice_key, strip_mic_rects, toolbar_rects};
use super::voice_session::VoiceView;

/// What the paint pass needs to know about the keyboard's state.
#[derive(Debug, Clone)]
pub(super) struct PaintState {
    /// The visible layer (decides Shift's look).
    pub layer: Layer,
    /// Caps Lock is on (Shift shows a bar under its arrow).
    pub caps_lock: bool,
    /// Index into the painted keys of the key under the finger.
    pub pressed: Option<usize>,
    /// What Enter does in the focused field (the Enter key's icon).
    pub enter: EnterAction,
    /// The platform the layout draws (Enter's label, Shift's look, the
    /// preview and popup style).
    pub family: LayoutFamily,
    /// The open long-press alternates popup.
    pub alternates: Option<AlternatesPopup>,
    /// The microphone key's state; `None` without a voice provider (no key).
    pub voice: Option<VoiceView>,
    /// How opaque the key labels draw (`0.0` while the iPhone's trackpad
    /// has faded them).
    pub label_alpha: f64,
}

/// The Enter key's cap and colours for what Enter does: Gboard draws an
/// icon; iOS writes a word, on its blue key for the actions that finish
/// something (done, search, go, send) and on a grey function key otherwise.
fn enter_cap(family: LayoutFamily, action: EnterAction) -> (KeyCap, KeyKind) {
    if family == LayoutFamily::Gboard {
        return (KeyCap::Glyph(KeyGlyph::for_enter(action)), KeyKind::Return);
    }
    let (word, blue) = match action {
        EnterAction::Newline => ("return", false),
        EnterAction::Done => ("done", true),
        EnterAction::Search => ("search", true),
        EnterAction::Go => ("go", true),
        EnterAction::Send => ("send", true),
        EnterAction::Next => ("next", false),
        EnterAction::Previous => ("previous", false),
    };
    let kind = if blue {
        KeyKind::Return
    } else {
        KeyKind::Utility
    };
    (KeyCap::Text(word.to_string()), kind)
}

impl Layout {
    /// Paint every key and return the painted keys with their hit areas
    /// (used by the tap dispatcher). The hide-keyboard chevron follows the
    /// rows' keys, then the voice keys when there is a provider: the
    /// microphone, and on Gboard the inert rest of its toolbar row.
    pub(super) fn paint(
        &self,
        ctx: &mut dyn DrawCtx,
        panel: Rect,
        style: &Style,
        state: &PaintState,
    ) -> Vec<PaintedKey> {
        let inner_x = panel.x + style.panel_padding_horizontal;
        let inner_w = panel.width - 2.0 * style.panel_padding_horizontal;

        // Place the drawn faces. Rows run top to bottom; Y-up means the top
        // of the panel is at panel.y + panel.height.
        let mut faces: Vec<Vec<Rect>> = Vec::with_capacity(self.rows.len());
        let mut specs: Vec<Vec<&KeySpec>> = Vec::with_capacity(self.rows.len());
        // Gboard's toolbar row, while it has the microphone, sits above the
        // keys; the keys and their hit areas fill the panel below it.
        let toolbar_h = if state.voice.is_some() {
            self.toolbar_height(style)
        } else {
            0.0
        };
        let keys_area = Rect::new(panel.x, panel.y, panel.width, panel.height - toolbar_h);
        let mut row_top_y = keys_area.y + keys_area.height - style.panel_padding_top;
        for row in &self.rows {
            let row_bottom_y = row_top_y - style.row_height;
            // Each unit of width is one key pitch (a key and one gap), so a
            // key `n` units wide is `n` pitches less a gap wide (Gboard's
            // 1.5-unit Shift spans one and a half letters and their gap),
            // and a half-unit spacer offsets the home row by half a pitch.
            let total_units: f64 = row.iter().map(|k| k.width_units).sum();
            let pitch = (inner_w + style.key_h_gap) / total_units.max(0.001);
            let mut cursor_x = inner_x;
            let (mut row_faces, mut row_specs) = (Vec::new(), Vec::new());
            for spec in row {
                let room = spec.width_units * pitch;
                if spec.kind != KeyKind::Spacer {
                    let kw = room - style.key_h_gap;
                    row_faces.push(Rect::new(cursor_x, row_bottom_y, kw, style.row_height));
                    row_specs.push(spec);
                }
                cursor_x += room;
            }
            faces.push(row_faces);
            specs.push(row_specs);
            row_top_y = row_bottom_y - style.key_v_gap;
        }

        // Hit areas tile the whole panel: the bottom row reaches down
        // through the chevron strip, except the strip's left end, which is
        // the chevron's own.
        let mut hits = assign_row_hits(&faces, keys_area);
        let strip_top = panel.y + style.bottom_strip_height;
        let chevron = chevron_rects(&faces, &mut hits, panel, strip_top, style);
        // The iPhone's microphone takes the strip's right end the same way.
        let strip_mic = match (&state.voice, state.family) {
            (Some(_), LayoutFamily::Ios) => strip_mic_rects(&faces, &mut hits, panel, style),
            _ => None,
        };

        let mut painted = Vec::with_capacity(faces.iter().map(Vec::len).sum::<usize>() + 1);
        for ((row_faces, row_specs), row_hits) in faces.iter().zip(&specs).zip(&hits) {
            for ((face, spec), hit) in row_faces.iter().zip(row_specs).zip(row_hits) {
                let (cap, kind) = match spec.kind {
                    KeyKind::Return => enter_cap(state.family, state.enter),
                    kind => (spec.cap.clone(), kind),
                };
                let drawn = KeySpec {
                    kind,
                    ..(*spec).clone()
                };
                let pressed = state.pressed == Some(painted.len());
                paint_key(ctx, *face, &drawn, &cap, pressed, style, state);
                painted.push(PaintedKey {
                    rect: *face,
                    hit: *hit,
                    action: spec.action,
                    cap,
                });
            }
        }

        // The strip key: Gboard's hide chevron; the iPhone's emoji key,
        // which opens the symbols page until there is an emoji page.
        if let Some((face, hit)) = chevron {
            let pressed = state.pressed == Some(painted.len());
            let (action, glyph) = match state.family {
                LayoutFamily::Gboard => (KeyAction::Dismiss, KeyGlyph::DismissDown),
                LayoutFamily::Ios => (KeyAction::Switch(Layer::Symbols), KeyGlyph::Emoji),
            };
            paint_chevron(ctx, face, glyph, pressed, style);
            painted.push(PaintedKey {
                rect: face,
                hit,
                action,
                cap: KeyCap::Glyph(glyph),
            });
        }

        if let Some(voice) = &state.voice {
            if toolbar_h > 0.0 {
                let bar = toolbar_rects(panel, toolbar_h, style);
                let pressed = state.pressed == Some(painted.len());
                paint_voice_key(ctx, bar.mic_face, voice, pressed, true, style);
                paint_status(ctx, bar.status_text, voice, false, style);
                painted.push(voice_key(bar.mic_face, bar.mic_hit));
                painted.push(inert_key(bar.status_hit));
            } else if let Some((face, hit)) = strip_mic {
                let pressed = state.pressed == Some(painted.len());
                paint_voice_key(ctx, face, voice, pressed, false, style);
                // The status line sits in the strip between the emoji key
                // and the microphone, which takes touches and types nothing.
                let left = chevron.map_or(panel.x, |(_, h)| h.x + h.width);
                let area = Rect::new(left, panel.y, hit.x - left, style.bottom_strip_height);
                paint_status(ctx, area, voice, true, style);
                painted.push(voice_key(face, hit));
                painted.push(inert_key(area));
            }
        }

        // The alternates popup replaces the preview while it is open.
        if let Some(popup) = &state.alternates {
            paint_alternates(ctx, panel, popup, style, state.family);
            return painted;
        }

        // The preview bubble paints last so it sits over the row above.
        if let Some(key) = state.pressed.and_then(|i| painted.get(i)) {
            let previewed = specs
                .iter()
                .flatten()
                .nth(state.pressed.unwrap_or(0))
                .is_some_and(|s| s.kind == KeyKind::Letter);
            if let (true, KeyCap::Text(text)) = (previewed, &key.cap) {
                if text.chars().count() == 1 {
                    match state.family {
                        LayoutFamily::Gboard => paint_preview(ctx, key.rect, text, style),
                        LayoutFamily::Ios => paint_ios_balloon(ctx, panel, key.rect, text, style),
                    }
                }
            }
        }
        painted
    }
}

/// The microphone key's painted entry.
fn voice_key(rect: Rect, hit: Rect) -> PaintedKey {
    PaintedKey {
        rect,
        hit,
        action: KeyAction::Voice,
        cap: KeyCap::Glyph(KeyGlyph::Microphone),
    }
}

/// The area beside the microphone that holds the status line: it takes
/// touches and types nothing.
fn inert_key(area: Rect) -> PaintedKey {
    PaintedKey {
        rect: area,
        hit: area,
        action: KeyAction::Inert,
        cap: KeyCap::Text(String::new()),
    }
}

/// The hide-keyboard chevron's drawn face and hit area, in the strip below
/// the keys under the bottom row's first key, and that key's hit area
/// raised to the strip's top so the two don't overlap. `None` without a
/// strip or a bottom row.
fn chevron_rects(
    faces: &[Vec<Rect>],
    hits: &mut [Vec<Rect>],
    panel: Rect,
    strip_top: f64,
    style: &Style,
) -> Option<(Rect, Rect)> {
    if style.bottom_strip_height <= 0.0 {
        return None;
    }
    let first_face = *faces.last()?.first()?;
    let first_hit = hits.last_mut()?.first_mut()?;
    let hit = Rect::new(
        panel.x,
        panel.y,
        first_hit.x + first_hit.width - panel.x,
        style.bottom_strip_height,
    );
    let top = first_hit.y + first_hit.height;
    first_hit.y = strip_top;
    first_hit.height = top - strip_top;
    let size = style.bottom_strip_height * 0.8;
    let face = Rect::new(
        first_face.x + first_face.width / 2.0 - size / 2.0,
        panel.y + (style.bottom_strip_height - size) / 2.0,
        size,
        size,
    );
    Some((face, hit))
}
