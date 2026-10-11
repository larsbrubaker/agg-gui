//! A selection in a [`super::RichDocument`], [`RichSelection`]: where it
//! started (the anchor) and where the caret is.  Ports agg-sharp's
//! `RichSelection.cs`.  The edit operations ([`super::edit_operations`],
//! [`super::table_code_operations`]) return one so an operation can either
//! place a caret or, like Backspace onto a Raw block, select something whole
//! for the next keystroke to act on.

use super::document::DocPosition;

/// A selection: an empty one (anchor == caret) is a plain caret.
///
/// `whole_block` marks a selection of one block as an object (set by
/// [`super::edit_operations::whole_block`]): deleting it removes the block.
/// Without it, a range is text, even one that happens to span all of a code
/// block's or table cell's text, so selecting that text and typing replaces
/// the text and keeps the block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RichSelection {
    /// Where the selection started.
    pub anchor: DocPosition,
    /// Where the caret is.
    pub caret: DocPosition,
    /// The selection is one block as an object.
    pub whole_block: bool,
}

impl RichSelection {
    /// A text selection from `anchor` to `caret`.
    pub fn new(anchor: DocPosition, caret: DocPosition) -> Self {
        Self {
            anchor,
            caret,
            whole_block: false,
        }
    }

    /// A plain caret (C#'s `RichSelection.At`).
    pub fn at(caret: DocPosition) -> Self {
        Self::new(caret, caret)
    }

    /// True for a plain caret.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.caret
    }

    /// The earlier end.
    pub fn start(&self) -> DocPosition {
        self.anchor.min(self.caret)
    }

    /// The later end.
    pub fn end(&self) -> DocPosition {
        self.anchor.max(self.caret)
    }
}
