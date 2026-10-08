//! The runner's keyboard calls — the port of `Type`, `PressModifierKeys`,
//! `ReleaseModifierKeys`, `SelectAll`, `SelectNone` and the `ModifierKeys`
//! flags (agg-sharp `GuiAutomation/AutomationRunner.cs`).
//!
//! Keys are real input: the type string is read by
//! [`TypedKeyParser`](crate::typed_key_parser::TypedKeyParser) and its
//! strokes go through the run's [`InputMethod`](crate::input::InputMethod)
//! (by default the shells' input forwarder), so they reach the focused
//! widget by the path a keyboard's do. The test body is the UI thread, so
//! every stroke has been delivered when the call returns; each call then
//! lets the UI catch up as C#'s does: one frame and `delay(0.2)` after
//! typing, `delay(0.2)` after a modifier change.
//!
//! The close chord `%{F4}` is not typed: it asks the window to close
//! ([`UiDriver::request_close`]), as C#'s input method closes the
//! `SystemWindow`.

use std::ops::{BitOr, BitOrAssign};

use super::AutomationRunner;
use crate::driver::{FrameKind, UiDriver};
use crate::keys::Keys;
use crate::typed_key_parser::TypedKeyParser;

/// The type string that closes the window instead of typing (Alt+F4).
pub const CLOSE_CHORD: &str = "%{F4}";

/// C# `AutomationRunner.ModifierKeys`: which modifiers
/// [`AutomationRunner::press_modifier_keys`] holds and
/// [`AutomationRunner::release_modifier_keys`] lets go of. Combine with `|`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ModifierKeys(u8);

impl ModifierKeys {
    pub const NONE: ModifierKeys = ModifierKeys(0);
    pub const SHIFT: ModifierKeys = ModifierKeys(0x1);
    /// The command modifier: Control, or Cmd on macOS (as `^` in a type
    /// string).
    pub const CONTROL: ModifierKeys = ModifierKeys(0x2);
    pub const ALT: ModifierKeys = ModifierKeys(0x4);

    /// Whether every flag of `other` is set.
    pub fn contains(self, other: ModifierKeys) -> bool {
        self.0 & other.0 == other.0
    }

    /// The `Keys` modifier bits and keys these flags stand for (C#
    /// `ShiftKey | Shift`, `ControlKey | Control`, `Menu | Alt`).
    pub fn to_keys(self) -> Keys {
        let mut keys = Keys::NONE;
        for (flag, key, bit) in [
            (ModifierKeys::SHIFT, Keys::SHIFT_KEY, Keys::SHIFT),
            (ModifierKeys::CONTROL, Keys::CONTROL_KEY, Keys::CONTROL),
            (ModifierKeys::ALT, Keys::MENU, Keys::ALT),
        ] {
            if self.contains(flag) {
                keys |= key | bit;
            }
        }
        keys
    }
}

impl BitOr for ModifierKeys {
    type Output = ModifierKeys;
    fn bitor(self, rhs: ModifierKeys) -> ModifierKeys {
        ModifierKeys(self.0 | rhs.0)
    }
}

impl BitOrAssign for ModifierKeys {
    fn bitor_assign(&mut self, rhs: ModifierKeys) {
        self.0 |= rhs.0;
    }
}

impl AutomationRunner {
    /// C# `Type`: put `text_to_type` through the window, stroke by stroke
    /// (`^` is the command modifier on the next key, `^+` command and shift,
    /// `{Enter}` a named key; see
    /// [`TypedKeyParser`](crate::typed_key_parser::TypedKeyParser)), then
    /// let one frame and 0.2 s of UI time pass. `%{F4}` asks the window to
    /// close instead.
    ///
    /// Panics with the parser's message when the string names a key that
    /// does not exist (C# throws `ArgumentException`), so a test that meant
    /// to press something it did not fails rather than passing quietly.
    pub fn type_text(&mut self, text_to_type: &str) -> &mut Self {
        self.check_not_timed_out();
        if text_to_type == CLOSE_CHORD {
            self.driver.request_close();
        } else {
            let strokes = match TypedKeyParser::parse(text_to_type) {
                Ok(strokes) => strokes,
                Err(error) => panic!("{error}"),
            };
            self.input.type_strokes(&mut self.driver, &strokes);
        }
        self.pump_frame(FrameKind::Reactive);
        self.delay(0.2)
    }

    /// C# `PressModifierKeys`: hold `modifier_keys` (on top of any already
    /// held) until [`release_modifier_keys`](Self::release_modifier_keys),
    /// then let 0.2 s of UI time pass. Strokes and clicks in between carry
    /// them. `NONE` does nothing.
    pub fn press_modifier_keys(&mut self, modifier_keys: ModifierKeys) -> &mut Self {
        self.check_not_timed_out();
        if modifier_keys == ModifierKeys::NONE {
            return self;
        }
        self.input
            .press_modifier_keys(&mut self.driver, modifier_keys.to_keys());
        self.delay(0.2)
    }

    /// C# `ReleaseModifierKeys`: let go of `modifier_keys` (others stay
    /// held), then let 0.2 s of UI time pass. `NONE` does nothing.
    pub fn release_modifier_keys(&mut self, modifier_keys: ModifierKeys) -> &mut Self {
        self.check_not_timed_out();
        if modifier_keys == ModifierKeys::NONE {
            return self;
        }
        self.input
            .release_modifier_keys(&mut self.driver, modifier_keys.to_keys());
        self.delay(0.2)
    }

    /// C# `SelectAll`: select everything in the focused widget (`"^a"`).
    pub fn select_all(&mut self) -> &mut Self {
        self.type_text("^a")
    }

    /// C# `SelectNone`: clear the selection by typing a space over it
    /// (`" "`), exactly as C# does — the selected text is replaced.
    pub fn select_none(&mut self) -> &mut Self {
        self.type_text(" ")
    }
}
