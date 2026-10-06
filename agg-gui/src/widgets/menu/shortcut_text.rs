//! Platform-specific display text for menu keyboard shortcuts.
//!
//! [`MenuShortcut`](super::model::MenuShortcut) (in `model.rs`) is the parsed,
//! portable form of a shortcut such as `"Ctrl+Shift+Z"`; this module turns it
//! back into the label a menu row, context menu or tooltip draws:
//!
//! * Windows / Linux / other: `Ctrl+Shift+Z` (unchanged from before).
//! * macOS: Apple's modifier glyphs with no separators, in Apple's order
//!   Control ⌃, Option ⌥, Shift ⇧, Command ⌘, then the key — `⇧⌘Z`, `⌘X`.
//!   Special keys use Apple's symbols (`⌫`, `⌦`, `↩`, `⎋`, `⇥`, arrows).
//!
//! The formatting takes the [`Platform`] explicitly so both renderings are
//! unit-testable on any host; [`MenuShortcut::display_text`] applies the
//! runtime platform (which WASM hosts set from the browser's user agent).
//!
//! The glyphs are only used when the font that will draw them actually has
//! them ([`MenuShortcut::display_text_for_font`]), so nothing renders as tofu:
//! without the modifier glyphs the whole Mac label is spelled out
//! (`Cmd+Shift+Z`); a key symbol the font lacks degrades on its own — to an
//! alternative symbol (`↩` → `⏎`) or the key's name (`⌘Esc`).  The crate's
//! bundled symbol face ([`crate::fonts::SHORTCUT_SYMBOLS`]) supplies the
//! modifier glyphs plus `⌫ ⌦ ⏎`.

use crate::platform::{self, Platform};
use crate::text::Font;

use super::model::{MenuItem, MenuShortcut, ShortcutKey};

/// Apple's modifier glyphs.  `MenuShortcut` has no separate Control flag
/// (its portable `command` is ⌘ on a Mac), so only ⌥ ⇧ ⌘ are emitted — in
/// Apple's canonical order.
const MAC_OPTION: char = '\u{2325}'; // ⌥
const MAC_SHIFT: char = '\u{21E7}'; // ⇧
const MAC_COMMAND: char = '\u{2318}'; // ⌘

impl MenuShortcut {
    /// Display text for the current runtime platform, assuming the drawing
    /// font has the macOS glyphs.  Prefer [`Self::display_text_for_font`]
    /// when the font is known.
    pub fn display_text(self) -> String {
        self.display_text_for(platform::current_platform())
    }

    /// Display text for `platform`: glyph form (`⇧⌘Z`) on macOS, `Ctrl+Shift+Z`
    /// elsewhere.
    pub fn display_text_for(self, platform: Platform) -> String {
        match platform {
            Platform::MacOS => self.mac_glyph_text(),
            Platform::Windows | Platform::Linux | Platform::Other => self.plain_text("Ctrl"),
        }
    }

    /// Like [`Self::display_text_for`], but only uses symbols `font`
    /// (including its fallback chain) can draw.  On macOS: if any modifier
    /// glyph is missing the whole label is spelled out (`Cmd+Shift+Z`);
    /// otherwise the key uses its first drawable symbol, else its name.
    pub fn display_text_for_font(self, platform: Platform, font: &Font) -> String {
        if platform != Platform::MacOS {
            return self.display_text_for(platform);
        }
        let drawable = |s: &str| s.chars().all(|ch| font.has_glyph(ch));
        let mut text = self.mac_modifier_glyphs();
        if !drawable(&text) {
            return self.plain_text("Cmd");
        }
        match self.key.mac_symbols().iter().find(|sym| drawable(sym)) {
            Some(sym) => text.push_str(sym),
            None => text.push_str(&self.key.plain_text()),
        }
        text
    }

    fn mac_glyph_text(self) -> String {
        let mut text = self.mac_modifier_glyphs();
        match self.key.mac_symbols().first() {
            Some(sym) => text.push_str(sym),
            None => text.push_str(&self.key.plain_text()),
        }
        text
    }

    /// Modifier glyphs in Apple's order (⌥ ⇧ ⌘).
    fn mac_modifier_glyphs(self) -> String {
        let mut text = String::new();
        if self.alt {
            text.push(MAC_OPTION);
        }
        if self.shift {
            text.push(MAC_SHIFT);
        }
        if self.command {
            text.push(MAC_COMMAND);
        }
        text
    }

