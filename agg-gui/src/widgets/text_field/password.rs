//! Password-mode support for [`TextField`](super::TextField).
//!
//! Split out of `text_field.rs` to keep that file under the project's 800-line
//! cap. Holds the builders that enable masking and the runtime "reveal"
//! override (a shared cell that the demo's eye-toggle button flips), plus the
//! `masking_active` helper the render/hit-test paths consult to decide whether
//! to substitute bullet glyphs for the real text.

use std::cell::Cell;
use std::rc::Rc;

use super::TextField;

impl TextField {
    /// Enable/disable masking. When on, every character renders as '•'.
    pub fn with_password_mode(mut self, v: bool) -> Self {
        self.password_mode = v;
        self
    }

    /// Bind a shared "reveal" cell. While the cell holds `true`, a
    /// password-mode field renders its plaintext (the eye-toggle affordance in
    /// the demo flips this cell). Has no effect when `password_mode` is `false`.
    pub fn with_password_reveal_cell(mut self, cell: Rc<Cell<bool>>) -> Self {
        self.password_reveal = Some(cell);
        self
    }

    /// Whether characters should currently be masked: password mode is on and
    /// no reveal cell is forcing plaintext.
    #[inline]
    pub fn masking_active(&self) -> bool {
        self.password_mode && !self.password_reveal.as_ref().is_some_and(|c| c.get())
    }
}
