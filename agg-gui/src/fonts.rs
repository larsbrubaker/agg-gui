//! Font faces bundled with agg-gui, so applications don't each copy them.
//!
//! agg-gui itself never loads a font implicitly — apps pass a [`Font`] to the
//! widgets (and to [`crate::font_settings::set_system_font`]).  This module
//! ships the faces the library's own UI assumes, as `&'static [u8]` plus their
//! license texts (all SIL Open Font License 1.1, files in `assets/fonts/`):
//!
//! * [`NOTO_SANS_REGULAR`] — the default UI text face.
//! * [`FONT_AWESOME_4_7`] — Font Awesome 4.7.0, the icon face the widgets'
//!   `'\u{F0xx}'` icon code points (menus, toolbars, …) refer to.
//! * [`SHORTCUT_SYMBOLS`] — a ~3 KB subset of Noto Sans Symbols 2 (v2.008)
//!   holding the macOS shortcut glyphs `⌘ ⇧ ⌥ ⌫ ⌦ ⏎` that menu labels use
//!   (see `widgets/menu/shortcut_text.rs`).  Noto Sans Symbols 2 has no
//!   `⌃ ↩ ⎋ ⇥`, arrows, `↖ ↘ ⇞ ⇟`; shortcut labels fall back per key for those.
//!
//! [`with_standard_fallbacks`] chains icon + symbol faces behind any primary
//! face; [`standard_ui_font`] is that chain on Noto Sans.  Statics are only
//! linked into binaries that reference them, so apps that load fonts
//! themselves (e.g. lazily on the web) don't pay for faces they don't use.

use std::sync::Arc;

use crate::text::Font;

/// Noto Sans Regular — the default UI text face.
pub static NOTO_SANS_REGULAR: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
/// License (SIL OFL 1.1) for [`NOTO_SANS_REGULAR`].
pub static NOTO_SANS_LICENSE: &str = include_str!("../assets/fonts/Noto-LICENSE-OFL.txt");

/// Font Awesome 4.7.0 icon face.
pub static FONT_AWESOME_4_7: &[u8] = include_bytes!("../assets/fonts/FontAwesome-4.7.0.ttf");
/// License (SIL OFL 1.1) for [`FONT_AWESOME_4_7`].
pub static FONT_AWESOME_LICENSE: &str = include_str!("../assets/fonts/FontAwesome-LICENSE-OFL.txt");

/// Subset of Noto Sans Symbols 2 with the macOS shortcut glyphs
/// (`⌘ ⇧ ⌥ ⌫ ⌦ ⏎`).
pub static SHORTCUT_SYMBOLS: &[u8] =
    include_bytes!("../assets/fonts/NotoSansSymbols2-ShortcutSubset.ttf");
/// License (SIL OFL 1.1) for [`SHORTCUT_SYMBOLS`].
pub static SHORTCUT_SYMBOLS_LICENSE: &str =
    include_str!("../assets/fonts/NotoSansSymbols2-LICENSE-OFL.txt");

/// The shortcut-symbol face, parsed.  Append it at the end of an app's own
/// fallback chain when the chain is built by hand.
pub fn shortcut_symbols_font() -> Font {
    Font::from_slice(SHORTCUT_SYMBOLS).expect("bundled shortcut symbol font is valid")
}

/// Chain the standard fallbacks behind `primary`:
/// `primary → Font Awesome 4.7 → shortcut symbols`.
///
/// `Font::with_fallback` replaces any fallback `primary` already had, so pass
/// a face without one (or build the chain by hand with
/// [`shortcut_symbols_font`] last).
pub fn with_standard_fallbacks(primary: Font) -> Font {
    let icons = Font::from_slice(FONT_AWESOME_4_7)
        .expect("bundled Font Awesome is valid")
        .with_fallback(Arc::new(shortcut_symbols_font()));
    primary.with_fallback(Arc::new(icons))
}

/// Noto Sans with the standard fallback chain — a ready-to-use UI font.
pub fn standard_ui_font() -> Font {
    with_standard_fallbacks(
        Font::from_slice(NOTO_SANS_REGULAR).expect("bundled Noto Sans is valid"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;
    use crate::widgets::menu::{MenuShortcut, ShortcutKey};

    #[test]
    fn standard_chain_draws_text_icons_and_shortcut_modifiers() {
        let font = standard_ui_font();
        assert!(font.has_glyph('A'));
        assert!(font.has_glyph('\u{F0C4}'), "FA scissors via fallback");
        for ch in [
            '\u{2318}', '\u{21E7}', '\u{2325}', '\u{232B}', '\u{2326}', '\u{23CE}',
        ] {
            assert!(font.has_glyph(ch), "{ch} via the symbol subset");
        }
        assert!(NOTO_SANS_LICENSE.contains("Open Font License"));
        assert!(FONT_AWESOME_LICENSE.contains("Open Font License"));
        assert!(SHORTCUT_SYMBOLS_LICENSE.contains("Open Font License"));
    }

    /// With the bundled chain every Mac shortcut label uses the modifier
    /// glyphs — never the spelled-out `Cmd+` fallback — for every key and
    /// modifier combination, and every character in it is drawable.
    #[test]
    fn every_mac_label_renders_as_glyphs_with_bundled_chain() {
        let font = standard_ui_font();
        let keys = [
            ShortcutKey::Char('X'),
            ShortcutKey::Char('Z'),
            ShortcutKey::Char('1'),
            ShortcutKey::Insert,
            ShortcutKey::Delete,
            ShortcutKey::Backspace,
            ShortcutKey::Enter,
            ShortcutKey::Escape,
            ShortcutKey::Tab,
            ShortcutKey::Space,
            ShortcutKey::ArrowLeft,
            ShortcutKey::ArrowRight,
            ShortcutKey::ArrowUp,
            ShortcutKey::ArrowDown,
            ShortcutKey::Home,
            ShortcutKey::End,
            ShortcutKey::PageUp,
            ShortcutKey::PageDown,
        ];
        for key in keys {
            for bits in 0..8u8 {
                let sc = MenuShortcut {
                    key,
                    command: bits & 1 != 0,
                    shift: bits & 2 != 0,
                    alt: bits & 4 != 0,
                };
                let label = sc.display_text_for_font(Platform::MacOS, &font);
                assert!(!label.contains("Cmd") && !label.contains('+'), "{label}");
                assert!(label.chars().all(|c| font.has_glyph(c)), "tofu in {label}");
            }
        }
        let p = |s: &str| {
            MenuShortcut::parse(s)
                .unwrap()
                .display_text_for_font(Platform::MacOS, &font)
        };
        assert_eq!(p("Ctrl+X"), "\u{2318}X");
        assert_eq!(p("Ctrl+Shift+Z"), "\u{21E7}\u{2318}Z");
        assert_eq!(p("Alt+Ctrl+Backspace"), "\u{2325}\u{2318}\u{232B}");
        assert_eq!(p("Delete"), "\u{2326}");
        // ↩ isn't in the bundled faces; Enter uses the ⏎ alternative.
        assert_eq!(p("Ctrl+Enter"), "\u{2318}\u{23CE}");
        // No bundled ⎋ / ⇥ / arrows: the key is named, modifiers stay glyphs.
        assert_eq!(p("Ctrl+Esc"), "\u{2318}Esc");
        assert_eq!(p("Shift+Tab"), "\u{21E7}Tab");
    }
}
