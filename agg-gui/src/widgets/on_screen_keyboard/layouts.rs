//! Keyboard layouts — declarative tables of rows / keys per layer.
//!
//! The layer set mirrors Google's Gboard (see `docs/Android Keyboard/`
//! screenshots): a letters page with a dedicated number row, a `?123`
//! symbols page, a `=\<` extended-symbols page, and a phone-style
//! number pad for numeric fields. Adding a new layer (e.g. a French
//! AZERTY, an emoji picker) is a data change here: define a new
//! [`Layer`] variant and return a `Layout` from [`Layout::for_layer`].
//! The paint pass (`layout_paint.rs`) and hit-tester don't change.
//!
//! Each input profile picks its family of pages ([`LayoutFamily`]): Android
//! and every other profile get these Gboard pages; an iPhone
//! ([`InputProfile::MobileIOS`]) gets the iOS keyboard from
//! `layouts_ios.rs` (no number row, `123` / `#+=` pages).

use super::key::{KeyAction, KeyCap, KeyGlyph};
use super::style::Style;
use crate::input_profile::InputProfile;

/// Which platform's keyboard a [`Layout`] draws: its pages, the Enter key's
/// label, the key preview and the long-press popup's look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutFamily {
    /// Google's Gboard (Android, and the default for every other profile):
    /// a number row above the letters, Enter drawn as an icon, the hide
    /// chevron below the keys.
    Gboard,
    /// The iPhone keyboard: no number row, `123` / `#+=` pages, Enter
    /// labelled with a word, the emoji key below the keys and no hide key.
    Ios,
}

impl LayoutFamily {
    /// The keyboard family an input profile's users expect.
    pub fn for_profile(profile: InputProfile) -> Self {
        match profile {
            InputProfile::MobileIOS => LayoutFamily::Ios,
            InputProfile::MobileAndroid | InputProfile::MobileOther | InputProfile::Desktop => {
                LayoutFamily::Gboard
            }
        }
    }

    /// Whether the letters page has its own row of digits (Gboard). Without
    /// one, holding a top-row letter offers its digit (see `alternates.rs`).
    pub fn has_number_row(self) -> bool {
        self == LayoutFamily::Gboard
    }
}

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
    pub(super) rows: Vec<Vec<KeySpec>>,
    /// The platform this layout draws.
    pub(super) family: LayoutFamily,
}

impl Layout {
    /// The layout for `layer` on the keyboard `profile`'s users expect:
    /// the iOS pages on an iPhone, the Gboard pages everywhere else.
    pub fn for_profile(layer: Layer, profile: InputProfile) -> Self {
        match LayoutFamily::for_profile(profile) {
            LayoutFamily::Gboard => Self::for_layer(layer),
            LayoutFamily::Ios => super::layouts_ios::ios_layer(layer),
        }
    }

    /// The Gboard layout for `layer` (see [`Layout::for_profile`]).
    pub fn for_layer(layer: Layer) -> Self {
        match layer {
            Layer::Letters => letters_layer(false),
            Layer::Shifted => letters_layer(true),
            Layer::Symbols => symbols_layer(),
            Layer::SymbolsShift => symbols_shift_layer(),
            Layer::NumPad => numpad_layer(),
        }
    }

    /// Height of the toolbar row above the keys: Gboard's, which holds the
    /// microphone key, while a voice provider is registered and the field
    /// isn't a password field; `0.0` otherwise, and on the iPhone, whose
    /// microphone sits below the keys.
    pub(super) fn toolbar_height(&self, style: &Style) -> f64 {
        if self.family == LayoutFamily::Gboard && super::voice_session::key_shown() {
            style.toolbar_height
        } else {
            0.0
        }
    }

    /// Compute the panel height required to render this layout at the
    /// given viewport width, accounting for vertical padding, row gaps, the
    /// strip below the keys that holds the hide-keyboard chevron, and the
    /// toolbar row above them while there is one (Gboard's, holding the
    /// microphone key while a voice provider is registered).
    pub fn compute_panel_height(&self, _viewport_width: f64, style: &Style) -> f64 {
        let rows = self.rows.len() as f64;
        self.toolbar_height(style)
            + style.panel_padding_top
            + style.panel_padding_bottom
            + style.bottom_strip_height
            + rows * style.row_height
            + (rows - 1.0).max(0.0) * style.key_v_gap
    }
}

// ---------------------------------------------------------------------------
// Key constructors
// ---------------------------------------------------------------------------

pub(super) fn key(c: char) -> KeySpec {
    KeySpec {
        width_units: 1.0,
        cap: KeyCap::Text(c.to_string()),
        action: KeyAction::Char(c),
        kind: KeyKind::Letter,
        pill: false,
    }
}

pub(super) fn wide_key(c: char, width_units: f64) -> KeySpec {
    KeySpec {
        width_units,
        ..key(c)
    }
}

/// A character key with the light-blue utility face (Gboard paints the
/// `,` / `.` keys and the numpad operator strip this way).
pub(super) fn util_char(c: char, width_units: f64) -> KeySpec {
    KeySpec {
        width_units,
        kind: KeyKind::Utility,
        ..key(c)
    }
}

pub(super) fn util_text(label: &str, action: KeyAction, width_units: f64) -> KeySpec {
    KeySpec {
        width_units,
        cap: KeyCap::Text(label.to_string()),
        action,
        kind: KeyKind::Utility,
        pill: false,
    }
}

pub(super) fn util_glyph(glyph: KeyGlyph, action: KeyAction, width_units: f64) -> KeySpec {
    KeySpec {
        width_units,
        cap: KeyCap::Glyph(glyph),
        action,
        kind: KeyKind::Utility,
        pill: false,
    }
}

pub(super) fn spacer(width_units: f64) -> KeySpec {
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

pub(super) fn space_bar(width_units: f64) -> KeySpec {
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

pub(super) fn backspace(width_units: f64) -> KeySpec {
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

    // Bottom row: ?123 · , · space · . · enter. Gboard puts its emoji key
    // between `,` and space; with no emoji page yet, the spacebar takes
    // that room (Gboard's own layout with its emoji key turned off). The
    // hide-keyboard chevron sits below the keys, as on the phone.
    rows.push(vec![
        mode_pill("?123", Layer::Symbols),
        util_char(',', 1.0),
        space_bar(5.0),
        util_char('.', 1.0),
        enter_key(),
    ]);

    Layout {
        rows,
        family: LayoutFamily::Gboard,
    }
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

    Layout {
        rows,
        family: LayoutFamily::Gboard,
    }
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

    Layout {
        rows,
        family: LayoutFamily::Gboard,
    }
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

    let rows: Vec<Vec<KeySpec>> = vec![
        vec![
            util_char('+', 1.5),
            digit('1'),
            digit('2'),
            digit('3'),
            util_char('%', 1.5),
        ],
        vec![
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
        ],
        vec![
            util_char('*', 1.5),
            digit('7'),
            digit('8'),
            digit('9'),
            backspace(1.5),
        ],
        vec![
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
        ],
    ];

    Layout {
        rows,
        family: LayoutFamily::Gboard,
    }
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
