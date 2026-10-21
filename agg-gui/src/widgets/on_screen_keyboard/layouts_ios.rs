//! The iPhone keyboard's pages, for [`InputProfile::MobileIOS`]: what an
//! iPhone user's thumbs expect instead of Gboard's layout.
//!
//! iOS has no number row: the letters page is three rows of letters over
//! `123 · space · return`, with Shift and Delete set in from the letters by
//! a quarter key; `123` opens the digits-and-punctuation page, whose `#+=`
//! key opens the second symbols page. The emoji key sits below the keys
//! (in the strip the iPhone keeps for emoji and dictation, painted by
//! `layout_paint.rs`). Every page has four rows, so the keyboard keeps one
//! height on every page, as on the phone. A numeric field opens on the
//! `123` page (what iOS shows for numbers and punctuation).
//!
//! The key constructors and [`Layout`] come from [`super::layouts`]; one
//! width unit is one key pitch there too.
//!
//! [`InputProfile::MobileIOS`]: crate::input_profile::InputProfile::MobileIOS

use super::key::{KeyAction, KeyCap, KeyGlyph};
use super::layouts::{
    backspace, key, space_bar, spacer, util_glyph, util_text, KeyKind, KeySpec, Layer, Layout,
    LayoutFamily,
};

/// iOS's Shift and Delete: a little wider than a letter, set in from the
/// letters beside them by a quarter key.
const SIDE_KEY: f64 = 1.25;
/// The gap between Shift / Delete and the letters, beyond the usual one.
const SIDE_INSET: f64 = 0.25;
/// The bottom row's mode key and return key (the spacebar takes the rest).
const BOTTOM_SIDE: f64 = 2.5;

/// The iOS layout for `layer`.
pub(super) fn ios_layer(layer: Layer) -> Layout {
    let rows = match layer {
        Layer::Letters => letters(false),
        Layer::Shifted => letters(true),
        // iOS's numbers-and-punctuation keyboard is its `123` page.
        Layer::Symbols | Layer::NumPad => symbols(),
        Layer::SymbolsShift => symbols_more(),
    };
    Layout {
        rows,
        family: LayoutFamily::Ios,
    }
}

fn letters(shifted: bool) -> Vec<Vec<KeySpec>> {
    let row = |s: &str| -> Vec<KeySpec> {
        s.chars()
            .map(|c| key(if shifted { c.to_ascii_uppercase() } else { c }))
            .collect()
    };
    let mut home = vec![spacer(0.5)];
    home.extend(row("asdfghjkl"));
    home.push(spacer(0.5));

    let shift_target = if shifted {
        Layer::Letters
    } else {
        Layer::Shifted
    };
    let mut third = vec![
        util_glyph(KeyGlyph::Shift, KeyAction::Switch(shift_target), SIDE_KEY),
        spacer(SIDE_INSET),
    ];
    third.extend(row("zxcvbnm"));
    third.extend([spacer(SIDE_INSET), backspace(SIDE_KEY)]);

    vec![
        row("qwertyuiop"),
        home,
        third,
        bottom_row("123", Layer::Symbols),
    ]
}

/// The `123` page: digits, then iOS's common punctuation.
fn symbols() -> Vec<Vec<KeySpec>> {
    vec![
        "1234567890".chars().map(key).collect(),
        "-/:;()$&@\"".chars().map(key).collect(),
        punctuation_row("#+=", Layer::SymbolsShift),
        bottom_row("ABC", Layer::Letters),
    ]
}

/// The `#+=` page: brackets, maths and currency.
fn symbols_more() -> Vec<Vec<KeySpec>> {
    vec![
        "[]{}#%^*+=".chars().map(key).collect(),
        "_\\|~<>€£¥•".chars().map(key).collect(),
        punctuation_row("123", Layer::Symbols),
        bottom_row("ABC", Layer::Letters),
    ]
}

