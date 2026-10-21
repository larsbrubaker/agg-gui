//! The paint pass of a keyboard [`Layout`]: places every key, gives each its
//! hit area, draws the keys, the hide-keyboard chevron below them, and the
//! preview bubble over a pressed letter.
//!
//! Split out of [`super::layouts`] so the layout tables stay pure data. The
//! hit areas come from [`super::hit`]; one key's look from [`super::paint`].
//! The returned [`PaintedKey`]s are what [`super::pointer`] hit-tests.

use crate::draw_ctx::DrawCtx;
use crate::geometry::Rect;

use super::enter_action::EnterAction;
use super::hit::assign_row_hits;
use super::key::{KeyAction, KeyCap, KeyGlyph, PaintedKey};
use super::layouts::{KeyKind, KeySpec, Layer, Layout};
use super::paint::{paint_chevron, paint_key, paint_preview};
use super::style::Style;

/// What the paint pass needs to know about the keyboard's state.
#[derive(Debug, Clone, Copy)]
pub(super) struct PaintState {
    /// The visible layer (decides Shift's look).
    pub layer: Layer,
    /// Caps Lock is on (Shift shows a bar under its arrow).
    pub caps_lock: bool,
    /// Index into the painted keys of the key under the finger.
    pub pressed: Option<usize>,
    /// What Enter does in the focused field (the Enter key's icon).
    pub enter: EnterAction,
}

impl Layout {
    /// Paint every key and return the painted keys with their hit areas
    /// (used by the tap dispatcher). The hide-keyboard chevron is the last
    /// painted key.
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
        let mut row_top_y = panel.y + panel.height - style.panel_padding_top;
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
        let mut hits = assign_row_hits(&faces, panel);
        let strip_top = panel.y + style.bottom_strip_height;
        let chevron = chevron_rects(&faces, &mut hits, panel, strip_top, style);

        let mut painted = Vec::with_capacity(faces.iter().map(Vec::len).sum::<usize>() + 1);
        for ((row_faces, row_specs), row_hits) in faces.iter().zip(&specs).zip(&hits) {
            for ((face, spec), hit) in row_faces.iter().zip(row_specs).zip(row_hits) {
                let cap = match spec.kind {
                    KeyKind::Return => KeyCap::Glyph(KeyGlyph::for_enter(state.enter)),
                    _ => spec.cap.clone(),
                };
                let pressed = state.pressed == Some(painted.len());
                paint_key(ctx, *face, spec, &cap, pressed, style, state);
                painted.push(PaintedKey {
                    rect: *face,
                    hit: *hit,
                    action: spec.action,
                    cap,
                });
            }
        }

        if let Some((face, hit)) = chevron {
            let pressed = state.pressed == Some(painted.len());
            paint_chevron(ctx, face, pressed, style);
            painted.push(PaintedKey {
                rect: face,
                hit,
                action: KeyAction::Dismiss,
                cap: KeyCap::Glyph(KeyGlyph::DismissDown),
            });
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
                    paint_preview(ctx, key.rect, text, style);
                }
            }
        }
        painted
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