    fn plain_text(self, command_label: &str) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.command {
            parts.push(command_label.to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        parts.push(self.key.plain_text());
        parts.join("+")
    }
}

impl ShortcutKey {
    /// Apple's symbols for a named key, preferred first (the rest are
    /// fallbacks when a font lacks the first).  Empty for characters and for
    /// keys a Mac labels by name (Insert, Space): those use
    /// [`Self::plain_text`].
    fn mac_symbols(self) -> &'static [&'static str] {
        match self {
            Self::Char(_) | Self::Insert | Self::Space => &[],
            Self::Delete => &["\u{2326}"],    // ⌦ forward delete
            Self::Backspace => &["\u{232B}"], // ⌫
            Self::Enter => &["\u{21A9}", "\u{23CE}"], // ↩, else ⏎
            Self::Escape => &["\u{238B}"],    // ⎋
            Self::Tab => &["\u{21E5}"],       // ⇥
            Self::ArrowLeft => &["\u{2190}"],
            Self::ArrowRight => &["\u{2192}"],
            Self::ArrowUp => &["\u{2191}"],
            Self::ArrowDown => &["\u{2193}"],
            Self::Home => &["\u{2196}"],     // ↖
            Self::End => &["\u{2198}"],      // ↘
            Self::PageUp => &["\u{21DE}"],   // ⇞
            Self::PageDown => &["\u{21DF}"], // ⇟
        }
    }

    fn plain_text(self) -> String {
        match self {
            Self::Char(ch) => ch.to_uppercase().collect(),
            Self::Insert => "Insert".into(),
            Self::Delete => "Delete".into(),
            Self::Backspace => "Backspace".into(),
            Self::Enter => "Enter".into(),
            Self::Escape => "Esc".into(),
            Self::Tab => "Tab".into(),
            Self::Space => "Space".into(),
            Self::ArrowLeft => "Left".into(),
            Self::ArrowRight => "Right".into(),
            Self::ArrowUp => "Up".into(),
            Self::ArrowDown => "Down".into(),
            Self::Home => "Home".into(),
            Self::End => "End".into(),
            Self::PageUp => "PageUp".into(),
            Self::PageDown => "PageDown".into(),
        }
    }
}

impl MenuItem {
    /// The shortcut label this row shows on `platform`.  A shortcut that
    /// parsed into an [`accelerator`](MenuItem::accelerator) is re-rendered
    /// for the platform (`"Ctrl+X"` → `⌘X` on a Mac); free-form text that
    /// didn't parse is shown as declared.
    pub fn shortcut_text_for(&self, platform: Platform) -> Option<String> {
        match self.accelerator {
            Some(acc) => Some(acc.display_text_for(platform)),
            None => self.shortcut.clone(),
        }
    }

