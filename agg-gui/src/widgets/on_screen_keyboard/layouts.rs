//! Keyboard layouts — declarative tables of rows / keys per layer.
//!
//! The layer set mirrors Google's Gboard (see `docs/Android Keyboard/`
//! screenshots): a letters page with a dedicated number row, a `?123`
//! symbols page, a `=\<` extended-symbols page, and a phone-style
//! number pad for numeric fields. Adding a new layer (e.g. a French
//! AZERTY, an emoji picker) is a data change here: define a new
//! [`Layer`] variant and return a `Layout` from [`Layout::for_layer`].
//! The painter ([`super::paint`]) and hit-tester don't change.

use crate::draw_ctx::DrawCtx;
use crate::geometry::Rect;

use super::key::{KeyAction, KeyCap, KeyGlyph, PaintedKey};
use super::paint::paint_key;
use super::style::Style;

/// Which layer of the keyboard is currently visible.
///
/// "Shifted" is a one-shot upper-case mode (single tap of Shift;
/// double-tap engages caps lock). The other layers follow Gboard's
/// paging: `?123` opens [`Layer::Symbols`], its `=\<` key opens
/// [`Layer::SymbolsShift`], and the `1234` key (or a numeric input
/// field) opens [`Layer::NumPad`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Letters,
    Shifted,
    /// Gboard's `?123` page — digits plus common symbols.
    Symbols,
    /// Gboard's `=\<` page — math / currency / bracket symbols.
    SymbolsShift,
    /// Phone-style digit grid with an operator strip; the layer numeric
    /// text fields open on.
    NumPad,
}

