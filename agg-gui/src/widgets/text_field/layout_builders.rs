//! Layout-trait builders for `TextField`.
//!
//! Carved out of `text_field.rs` so the parent module stays under the
//! project's 800-line cap.  Every method here is a thin chained-style
//! setter on `WidgetBase` — they don't touch text-editing state, so
//! they read better as their own cohesive block.  `natural_height` (the
//! height `layout` reports) lives here too, beside the line-box builder.

use super::TextField;
use crate::font_settings::{current_line_box, LineBox};
use crate::geometry::Size;
use crate::layout_props::{HAnchor, Insets, VAnchor};

impl TextField {
    /// Pin this field's line box, ignoring the app-wide
    /// [`crate::font_settings::set_line_box`].
    pub fn with_line_box(mut self, line_box: LineBox) -> Self {
        self.line_box = Some(line_box);
        self
    }

    /// The height `layout` reports.  [`LineBox::Standard`]: 2.4 em, at
    /// least 28 px.  [`LineBox::Em`]: one em of text plus `padding` above
    /// and below — agg-sharp's `TextEditWidget` is one em tall and
    /// `ThemedTextEditWidget` pads it on every side (raise it to a field
    /// design height with `with_min_size`).
    pub(super) fn natural_height(&self) -> f64 {
        match self.line_box.unwrap_or_else(current_line_box) {
            LineBox::Standard => (self.font_size * 2.4).max(28.0),
            LineBox::Em => self.font_size + self.padding * 2.0,
        }
    }

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
