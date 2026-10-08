//! From C# [`Keys`] strokes to the key events agg-gui's `App` takes.
//!
//! agg-gui has no separate KeyPress: a stroke that types a character is
//! `Key::Char(c)`, and named keys (`{Enter}`, `{Left}`) are their own `Key`
//! variants, spelled the way `agg_gui::winit_adapter` reports them so a
//! simulated stroke reaches widgets exactly as a real one does.  The C#
//! Control bit is the platform's *command* modifier — Cmd on macOS, Ctrl
//! elsewhere — so `"^a"` selects all on every OS (a deliberate change from
//! C#, which sent a literal Control on the Mac).

use agg_gui::platform::{current_platform, Platform};
use agg_gui::{Key, Modifiers};

use crate::keys::Keys;
use crate::typed_key_parser::TypedKey;

impl Keys {
    /// The agg-gui key for this value's key code (modifier bits ignored).
    ///
    /// Letters become their lower-case `Key::Char` (a real keyboard reports
    /// the unshifted letter with a chord), digits and the OEM punctuation
    /// keys become the character on the key, and keys agg-gui has no variant
    /// for become `Key::Other` with the name winit/the browser use
    /// (`"F4"`, `"Shift"`), falling back to the C# member name.
    pub fn to_agg_key(self) -> Key {
        let code = self.key_code();
        let c = code.0;
        let named = match code {
            Keys::BACK => Some(Key::Backspace),
            Keys::DELETE => Some(Key::Delete),
            Keys::INSERT => Some(Key::Insert),
            Keys::LEFT => Some(Key::ArrowLeft),
            Keys::RIGHT => Some(Key::ArrowRight),
            Keys::UP => Some(Key::ArrowUp),
            Keys::DOWN => Some(Key::ArrowDown),
            Keys::HOME => Some(Key::Home),
            Keys::END => Some(Key::End),
            Keys::PAGE_UP => Some(Key::PageUp),
            Keys::PAGE_DOWN => Some(Key::PageDown),
            Keys::TAB => Some(Key::Tab),
            Keys::ENTER => Some(Key::Enter),
            Keys::ESCAPE => Some(Key::Escape),
            Keys::SPACE => Some(Key::Char(' ')),
            Keys::MULTIPLY => Some(Key::Char('*')),
            Keys::ADD => Some(Key::Char('+')),
            Keys::SUBTRACT => Some(Key::Char('-')),
            Keys::DECIMAL => Some(Key::Char('.')),
            Keys::DIVIDE => Some(Key::Char('/')),
            Keys::OEM_SEMICOLON => Some(Key::Char(';')),
            Keys::OEMPLUS => Some(Key::Char('=')),
            Keys::OEMCOMMA => Some(Key::Char(',')),
            Keys::OEM_MINUS => Some(Key::Char('-')),
            Keys::OEM_PERIOD => Some(Key::Char('.')),
            Keys::OEM_QUESTION => Some(Key::Char('/')),
            Keys::OEMTILDE => Some(Key::Char('`')),
            Keys::OEM_OPEN_BRACKETS => Some(Key::Char('[')),
            Keys::OEM_PIPE => Some(Key::Char('\\')),
            Keys::OEM_CLOSE_BRACKETS => Some(Key::Char(']')),
            Keys::OEM_QUOTES => Some(Key::Char('\'')),
            Keys::SHIFT_KEY | Keys::L_SHIFT_KEY | Keys::R_SHIFT_KEY => {
                Some(Key::Other("Shift".into()))
            }
            Keys::CONTROL_KEY | Keys::L_CONTROL_KEY | Keys::R_CONTROL_KEY => {
                Some(Key::Other("Control".into()))
            }
            Keys::MENU | Keys::L_MENU | Keys::R_MENU => Some(Key::Other("Alt".into())),
            Keys::L_WIN | Keys::R_WIN => Some(Key::Other("Super".into())),
            Keys::CAPS_LOCK => Some(Key::Other("CapsLock".into())),
            Keys::NUM_LOCK => Some(Key::Other("NumLock".into())),
            Keys::SCROLL => Some(Key::Other("ScrollLock".into())),
            Keys::APPS => Some(Key::Other("ContextMenu".into())),
            _ => None,
        };
        if let Some(key) = named {
            return key;
        }
        if (Keys::A.0..=Keys::Z.0).contains(&c) {
            return Key::Char(char::from(b'a' + (c - Keys::A.0) as u8));
        }
        if (Keys::D0.0..=Keys::D9.0).contains(&c) {
            return Key::Char(char::from(b'0' + (c - Keys::D0.0) as u8));
        }
        if (Keys::NUM_PAD0.0..=Keys::NUM_PAD9.0).contains(&c) {
            return Key::Char(char::from(b'0' + (c - Keys::NUM_PAD0.0) as u8));
        }
        if (Keys::F1.0..=Keys::F24.0).contains(&c) {
            return Key::Other(format!("F{}", c - Keys::F1.0 + 1));
        }
        Key::Other(code.to_string())
    }

    /// The agg-gui modifiers for this value's modifier bits: Control is the
    /// platform command modifier (meta on macOS, ctrl elsewhere).
    pub fn to_agg_modifiers(self) -> Modifiers {
        let command = self.contains(Keys::CONTROL);
        let on_mac = current_platform() == Platform::MacOS;
        Modifiers {
            shift: self.contains(Keys::SHIFT),
            ctrl: command && !on_mac,
            alt: self.contains(Keys::ALT),
            meta: command && on_mac,
        }
    }
}

impl TypedKey {
    /// The agg-gui key this stroke presses: the typed character when it has
    /// one, otherwise the key code's key.
    pub fn agg_key(&self) -> Key {
        if self.has_character() {
            Key::Char(self.character())
        } else {
            self.key().to_agg_key()
        }
    }

    /// The agg-gui modifiers held for this stroke.
    pub fn agg_modifiers(&self) -> Modifiers {
        self.key().to_agg_modifiers()
    }
}
