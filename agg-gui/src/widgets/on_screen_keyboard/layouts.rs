//! Keyboard layouts — declarative tables of rows / keys per layer.
//!
//! Adding a new layer (e.g. a French AZERTY, an emoji picker, a search-
//! optimised "go" button) is a data change here: define a new
//! [`Layer`] variant and return a `Layout` from
//! [`Layout::for_layer`]. The painter and hit-tester don't change.

use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect};

use super::key::{KeyAction, KeyCap, KeyGlyph, PaintedKey};
use super::style::Style;

/// Which layer of the keyboard is currently visible.
///
/// "Shifted" is a one-shot upper-case mode (single tap of Shift); we'll
/// add a `CapsLocked` variant later for double-tap behavior. "Numbers"
/// holds digits + the most-used punctuation. "Symbols" is the third
/// page reached from inside "Numbers".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Letters,
    Shifted,
    Numbers,
    Symbols,
}

/// Description of one key in a row — width is expressed in
/// "letter-widths". A standard letter is 1.0; Shift / Backspace are
/// usually 1.5; the spacebar is wide (e.g. 5.0 on iOS).
#[derive(Debug, Clone)]
struct KeySpec {
    width_units: f64,
    cap: KeyCap,
    action: KeyAction,
    kind: KeyKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    /// Letter / digit / punctuation — uses `key_face_*` style tokens.
    Letter,
    /// Shift / mode-switch / backspace / dismiss — uses `util_*`
    /// tokens.
    Utility,
    /// Return key — uses `return_*` tokens.
    Return,
}

/// A laid-out layer, ready to paint. Captured so the paint and the
/// hit-test layer-aware logic share a single source of truth.
pub struct Layout {
    rows: Vec<Vec<KeySpec>>,
}

impl Layout {
    pub fn for_layer(layer: Layer) -> Self {
        match layer {
            Layer::Letters => letters_layer(false),
            Layer::Shifted => letters_layer(true),
            Layer::Numbers => numbers_layer(),
            Layer::Symbols => symbols_layer(),
        }
    }

    /// Compute the panel height required to render this layout at the
    /// given viewport width, accounting for vertical padding and row
    /// gaps.
    pub fn compute_panel_height(&self, _viewport_width: f64, style: &Style) -> f64 {
        let rows = self.rows.len() as f64;
        style.panel_padding_top
            + style.panel_padding_bottom
            + rows * style.row_height
            + (rows - 1.0).max(0.0) * style.key_v_gap
    }

    /// Paint every key and return the on-screen hit rects (used by the
    /// tap dispatcher).
    pub fn paint(
        &self,
        ctx: &mut dyn DrawCtx,
        panel: Rect,
        style: &Style,
        active_layer: Layer,
    ) -> Vec<PaintedKey> {
        let mut painted = Vec::with_capacity(self.rows.iter().map(|r| r.len()).sum());

        let inner_x = panel.x + style.panel_padding_horizontal;
        let inner_w = panel.width - 2.0 * style.panel_padding_horizontal;

        // Rows paint top-to-bottom visually. Y-up means the top of the
        // panel is at panel.y + panel.height; descend by row_height +
        // gap per row.
        let mut row_top_y = panel.y + panel.height - style.panel_padding_top;

        for (row_index, row) in self.rows.iter().enumerate() {
            let row_bottom_y = row_top_y - style.row_height;
            let total_units: f64 = row.iter().map(|k| k.width_units).sum();
            let total_gaps = (row.len() as f64 - 1.0).max(0.0) * style.key_h_gap;
            let key_unit_width = (inner_w - total_gaps) / total_units.max(0.001);

            let mut cursor_x = inner_x;
            for spec in row.iter() {
                let kw = spec.width_units * key_unit_width;
                let rect = Rect::new(cursor_x, row_bottom_y, kw, style.row_height);

                let pressed = false; // pressed visuals come from hover state, painted later
                paint_key(ctx, rect, spec, pressed, style, active_layer);

                painted.push(PaintedKey {
                    rect,
                    action: spec.action,
                    cap: spec.cap.clone(),
                });
                cursor_x += kw + style.key_h_gap;
            }

            if row_index + 1 < self.rows.len() {
                row_top_y = row_bottom_y - style.key_v_gap;
            }
        }

        painted
    }
}

// ---------------------------------------------------------------------------
// Layer definitions
// ---------------------------------------------------------------------------