    /// [`Self::shortcut_text_for`] with the macOS glyph-coverage fallback of
    /// [`MenuShortcut::display_text_for_font`].  This is what menus draw.
    pub fn shortcut_text_for_font(&self, platform: Platform, font: &Font) -> Option<String> {
        match self.accelerator {
            Some(acc) => Some(acc.display_text_for_font(platform, font)),
            None => self.shortcut.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASCADIA: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

    fn sc(text: &str) -> MenuShortcut {
        MenuShortcut::parse(text).expect("shortcut parses")
    }

    #[test]
    fn mac_uses_modifier_glyphs_without_separators() {
        let mac = Platform::MacOS;
        assert_eq!(sc("Ctrl+X").display_text_for(mac), "\u{2318}X"); // ⌘X
        assert_eq!(sc("Ctrl+c").display_text_for(mac), "\u{2318}C");
        // Apple order: ⌥ ⇧ ⌘ regardless of declaration order.
        assert_eq!(
            sc("Ctrl+Shift+Z").display_text_for(mac),
            "\u{21E7}\u{2318}Z"
        );
        assert_eq!(
            sc("Shift+Ctrl+Z").display_text_for(mac),
            "\u{21E7}\u{2318}Z"
        );
        assert_eq!(sc("Alt+N").display_text_for(mac), "\u{2325}N"); // ⌥N
        assert_eq!(
            sc("Ctrl+Alt+Shift+K").display_text_for(mac),
            "\u{2325}\u{21E7}\u{2318}K"
        );
    }

    #[test]
    fn mac_named_keys_use_apple_symbols() {
        let mac = Platform::MacOS;
        assert_eq!(
            sc("Ctrl+Backspace").display_text_for(mac),
            "\u{2318}\u{232B}"
        );
        assert_eq!(sc("Delete").display_text_for(mac), "\u{2326}");
        assert_eq!(sc("Ctrl+Enter").display_text_for(mac), "\u{2318}\u{21A9}");
        assert_eq!(sc("Esc").display_text_for(mac), "\u{238B}");
        assert_eq!(sc("Shift+Tab").display_text_for(mac), "\u{21E7}\u{21E5}");
        assert_eq!(sc("Ctrl+Left").display_text_for(mac), "\u{2318}\u{2190}");
        assert_eq!(sc("Ctrl+Space").display_text_for(mac), "\u{2318}Space");
        assert_eq!(sc("F").display_text_for(mac), "F"); // plain key
    }

    #[test]
    fn other_platforms_keep_ctrl_plus_text() {
        for p in [Platform::Windows, Platform::Linux, Platform::Other] {
            assert_eq!(sc("Ctrl+X").display_text_for(p), "Ctrl+X");
            assert_eq!(sc("Ctrl+Shift+Z").display_text_for(p), "Ctrl+Shift+Z");
            assert_eq!(sc("Alt+N").display_text_for(p), "Alt+N");
            assert_eq!(sc("Ctrl+Backspace").display_text_for(p), "Ctrl+Backspace");
            assert_eq!(sc("Esc").display_text_for(p), "Esc");
            assert_eq!(sc("a").display_text_for(p), "A");
        }
    }

    #[test]
    fn new_named_keys_parse_and_match() {
        use crate::event::{Key, Modifiers};
        assert_eq!(sc("Tab").key, ShortcutKey::Tab);
        assert_eq!(sc("Space").key, ShortcutKey::Space);
        assert_eq!(sc("Up").key, ShortcutKey::ArrowUp);
        assert_eq!(sc("PageDown").key, ShortcutKey::PageDown);
        assert!(sc("Space").matches(&Key::Char(' '), Modifiers::default()));
        assert!(sc("Down").matches(&Key::ArrowDown, Modifiers::default()));
        assert!(!sc("Down").matches(&Key::ArrowUp, Modifiers::default()));
    }

    /// The Mac glyph form is only used when the drawing font can render it.
    /// Cascadia Code (and every font the demo ships) lacks ⌘, so a Mac label
    /// falls back to spelled-out text rather than drawing tofu; a symbol the
    /// font does have (← in Cascadia) is still drawn as the symbol.
    #[test]
    fn mac_glyphs_fall_back_to_text_when_font_lacks_them() {
        let font = Font::from_slice(CASCADIA).expect("font parses");
        assert!(!font.has_glyph(MAC_COMMAND));
        assert!(font.has_glyph('\u{2190}'));
        let mac = Platform::MacOS;
        assert_eq!(sc("Ctrl+X").display_text_for_font(mac, &font), "Cmd+X");
        assert_eq!(
            sc("Ctrl+Shift+Z").display_text_for_font(mac, &font),
            "Cmd+Shift+Z"
        );
        assert_eq!(sc("Left").display_text_for_font(mac, &font), "\u{2190}");
        assert_eq!(
            sc("Ctrl+X").display_text_for_font(Platform::Windows, &font),
            "Ctrl+X"
        );
    }

    #[test]
    fn menu_item_renders_parsed_shortcut_per_platform() {
        let item = MenuItem::action("Redo", "redo").shortcut("Ctrl+Shift+Z");
        assert_eq!(
            item.shortcut_text_for(Platform::MacOS).as_deref(),
            Some("\u{21E7}\u{2318}Z")
        );
        assert_eq!(
            item.shortcut_text_for(Platform::Linux).as_deref(),
            Some("Ctrl+Shift+Z")
        );
        // Free-form text that doesn't parse is shown as declared.
        let custom = MenuItem::action("Go", "go").shortcut("Hold Fn");
        assert_eq!(
            custom.shortcut_text_for(Platform::MacOS).as_deref(),
            Some("Hold Fn")
        );
    }
}
