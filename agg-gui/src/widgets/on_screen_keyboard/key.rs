//! Individual key cell — geometry, label, and the action a tap commits.

use crate::geometry::Rect;

use super::layouts::Layer;

/// What happens when the user releases a tap on this key.
#[derive(Debug, Clone, Copy)]
pub enum KeyAction {
    /// Insert a literal character at the focused widget's cursor. The
    /// keyboard synthesizes `Event::KeyDown { Key::Char(c), … }` for
    /// every letter / digit / punctuation key.
    Char(char),
    /// Delete one character / grapheme to the left of the cursor.
    Backspace,
    /// Submit the form / accept the value. The host widget (TextField,
    /// TextArea) decides what "submit" means.
    Enter,
    /// Insert a single space — kept separate from `Char(' ')` because
    /// the visual / layout treatment for the spacebar is special.
    Space,
    /// Switch the visible layer (letters / shift / numbers / symbols).
    /// Internal to the keyboard; never reaches the focused widget.
    Switch(Layer),
    /// Dismiss the keyboard (no-op on the focused widget). Maps to the
    /// "downward chevron" key common on iOS keyboards.
    Dismiss,
}

/// Visual label rendered on a key. Either text or a tiny glyph.
#[derive(Debug, Clone)]
pub enum KeyCap {
    /// A single character or short word ("space", "ABC", "123").
    Text(String),
    /// A vector glyph drawn by `key::paint_glyph`. Used for keys whose
    /// label is a symbol that doesn't have a satisfying Unicode form
    /// (e.g. iOS / Android backspace, return arrow).
    Glyph(KeyGlyph),
}

/// Built-in glyphs the keyboard paints procedurally (no font lookup
/// required so the keyboard renders even before the host app has
/// installed a typeface).
#[derive(Debug, Clone, Copy)]
pub enum KeyGlyph {
    /// Left-pointing chevron with an X — backspace.
    Backspace,
    /// Up-arrow into a horizontal bar — shift.
    Shift,
    /// Down-pointing chevron — dismiss keyboard.
    DismissDown,
    /// Bent arrow — return / enter.
    Return,
}

/// One key cell positioned and painted by the layout engine. Stored in
/// [`super::state::KeyboardState::last_painted_keys`] so taps can be
/// hit-tested in O(n).
#[derive(Debug, Clone)]
pub struct PaintedKey {
    /// Hit-test rectangle in viewport coordinates (Y-up).
    pub rect: Rect,
    /// Action committed on release.
    pub action: KeyAction,
    /// Cap as it was painted (kept for inspection / accessibility).
    #[allow(dead_code)]
    pub cap: KeyCap,
}
