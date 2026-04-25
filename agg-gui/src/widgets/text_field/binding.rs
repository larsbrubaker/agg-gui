use super::*;

impl TextField {
    /// Bind the field to external text state.
    ///
    /// `layout` picks up external writes (e.g. a Clear button) and
    /// `on_change` writes user edits back into the cell.
    pub fn with_text_cell(mut self, cell: Rc<RefCell<String>>) -> Self {
        let text = cell.borrow().clone();
        self.set_text(text);
        self.text_cell = Some(cell);
        self
    }

    pub(crate) fn sync_from_text_cell(&mut self) {
        let Some(cell) = &self.text_cell else {
            return;
        };
        let external = cell.borrow().clone();
        if external != self.edit.borrow().text {
            self.set_text(external);
        }
    }
}
