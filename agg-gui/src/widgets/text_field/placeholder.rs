//! The hint an empty `TextField` shows in its dim colour: a fixed
//! [`placeholder`](super::TextField::placeholder), or a shared cell whose
//! text the owner changes while the field is on screen (a suggestion that
//! follows another control's choice).

use std::cell::RefCell;
use std::rc::Rc;

use super::TextField;

impl TextField {
    /// Shows `cell`'s text as the hint while the field is empty, read at
    /// every paint, so writing the cell changes the hint without rebuilding
    /// the field.  It takes the place of a fixed
    /// [`with_placeholder`](Self::with_placeholder) hint.
    pub fn with_placeholder_cell(mut self, cell: Rc<RefCell<String>>) -> Self {
        self.placeholder_cell = Some(cell);
        self
    }

    /// The hint shown now: the shared cell's text when there is one, else
    /// the fixed `placeholder`.
    pub fn placeholder_text(&self) -> String {
        match &self.placeholder_cell {
            Some(cell) => cell.borrow().clone(),
            None => self.placeholder.clone(),
        }
    }
}