/// The symbol pages' third row: a page key, five wide punctuation keys and
/// Delete, as iOS lays it out.
fn punctuation_row(page_label: &str, page: Layer) -> Vec<KeySpec> {
    let mut row = vec![
        util_text(page_label, KeyAction::Switch(page), SIDE_KEY),
        spacer(SIDE_INSET),
    ];
    // Five keys share the room seven letters take on the letters page.
    let width = (10.0 - 2.0 * (SIDE_KEY + SIDE_INSET)) / 5.0;
    row.extend(".,?!'".chars().map(|c| KeySpec {
        width_units: width,
        ..key(c)
    }));
    row.extend([spacer(SIDE_INSET), backspace(SIDE_KEY)]);
    row
}

/// `123` (or `ABC`) · space · return. The Enter key's label is set by the
/// paint pass from the field's Enter action.
fn bottom_row(mode_label: &str, mode: Layer) -> Vec<KeySpec> {
    vec![
        util_text(mode_label, KeyAction::Switch(mode), BOTTOM_SIDE),
        KeySpec {
            cap: KeyCap::Text("space".to_string()),
            ..space_bar(10.0 - 2.0 * BOTTOM_SIDE)
        },
        KeySpec {
            width_units: BOTTOM_SIDE,
            cap: KeyCap::Text("return".to_string()),
            action: KeyAction::Enter,
            kind: KeyKind::Return,
            pill: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_profile::InputProfile;

    fn chars(row: &[KeySpec]) -> String {
        row.iter()
            .filter_map(|k| match k.action {
                KeyAction::Char(c) => Some(c),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_iphone_letters_page_has_no_number_row() {
        let l = Layout::for_profile(Layer::Letters, InputProfile::MobileIOS);
        assert_eq!(l.family, LayoutFamily::Ios);
        assert_eq!(l.rows.len(), 4, "three letter rows and the bottom row");
        assert_eq!(chars(&l.rows[0]), "qwertyuiop");
        assert_eq!(chars(&l.rows[1]), "asdfghjkl");
        assert_eq!(chars(&l.rows[2]), "zxcvbnm");
    }

    #[test]
    fn every_iphone_row_spans_ten_keys_and_every_page_has_four_rows() {
        for layer in [
            Layer::Letters,
            Layer::Shifted,
            Layer::Symbols,
            Layer::SymbolsShift,
            Layer::NumPad,
        ] {
            let l = Layout::for_profile(layer, InputProfile::MobileIOS);
            assert_eq!(l.rows.len(), 4, "{layer:?}");
            for row in &l.rows {
                let total: f64 = row.iter().map(|k| k.width_units).sum();
                assert!((total - 10.0).abs() < 1e-9, "{layer:?}: row width {total}");
            }
        }
    }

    #[test]
    fn the_iphone_bottom_row_is_123_space_return() {
        let l = Layout::for_profile(Layer::Letters, InputProfile::MobileIOS);
        let caps: Vec<String> = l.rows[3]
            .iter()
            .map(|k| match &k.cap {
                KeyCap::Text(t) => t.clone(),
                KeyCap::Glyph(g) => format!("{g:?}"),
            })
            .collect();
        assert_eq!(caps, ["123", "space", "return"]);
        assert!(matches!(
            l.rows[3][0].action,
            KeyAction::Switch(Layer::Symbols)
        ));
        let symbols = Layout::for_profile(Layer::Symbols, InputProfile::MobileIOS);
        assert_eq!(chars(&symbols.rows[1]), "-/:;()$&@\"");
        assert!(matches!(
            symbols.rows[2][0].action,
            KeyAction::Switch(Layer::SymbolsShift)
        ));
    }

    #[test]
    fn android_and_other_phones_keep_the_gboard_pages() {
        for profile in [InputProfile::MobileAndroid, InputProfile::MobileOther] {
            let l = Layout::for_profile(Layer::Letters, profile);
            assert_eq!(l.family, LayoutFamily::Gboard);
            assert_eq!(chars(&l.rows[0]), "1234567890", "{profile:?}");
        }
    }
}
