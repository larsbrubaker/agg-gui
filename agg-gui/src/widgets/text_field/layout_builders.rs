//! Layout-trait builders for `TextField`.
//!
//! Carved out of `text_field.rs` so the parent module stays under the
//! project's 800-line cap.  Every method here is a thin chained-style
//! setter on `WidgetBase` — they don't touch text-editing state, so
//! they read better as their own cohesive block.

use super::TextField;
use crate::geometry::Size;
use crate::layout_props::{HAnchor, Insets, VAnchor};

impl TextField {
    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }
}
