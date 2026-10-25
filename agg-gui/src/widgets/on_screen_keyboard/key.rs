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
    /// Dismiss the keyboard (no-op on the focused widget). The small
    /// downward chevron below the keys' bottom-left corner, where the
    /// phone shows its own hide-keyboard control.
    Dismiss,
    /// Start or stop dictation: the microphone key, shown while a voice
    /// provider is registered (`voice.rs`).
    Voice,
    /// Takes the touch and does nothing: where the dictation status shows
    /// beside the microphone (the rest of Gboard's toolbar row, the
    /// iPhone's strip between the emoji key and the microphone).
    Inert,
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
    /// ⌴ bracket — the space key on the numeric pad (the main spacebar
    /// is blank, like Gboard's).
    Space,
    /// Magnifier — Enter in a search field.
    Search,
    /// Check mark — Enter that finishes the field.
    Done,
    /// Right arrow — Enter that moves to the next field.
    Next,
    /// Left arrow — Enter that moves to the previous field.
    Previous,
    /// Arrow into a bar — Enter that opens what was typed.
    Go,
    /// Paper plane — Enter that sends a message.
    Send,
    /// Smiling face — the iPhone's emoji key, below the keys.
    Emoji,
    /// Microphone — the voice-input key, drawn by `voice_paint.rs` (Font
    /// Awesome's microphone) in its current state.
    Microphone,
}

impl KeyGlyph {
    /// The Enter key's icon for what Enter does in the focused field.
    pub fn for_enter(action: super::enter_action::EnterAction) -> Self {
        use super::enter_action::EnterAction as E;
        match action {
            E::Newline => KeyGlyph::Return,
            E::Done => KeyGlyph::Done,
            E::Search => KeyGlyph::Search,
            E::Next => KeyGlyph::Next,
            E::Previous => KeyGlyph::Previous,
            E::Go => KeyGlyph::Go,
            E::Send => KeyGlyph::Send,
        }
    }
}

/// One key cell positioned and painted by the layout engine. Stored in
/// [`super::state::KeyboardState::last_painted_keys`] so taps can be
/// hit-tested in O(n).
///
/// `rect` is the drawn face; `hit` is the larger area that takes touches
/// for it (out to the midlines between keys and the panel's edges, see
/// `hit.rs`), so the keys' hit areas cover the whole panel.
#[derive(Debug, Clone)]
pub struct PaintedKey {
    /// The drawn key face in viewport coordinates (Y-up).
    pub rect: Rect,
    /// The area that takes touches for this key (viewport, Y-up): the
    /// face grown to the midlines between keys and to the panel's edges.
    pub hit: Rect,
    /// Action committed on release.
    pub action: KeyAction,
    /// Cap as it was painted (kept for inspection / accessibility). A
    /// letter's cap shows the case it types now: lower case, or upper
    /// case while Shift or Caps Lock is on.
    pub cap: KeyCap,
}