fn letters_layer(shifted: bool) -> Layout {
    let case = |lower: char, upper: char| if shifted { upper } else { lower };

    let row_keys = |letters: &[(char, char)]| -> Vec<KeySpec> {
        letters
            .iter()
            .map(|(lo, up)| {
                let c = case(*lo, *up);
                KeySpec {
                    width_units: 1.0,
                    cap: KeyCap::Text(c.to_string()),
                    action: KeyAction::Char(c),
                    kind: KeyKind::Letter,
                }
            })
            .collect()
    };

    let mut rows: Vec<Vec<KeySpec>> = Vec::with_capacity(4);
    rows.push(row_keys(&[
        ('q', 'Q'),
        ('w', 'W'),
        ('e', 'E'),
        ('r', 'R'),
        ('t', 'T'),
        ('y', 'Y'),
        ('u', 'U'),
        ('i', 'I'),
        ('o', 'O'),
        ('p', 'P'),
    ]));

    let mut row2 = row_keys(&[
        ('a', 'A'),
        ('s', 'S'),
        ('d', 'D'),
        ('f', 'F'),
        ('g', 'G'),
        ('h', 'H'),
        ('j', 'J'),
        ('k', 'K'),
        ('l', 'L'),
    ]);
    // iOS pads row 2 with half-key gaps; emulate by adding 0.5-width
    // invisible spacers at each end. Easier: keep row 2 9 keys wide,
    // which means the layout engine will auto-fit. The visual offset
    // emerges from the row-2 letter count being one less than row 1.
    rows.push(row2);

    let mut row3: Vec<KeySpec> = Vec::with_capacity(11);
    row3.push(KeySpec {
        width_units: 1.5,
        cap: KeyCap::Glyph(KeyGlyph::Shift),
        action: KeyAction::Switch(if shifted { Layer::Letters } else { Layer::Shifted }),
        kind: KeyKind::Utility,
    });
    row3.extend(row_keys(&[
        ('z', 'Z'),
        ('x', 'X'),
        ('c', 'C'),
        ('v', 'V'),
        ('b', 'B'),
        ('n', 'N'),
        ('m', 'M'),
    ]));
    row3.push(KeySpec {
        width_units: 1.5,
        cap: KeyCap::Glyph(KeyGlyph::Backspace),
        action: KeyAction::Backspace,
        kind: KeyKind::Utility,
    });
    rows.push(row3);

    rows.push(action_row(if shifted { Layer::Shifted } else { Layer::Letters }));
    Layout { rows }
}

fn numbers_layer() -> Layout {
    let digit = |c: char| KeySpec {
        width_units: 1.0,
        cap: KeyCap::Text(c.to_string()),
        action: KeyAction::Char(c),
        kind: KeyKind::Letter,
    };

    let mut rows = Vec::with_capacity(4);
    rows.push(['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'].iter().map(|c| digit(*c)).collect());
    rows.push(['-', '/', ':', ';', '(', ')', '$', '&', '@', '"'].iter().map(|c| digit(*c)).collect());

    let mut row3 = Vec::with_capacity(9);
    row3.push(KeySpec {
        width_units: 1.5,
        cap: KeyCap::Text("#+=".to_string()),
        action: KeyAction::Switch(Layer::Symbols),
        kind: KeyKind::Utility,
    });
    for c in ['.', ',', '?', '!', '\''] {
        row3.push(digit(c));
    }
    row3.push(KeySpec {
        width_units: 1.5,
        cap: KeyCap::Glyph(KeyGlyph::Backspace),
        action: KeyAction::Backspace,
        kind: KeyKind::Utility,
    });
    rows.push(row3);

    rows.push(action_row(Layer::Numbers));
    Layout { rows }
}

fn symbols_layer() -> Layout {
    let sym = |c: char| KeySpec {
        width_units: 1.0,
        cap: KeyCap::Text(c.to_string()),
        action: KeyAction::Char(c),
        kind: KeyKind::Letter,
    };

    let mut rows = Vec::with_capacity(4);
    rows.push(['[', ']', '{', '}', '#', '%', '^', '*', '+', '='].iter().map(|c| sym(*c)).collect());
    rows.push(['_', '\\', '|', '~', '<', '>', '€', '£', '¥', '·'].iter().map(|c| sym(*c)).collect());

    let mut row3 = Vec::with_capacity(9);
    row3.push(KeySpec {
        width_units: 1.5,
        cap: KeyCap::Text("123".to_string()),
        action: KeyAction::Switch(Layer::Numbers),
        kind: KeyKind::Utility,
    });
    for c in ['.', ',', '?', '!', '\''] {
        row3.push(sym(c));
    }
    row3.push(KeySpec {
        width_units: 1.5,
        cap: KeyCap::Glyph(KeyGlyph::Backspace),
        action: KeyAction::Backspace,
        kind: KeyKind::Utility,
    });
    rows.push(row3);

    rows.push(action_row(Layer::Symbols));
    Layout { rows }
}

