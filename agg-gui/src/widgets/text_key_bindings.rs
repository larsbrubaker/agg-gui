//! Which modifier chords mean word-wise and line-wise caret motion and
//! deletion: a port of the binding predicates in agg-sharp's
//! `Gui/TextWidgets/InternalTextEditWidget.cs` (`WordJumpRequested`,
//! `MacCommandRequested`). `TextField` and `TextArea` both decide their
//! arrow, Backspace and Delete keys here, so the two editors agree.
//!
//! The switch is [`crate::platform::use_mac_key_bindings`]:
//! - **Mac:** Option moves (and deletes) by word; Command moves to the
//!   start/end of the line (Left/Right) or of the document (Up/Down) and
//!   Command+Backspace deletes to the start of the line.
//! - **Windows/Linux:** Control moves by word, Control+Home/End go to the
//!   start/end of the document.
//!
//! C#'s Mac platform layer folds Command *and* physical Control onto
//! `Keys.Control`; agg-gui delivers Command as `meta`, so on Mac either
//! one is the Command chord here, which keeps C#'s behaviour for both keys.

use crate::event::Modifiers;
use crate::platform::use_mac_key_bindings;

/// Whether this chord asks to move (or delete) a whole word at a time:
/// Option on Mac, Control elsewhere.
pub fn word_jump_requested(mods: Modifiers) -> bool {
    if use_mac_key_bindings() {
        mods.alt
    } else {
        mods.ctrl
    }
}

/// Whether this chord is a Mac Command chord, which on the arrow keys means
/// line-wise (Left/Right) or document-wise (Up/Down) motion. Always false
/// off Mac: Windows spells those Home/End and Control+Home/End.
pub fn mac_command_requested(mods: Modifiers) -> bool {
    use_mac_key_bindings() && (mods.meta || mods.ctrl)
}

/// Whether an unshifted motion key with these modifiers ends the selection.
/// A plain motion does; so does a Mac Command-arrow, which is a plain caret
/// motion on Mac. Windows' Control-arrow is a word jump and keeps it (C#'s
/// `turnOffSelection` arm at the top of `OnKeyDown`).
pub fn motion_ends_selection(mods: Modifiers) -> bool {
    !mods.shift && (!(mods.ctrl || mods.meta) || mac_command_requested(mods))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{override_platform_for_thread, Platform};

    fn mods(ctrl: bool, alt: bool, meta: bool) -> Modifiers {
        Modifiers {
            ctrl,
            alt,
            meta,
            ..Modifiers::default()
        }
    }

    #[test]
    fn mac_uses_option_for_words_and_command_for_lines() {
        let _mac = override_platform_for_thread(Platform::MacOS);
        assert!(word_jump_requested(mods(false, true, false)));
        assert!(!word_jump_requested(mods(true, false, false)));
        assert!(mac_command_requested(mods(false, false, true)));
        assert!(mac_command_requested(mods(true, false, false)));
        assert!(!mac_command_requested(mods(false, true, false)));
    }

    #[test]
    fn windows_uses_control_for_words_and_has_no_command_chord() {
        let _win = override_platform_for_thread(Platform::Windows);
        assert!(word_jump_requested(mods(true, false, false)));
        assert!(!word_jump_requested(mods(false, true, false)));
        assert!(!mac_command_requested(mods(true, false, true)));
    }
}
