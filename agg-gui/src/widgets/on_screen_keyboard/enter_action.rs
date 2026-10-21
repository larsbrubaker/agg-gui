//! What the focused field tells the on-screen keyboard about itself beyond
//! its input mode: what its Enter key does and whether a sentence starts
//! with a capital.
//!
//! The phone's keyboard draws Enter as the action it performs (a magnifier
//! in a search box, a check mark when it finishes the form, an arrow when
//! it moves to the next field) and capitalises only fields that hold
//! sentences. A widget reports both through
//! [`Widget::text_input_hints`](crate::widget::Widget::text_input_hints);
//! [`super::set_text_input_focused_with_hints`] stores them for the paint
//! pass (`paint.rs`) and the sentence-start rule ([`super`]).
//! The Enter key still sends the same `Key::Enter` to the field whatever
//! it shows; the field decides what Enter means.

/// What pressing Enter does in the focused field, which decides the Enter
/// key's icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnterAction {
    /// Starts a new line (multi-line text): the bent return arrow.
    #[default]
    Newline,
    /// Finishes editing the field: a check mark.
    Done,
    /// Runs a search: a magnifier.
    Search,
    /// Moves to the next field: a right arrow.
    Next,
    /// Moves to the previous field: a left arrow.
    Previous,
    /// Opens or follows what was typed (an address, a command): an arrow
    /// into a bar.
    Go,
    /// Sends a message: a paper plane.
    Send,
}

/// The focused field's hints for the on-screen keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextInputHints {
    /// What Enter does; picks the Enter key's icon.
    pub enter_action: EnterAction,
    /// `true` when the field holds sentences: the keyboard opens with
    /// Shift on when the field is empty or its text ends a sentence.
    /// `false` (a name, a search, a number) always opens in lower case.
    pub auto_capitalize: bool,
}

impl TextInputHints {
    /// A one-line field: Enter finishes it (check mark), and letters open
    /// in lower case, as the phone does for a plain text box.
    pub const SINGLE_LINE: Self = Self {
        enter_action: EnterAction::Done,
        auto_capitalize: false,
    };
    /// Multi-line text: Enter starts a new line, and sentences start with
    /// a capital.
    pub const MULTI_LINE: Self = Self {
        enter_action: EnterAction::Newline,
        auto_capitalize: true,
    };
}

impl Default for TextInputHints {
    /// [`TextInputHints::MULTI_LINE`]: what a text widget that says nothing
    /// about itself gets.
    fn default() -> Self {
        Self::MULTI_LINE
    }
}