/// Description of one key in a row — width is expressed in
/// "letter-widths". A standard letter is 1.0; Shift / Backspace are
/// 1.5; the spacebar is wide (4.0 on Gboard).
#[derive(Debug, Clone)]
pub(super) struct KeySpec {
    pub(super) width_units: f64,
    pub(super) cap: KeyCap,
    pub(super) action: KeyAction,
    pub(super) kind: KeyKind,
    /// Fully-rounded ends (Gboard draws the corner keys — `?123`,
    /// `ABC`, enter — as pills / circles).
    pub(super) pill: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyKind {
    /// Letter / digit / symbol — uses `key_face_*` style tokens.
    Letter,
    /// Shift / mode-switch / backspace / punctuation accents — uses
    /// `util_*` tokens (the light-blue keys on Gboard).
    Utility,
    /// Enter / search key — uses `return_*` tokens.
    Return,
    /// Invisible filler used to center short rows (Gboard's home row).
    /// Never painted, never hit-testable.
    Spacer,
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
            Layer::Symbols => symbols_layer(),
            Layer::SymbolsShift => symbols_shift_layer(),
            Layer::NumPad => numpad_layer(),
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
                if spec.kind == KeyKind::Spacer {
                    cursor_x += kw + style.key_h_gap;
                    continue;
                }
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
// Key constructors
// ---------------------------------------------------------------------------

fn key(c: char) -> KeySpec {
    KeySpec {
        width_units: 1.0,
        cap: KeyCap::Text(c.to_string()),
        action: KeyAction::Char(c),
        kind: KeyKind::Letter,
        pill: false,
    }
}

fn wide_key(c: char, width_units: f64) -> KeySpec {
    KeySpec {
        width_units,
        ..key(c)
    }
}

/// A character key with the light-blue utility face (Gboard paints the
/// `,` / `.` keys and the numpad operator strip this way).
fn util_char(c: char, width_units: f64) -> KeySpec {
    KeySpec {
        width_units,
        kind: KeyKind::Utility,
        ..key(c)
    }
}

fn util_text(label: &str, action: KeyAction, width_units: f64) -> KeySpec {
    KeySpec {
        width_units,
        cap: KeyCap::Text(label.to_string()),
        action,
        kind: KeyKind::Utility,
        pill: false,
    }
}

fn util_glyph(glyph: KeyGlyph, action: KeyAction, width_units: f64) -> KeySpec {
    KeySpec {
        width_units,
        cap: KeyCap::Glyph(glyph),
        action,
        kind: KeyKind::Utility,
        pill: false,
    }
}

fn spacer(width_units: f64) -> KeySpec {
    KeySpec {
        width_units,
        cap: KeyCap::Text(String::new()),
        action: KeyAction::Dismiss, // never dispatched — spacers aren't hit-testable
        kind: KeyKind::Spacer,
        pill: false,
    }
}

/// Bottom-corner mode key (`?123` / `ABC`) — pill-shaped like Gboard's.
fn mode_pill(label: &str, target: Layer) -> KeySpec {
    KeySpec {
        pill: true,
        ..util_text(label, KeyAction::Switch(target), 1.5)
    }
}

fn space_bar(width_units: f64) -> KeySpec {
    KeySpec {
        width_units,
        // Gboard's spacebar is blank (no "space" label).
        cap: KeyCap::Text(String::new()),
        action: KeyAction::Space,
        kind: KeyKind::Letter,
        pill: false,
    }
}

fn enter_key() -> KeySpec {
    KeySpec {
        width_units: 1.5,
        cap: KeyCap::Glyph(KeyGlyph::Return),
        action: KeyAction::Enter,
        kind: KeyKind::Return,
        pill: true,
    }
}

fn backspace(width_units: f64) -> KeySpec {
    util_glyph(KeyGlyph::Backspace, KeyAction::Backspace, width_units)
}

// ---------------------------------------------------------------------------
// Layer definitions
// ---------------------------------------------------------------------------

fn letters_layer(shifted: bool) -> Layout {
    let case = |lower: char, upper: char| if shifted { upper } else { lower };

    let row_keys = |letters: &[(char, char)]| -> Vec<KeySpec> {
        letters.iter().map(|(lo, up)| key(case(*lo, *up))).collect()
    };

    let mut rows: Vec<Vec<KeySpec>> = Vec::with_capacity(5);

    // Gboard shows a dedicated number row above the letters.
    rows.push("1234567890".chars().map(key).collect());

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

    // Home row: 9 letters centered with half-key spacers so the keys
    // stay the same width as row 1 (Gboard behavior) instead of
    // stretching to fill.
    let mut row_home: Vec<KeySpec> = Vec::with_capacity(11);
    row_home.push(spacer(0.5));
    row_home.extend(row_keys(&[
        ('a', 'A'),
        ('s', 'S'),
        ('d', 'D'),
        ('f', 'F'),
        ('g', 'G'),
        ('h', 'H'),
        ('j', 'J'),
        ('k', 'K'),
        ('l', 'L'),
    ]));
    row_home.push(spacer(0.5));
    rows.push(row_home);

    let mut row_shift: Vec<KeySpec> = Vec::with_capacity(9);
    row_shift.push(util_glyph(
        KeyGlyph::Shift,
        KeyAction::Switch(if shifted {
            Layer::Letters
        } else {
            Layer::Shifted
        }),
        1.5,
    ));
    row_shift.extend(row_keys(&[
        ('z', 'Z'),
        ('x', 'X'),
        ('c', 'C'),
        ('v', 'V'),
        ('b', 'B'),
        ('n', 'N'),
        ('m', 'M'),
    ]));
    row_shift.push(backspace(1.5));
    rows.push(row_shift);

    // Bottom row: ?123 · , · dismiss · space · . · enter. Gboard puts
    // its emoji key where we place dismiss — we have no emoji layer,
    // and the panel needs an explicit close affordance (no system nav
    // bar underneath us).
    rows.push(vec![
        mode_pill("?123", Layer::Symbols),
        util_char(',', 1.0),
        KeySpec {
            cap: KeyCap::Glyph(KeyGlyph::DismissDown),
            action: KeyAction::Dismiss,
            ..key(' ')
        },
        space_bar(4.0),
        util_char('.', 1.0),
        enter_key(),
    ]);

    Layout { rows }
}

/// Gboard's `?123` page.
fn symbols_layer() -> Layout {
    let mut rows: Vec<Vec<KeySpec>> = Vec::with_capacity(4);

    rows.push("1234567890".chars().map(key).collect());
    rows.push("@#$_&-+()/".chars().map(key).collect());

    let mut row3: Vec<KeySpec> = Vec::with_capacity(9);
    row3.push(util_text(
        "=\\<",
        KeyAction::Switch(Layer::SymbolsShift),
        1.5,
    ));
    row3.extend("*\"':;!?".chars().map(key));
    row3.push(backspace(1.5));
    rows.push(row3);

    rows.push(vec![
        mode_pill("ABC", Layer::Letters),
        util_char(',', 1.0),
        KeySpec {
            cap: KeyCap::Text("1234".to_string()),
            action: KeyAction::Switch(Layer::NumPad),
            ..key(' ')
        },
        space_bar(4.0),
        util_char('.', 1.0),
        enter_key(),
    ]);

    Layout { rows }
}

/// Gboard's `=\<` page.
fn symbols_shift_layer() -> Layout {
    let mut rows: Vec<Vec<KeySpec>> = Vec::with_capacity(4);

    rows.push("~`|•√π÷×§Δ".chars().map(key).collect());
    rows.push("£¢€¥^°={}\\".chars().map(key).collect());

    let mut row3: Vec<KeySpec> = Vec::with_capacity(9);
    row3.push(util_text("?123", KeyAction::Switch(Layer::Symbols), 1.5));
    row3.extend("%©®™✓[]".chars().map(key));
    row3.push(backspace(1.5));
    rows.push(row3);

    rows.push(vec![
        mode_pill("ABC", Layer::Letters),
        util_char('<', 1.0),
        KeySpec {
            cap: KeyCap::Text("1234".to_string()),
            action: KeyAction::Switch(Layer::NumPad),
            ..key(' ')
        },
        space_bar(4.0),
        util_char('>', 1.0),
        enter_key(),
    ]);

    Layout { rows }
}

/// Phone-style number pad: operator strip on the left, digit grid in
/// the middle, `%` / space / backspace column on the right. All rows
/// sum to 9 units so the columns align.
///
/// Deviation from Gboard: its operator strip squeezes `+ - * /` into
/// three rows' height; ours is one operator per row, so `/` is dropped
/// (still reachable on the `?123` page).
fn numpad_layer() -> Layout {
    let digit = |c: char| wide_key(c, 2.0);

    let mut rows: Vec<Vec<KeySpec>> = Vec::with_capacity(4);
    rows.push(vec![
        util_char('+', 1.5),
        digit('1'),
        digit('2'),
        digit('3'),
        util_char('%', 1.5),
    ]);
    rows.push(vec![
        util_char('-', 1.5),
        digit('4'),
        digit('5'),
        digit('6'),
        KeySpec {
            width_units: 1.5,
            cap: KeyCap::Glyph(KeyGlyph::Space),
            action: KeyAction::Space,
            kind: KeyKind::Utility,
            pill: false,
        },
    ]);
    rows.push(vec![
        util_char('*', 1.5),
        digit('7'),
        digit('8'),
        digit('9'),
        backspace(1.5),
    ]);
    rows.push(vec![
        mode_pill("ABC", Layer::Letters),
        util_char(',', 1.0),
        KeySpec {
            cap: KeyCap::Text("!?#".to_string()),
            action: KeyAction::Switch(Layer::Symbols),
            ..key(' ')
        },
        wide_key('0', 2.0),
        key('='),
        util_char('.', 1.0),
        enter_key(),
    ]);

    Layout { rows }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_layer_has_five_rows_with_number_row() {
        let l = Layout::for_layer(Layer::Letters);
        assert_eq!(l.rows.len(), 5, "number row + 3 letter rows + action row");
        let digits: Vec<char> = l.rows[0]
            .iter()
            .filter_map(|k| match k.action {
                KeyAction::Char(c) => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(digits, "1234567890".chars().collect::<Vec<_>>());
    }

    #[test]
    fn home_row_is_centered_with_spacers() {
        let l = Layout::for_layer(Layer::Letters);
        let home = &l.rows[2];
        assert_eq!(home.first().unwrap().kind, KeyKind::Spacer);
        assert_eq!(home.last().unwrap().kind, KeyKind::Spacer);
        // Spacers + 9 letters keep the row at the same 10-unit width as
        // the rows above, so letter keys don't stretch.
        let total: f64 = home.iter().map(|k| k.width_units).sum();
        assert!((total - 10.0).abs() < 1e-9);
    }

    #[test]
    fn shift_key_switches_layer() {
        let l = Layout::for_layer(Layer::Letters);
        let shift = &l.rows[3][0];
        match shift.action {
            KeyAction::Switch(Layer::Shifted) => {}
            other => panic!("expected Switch(Shifted) on shift row, got {other:?}"),
        }
    }

    #[test]
    fn shifted_layer_emits_uppercase_chars() {
        let l = Layout::for_layer(Layer::Shifted);
        // Second row (after the number row), first key: should be 'Q'.
        let q = &l.rows[1][0];
        match q.action {
            KeyAction::Char('Q') => {}
            other => panic!("expected Char('Q'), got {other:?}"),
        }
    }

    #[test]
    fn symbols_layer_matches_gboard_page() {
        let l = Layout::for_layer(Layer::Symbols);
        let row2: Vec<char> = l.rows[1]
            .iter()
            .filter_map(|k| match k.action {
                KeyAction::Char(c) => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(row2, "@#$_&-+()/".chars().collect::<Vec<_>>());
        // First key of row 3 pages to the extended symbols layer.
        match l.rows[2][0].action {
            KeyAction::Switch(Layer::SymbolsShift) => {}
            other => panic!("expected Switch(SymbolsShift), got {other:?}"),
        }
    }

    #[test]
    fn symbols_shift_layer_pages_back_to_symbols() {
        let l = Layout::for_layer(Layer::SymbolsShift);
        match l.rows[2][0].action {
            KeyAction::Switch(Layer::Symbols) => {}
            other => panic!("expected Switch(Symbols), got {other:?}"),
        }
    }

    #[test]
    fn numpad_has_digit_grid_and_operators() {
        let l = Layout::for_layer(Layer::NumPad);
        let chars_of = |row: &Vec<KeySpec>| -> Vec<char> {
            row.iter()
                .filter_map(|k| match k.action {
                    KeyAction::Char(c) => Some(c),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(chars_of(&l.rows[0]), vec!['+', '1', '2', '3', '%']);
        assert_eq!(chars_of(&l.rows[2]), vec!['*', '7', '8', '9']);
        assert!(chars_of(&l.rows[3]).contains(&'0'));
        // Every row spans the same 9 units so the columns align.
        for row in &l.rows {
            let total: f64 = row.iter().map(|k| k.width_units).sum();
            assert!((total - 9.0).abs() < 1e-9, "row width {total} != 9");
        }
    }

    #[test]
    fn bottom_row_mode_keys_page_between_layers() {
        let letters = Layout::for_layer(Layer::Letters);
        match letters.rows[4][0].action {
            KeyAction::Switch(Layer::Symbols) => {}
            other => panic!("?123 should open Symbols, got {other:?}"),
        }
        let symbols = Layout::for_layer(Layer::Symbols);
        match symbols.rows[3][0].action {
            KeyAction::Switch(Layer::Letters) => {}
            other => panic!("ABC should open Letters, got {other:?}"),
        }
    }
}
