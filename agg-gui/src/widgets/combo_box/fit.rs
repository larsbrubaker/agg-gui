//! Content-fitted width for [`super::ComboBox`]: agg-sharp's `DropDownList`
//! (MatterCAD's `MHDropDownList`) is `HAnchor.Fit`, as wide as its widest
//! option, so it sits beside other controls in a row instead of taking the
//! whole row. Off by default — a `ComboBox` otherwise reports the full width
//! it is offered, as it always has.
//!
//! Split out of `combo_box.rs` to keep that file under the 800-line limit.

use super::*;

impl ComboBox {
    /// Report the widest option (or the no-selection placeholder) plus the
    /// box's padding and arrow as this combo's width, never more than offered,
    /// instead of the whole offered width.
    pub fn with_fit_width(mut self, fit: bool) -> Self {
        self.fit_width = fit;
        self
    }

    /// Object-safe counterpart of [`with_fit_width`](Self::with_fit_width).
    pub fn set_fit_width(&mut self, fit: bool) {
        self.fit_width = fit;
    }

    /// Whether this combo fits its width to its options.
    pub fn fit_width(&self) -> bool {
        self.fit_width
    }

    /// The width the closed box needs to show any option (or the placeholder)
    /// uncut: the widest text, the left and right padding, and the arrow.
    /// agg-sharp sizes `DropDownList.MinimumSize` the same way, by measuring
    /// the closed text with every item's label in turn.
    pub(super) fn fit_content_width(&mut self) -> f64 {
        let unbounded = Size::new(f64::MAX, ITEM_H);
        let mut widest = self.selected_label.layout(unbounded).width;
        for label in self.item_labels.borrow_mut().iter_mut() {
            widest = widest.max(label.layout(unbounded).width);
        }
        (widest + PAD_X * 2.0 + ARROW_W).ceil()
    }
}
