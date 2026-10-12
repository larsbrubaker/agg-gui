//! Replacing a [`super::ComboBox`]'s options after it is built.
//!
//! A combo normally keeps the list it was built with. A host whose list can
//! change while the combo is in the tree (MatterCAD's `FontSelectorField`,
//! whose property can be set to a face its list does not hold yet) either
//! calls [`ComboBox::set_options`] on a combo it owns, or binds an external
//! list with [`ComboBox::with_options_cell`], which `layout()` reads every
//! frame (the same pattern as `with_selected_cell`; the options are applied
//! before the selected cell, so an index written with a new list means a row
//! of that list).
//!
//! Split out of `combo_box.rs` to keep that file under the 800-line limit.

use super::*;

impl ComboBox {
    /// Bind the options to an external list: `layout()` takes the list
    /// whenever it differs from the combo's own (as [`ComboBox::set_options`]),
    /// except while the popup is open. The cell starts as the combo's list
    /// when it is empty.
    pub fn with_options_cell(mut self, cell: Rc<RefCell<Vec<String>>>) -> Self {
        if cell.borrow().is_empty() {
            cell.borrow_mut().clone_from(&self.options);
        } else {
            let options = cell.borrow().clone();
            self.set_options(options);
        }
        self.options_cell = Some(cell);
        self
    }

    /// Replace the options. The selected index is kept (clamped to the new
    /// list) and its label shows the new text; the open list closes. Eager
    /// per-item fonts ([`ComboBox::with_item_fonts`]), tooltips and disabled
    /// rows stay attached to their indices; lazily resolved fonts
    /// ([`ComboBox::with_item_font_fn`]) are asked for again. Does not fire
    /// `on_change`.
    pub fn set_options(&mut self, options: Vec<impl Into<String>>) {
        let options: Vec<String> = options.into_iter().map(Into::into).collect();
        if options == self.options {
            return;
        }
        self.options = options;
        let size = self.font_size;
        let labels: Vec<Label> = self
            .options
            .iter()
            .enumerate()
            .map(
                |(i, text)| match self.item_fonts.as_ref().and_then(|f| f.get(i)) {
                    Some(font) => Label::new(text, Arc::clone(font))
                        .with_font_size(size)
                        .with_ignore_system_font(true),
                    None => Self::make_label(text, size, Arc::clone(&self.font)),
                },
            )
            .collect();
        *self.item_labels.borrow_mut() = labels;
        if let Some(lazy) = &self.lazy_item_fonts {
            lazy.forget();
        }
        self.open = false;
        self.hovered_item = None;
        self.type_ahead = None;
        let n = self.options.len();
        self.key_highlight = self.key_highlight.min(n.saturating_sub(1));
        self.scroll_offset = self
            .scroll_offset
            .min(n.saturating_sub(self.popup_visible_count));
        if n == 0 {
            self.selected = 0;
            if self.has_selection {
                self.selected_label.set_text("");
            }
        } else {
            self.selected = self.selected.min(n - 1);
            if self.has_selection {
                self.set_selected(self.selected);
            }
        }
        crate::animation::request_draw();
    }

    /// The options, in order.
    pub fn options(&self) -> &[String] {
        &self.options
    }

    /// The text the closed box shows for the selection; `None` while nothing
    /// is selected or the list is empty.
    pub fn selected_text(&self) -> Option<&str> {
        if !self.has_selection {
            return None;
        }
        self.options.get(self.selected).map(String::as_str)
    }

    /// Take the bound options cell's list when it changed (`layout()`).
    pub(super) fn sync_options_cell(&mut self) {
        let Some(cell) = &self.options_cell else {
            return;
        };
        if *cell.borrow() != self.options {
            let options = cell.borrow().clone();
            self.set_options(options);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> Arc<Font> {
        Arc::new(
            Font::from_slice(include_bytes!("../../../../demo/assets/CascadiaCode.ttf"))
                .expect("test font"),
        )
    }

    #[test]
    fn set_options_replaces_the_list_and_keeps_the_index() {
        let mut combo = ComboBox::new(vec!["A", "B", "C"], 1, font());
        combo.set_options(vec!["A", "Inserted", "B", "C"]);
        assert_eq!(combo.options(), ["A", "Inserted", "B", "C"]);
        assert_eq!(combo.selected(), 1);
        assert_eq!(combo.selected_text(), Some("Inserted"));
        assert_eq!(combo.item_labels.borrow().len(), 4);
        assert_eq!(combo.item_labels.borrow()[1].text_str(), "Inserted");
    }

    #[test]
    fn set_options_clamps_the_selection_to_a_shorter_list() {
        let mut combo = ComboBox::new(vec!["A", "B", "C"], 2, font());
        combo.set_options(vec!["X"]);
        assert_eq!(combo.selected(), 0);
        assert_eq!(combo.selected_text(), Some("X"));
        combo.set_options(Vec::<String>::new());
        assert_eq!(combo.selected_text(), None);
    }

    #[test]
    fn options_cell_list_is_taken_before_the_selected_cell_at_layout() {
        let options = Rc::new(RefCell::new(Vec::new()));
        let selected = Rc::new(Cell::new(0));
        let mut combo = ComboBox::new(vec!["A", "C"], 0, font())
            .with_options_cell(Rc::clone(&options))
            .with_selected_cell(Rc::clone(&selected));
        assert_eq!(
            *options.borrow(),
            ["A", "C"],
            "an empty cell starts as the list"
        );
        // The host inserts a row and selects it in one step.
        *options.borrow_mut() = vec!["A".into(), "B".into(), "C".into()];
        selected.set(1);
        combo.layout(Size::new(200.0, 40.0));
        assert_eq!(combo.options(), ["A", "B", "C"]);
        assert_eq!(combo.selected_text(), Some("B"));
    }
}
