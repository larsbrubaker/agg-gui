//! How far the folder tree is scrolled ([`TreeScroll`]), kept in the [`TreeModel`](super::TreeModel)
//! so it survives the app building the tree widget again.
//!
//! When the rows are taller than the tree, the wheel scrolls them (agg-gui's notches, scaled by
//! `PIXELS_PER_NOTCH` as a `ScrollView` scales them), never past the first row's top margin or the
//! last row's bottom. Each time the browser shows another folder, that folder's row is scrolled
//! just into sight (the least movement that shows it whole), so the row drawn selected is always
//! one the user can see. A wheel over a tree whose rows all fit is left to whatever is behind it.

/// The tree's scroll offset: how far the rows have moved up, in logical units (0 = the top).
#[derive(Clone, Debug, Default)]
pub struct TreeScroll {
    offset: f64,
    /// The largest offset at the last layout: the rows' length less the tree's height.
    max: f64,
    /// The browser navigation whose showing row was last scrolled into sight.
    revealed_at: Option<u64>,
}

impl TreeScroll {
    /// How far the rows have moved up.
    pub fn offset(&self) -> f64 {
        self.offset
    }

    /// Fits the offset to rows `content` long in a tree `height` tall: the showing row (its top
    /// and bottom from the content's top, `showing`) scrolled into sight once per `navigation`,
    /// and the offset kept within the rows. Answers the offset.
    pub(crate) fn fit(
        &mut self,
        content: f64,
        height: f64,
        showing: Option<(f64, f64)>,
        navigation: u64,
    ) -> f64 {
        self.max = (content - height).max(0.0);
        if self.revealed_at != Some(navigation) {
            if let Some((top, bottom)) = showing {
                if top < self.offset {
                    self.offset = top;
                } else if bottom > self.offset + height {
                    self.offset = bottom - height;
                }
                self.revealed_at = Some(navigation);
            }
        }
        self.offset = self.offset.clamp(0.0, self.max);
        self.offset
    }

    /// A wheel turned `delta_y` notches (positive to see what is above): answers whether the
    /// rows moved, false when they all fit or are already at that end.
    pub(crate) fn wheel(&mut self, delta_y: f64) -> bool {
        let before = self.offset;
        self.offset =
            (self.offset - delta_y * agg_gui::wheel::PIXELS_PER_NOTCH).clamp(0.0, self.max);
        self.offset != before
    }
}