/// The bottom row of every layer: mode switcher, space, return, and a
/// dismiss key. `current` is the layer the row sits under; the mode key
/// label / target is derived from where the user would expect to go
/// next (letters → numbers, numbers/symbols → letters, shifted → numbers).
fn action_row(current: Layer) -> Vec<KeySpec> {
    let (mode_label, mode_action) = match current {
        Layer::Letters | Layer::Shifted => ("123", KeyAction::Switch(Layer::Numbers)),
        Layer::Numbers | Layer::Symbols => ("ABC", KeyAction::Switch(Layer::Letters)),
    };
    vec![
        KeySpec {
            width_units: 1.5,
            cap: KeyCap::Text(mode_label.to_string()),
            action: mode_action,
            kind: KeyKind::Utility,
        },
        KeySpec {
            width_units: 1.0,
            cap: KeyCap::Glyph(KeyGlyph::DismissDown),
            action: KeyAction::Dismiss,
            kind: KeyKind::Utility,
        },
        KeySpec {
            width_units: 5.0,
            cap: KeyCap::Text("space".to_string()),
            action: KeyAction::Space,
            kind: KeyKind::Letter,
        },
        KeySpec {
            width_units: 2.0,
            cap: KeyCap::Glyph(KeyGlyph::Return),
            action: KeyAction::Enter,
            kind: KeyKind::Return,
        },
    ]
}

// ---------------------------------------------------------------------------
// Key painting
// ---------------------------------------------------------------------------

fn paint_key(
    ctx: &mut dyn DrawCtx,
    rect: Rect,
    spec: &KeySpec,
    pressed: bool,
    style: &Style,
    _active_layer: Layer,
) {
    let (bg, text_color) = match (spec.kind, pressed) {
        (KeyKind::Letter, false) => (style.key_face_bg, style.key_face_text),
        (KeyKind::Letter, true) => (style.key_face_bg_pressed, style.key_face_text_pressed),
        (KeyKind::Utility, false) => (style.util_key_bg, style.util_key_text),
        (KeyKind::Utility, true) => (style.util_key_bg_pressed, style.key_face_text_pressed),
        (KeyKind::Return, false) => (style.return_key_bg, style.return_key_text),
        (KeyKind::Return, true) => (style.return_key_bg_pressed, style.return_key_text),
    };

    // Faux 1-pixel drop shadow (Y-up: shadow_offset_y is negative).
    ctx.set_fill_color(style.key_shadow);
    ctx.begin_path();
    ctx.rounded_rect(
        rect.x,
        rect.y + style.key_shadow_offset_y,
        rect.width,
        rect.height,
        style.key_corner_radius,
    );
    ctx.fill();

    ctx.set_fill_color(bg);
    ctx.begin_path();
    ctx.rounded_rect(
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        style.key_corner_radius,
    );
    ctx.fill();

    ctx.set_fill_color(text_color);
    let center = Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);

    match &spec.cap {
        KeyCap::Text(text) => {
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
            paint_glyph(ctx, center, style, *glyph, text_color);
        }
    }
}

fn paint_glyph(
    ctx: &mut dyn DrawCtx,
    center: Point,
    style: &Style,
    glyph: super::key::KeyGlyph,
    color: crate::color::Color,
) {
    use super::key::KeyGlyph;
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
            ctx.stroke();
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_layer_has_four_rows() {
        let l = Layout::for_layer(Layer::Letters);
        assert_eq!(l.rows.len(), 4);
    }

    #[test]
    fn shift_key_switches_layer() {
        let l = Layout::for_layer(Layer::Letters);
        let row3 = &l.rows[2];
        let shift = &row3[0];
        match shift.action {
            KeyAction::Switch(Layer::Shifted) => {}
            other => panic!("expected Switch(Shifted) on row3[0], got {other:?}"),
        }
    }

    #[test]
    fn shifted_layer_emits_uppercase_chars() {
        let l = Layout::for_layer(Layer::Shifted);
        // First row, first key: should be 'Q'.
        let q = &l.rows[0][0];
        match q.action {
            KeyAction::Char('Q') => {}
            other => panic!("expected Char('Q'), got {other:?}"),
        }
    }

    #[test]
    fn numbers_layer_includes_digits() {
        let l = Layout::for_layer(Layer::Numbers);
        let chars: Vec<char> = l.rows[0]
            .iter()
            .filter_map(|k| match k.action {
                KeyAction::Char(c) => Some(c),
                _ => None,
            })
            .collect();
        for d in ['1', '2', '3', '0'] {
            assert!(chars.contains(&d), "missing digit {d} in numbers row 1");
        }
    }
}
