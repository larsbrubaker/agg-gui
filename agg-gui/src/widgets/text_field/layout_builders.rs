//! Layout-trait builders for `TextField`.
//!
//! Carved out of `text_field.rs` so the parent module stays under the
//! project's 800-line cap.  Every method here is a thin chained-style
//! setter on `WidgetBase` — they don't touch text-editing state, so
//! they read better as their own cohesive block.  `natural_height` (the
//! height `layout` reports) lives here too, beside the line-box builder,
//! together with the per-side text insets that feed it.

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

    /// Inset the text by a different amount on each side instead of the
    /// uniform `padding`.  `left` / `right` bound the text clip, caret and
    /// click mapping; `top` / `bottom` add to the one-em height under
    /// [`LineBox::Em`] and shift the text's vertical centre.  Lets a field
    /// push its text past a leading label without growing taller.  Unset
    /// (the default), every side uses `padding`.
    pub fn with_text_insets(mut self, insets: Insets) -> Self {
        self.text_insets = Some(insets);
        self
    }

    /// The effective text insets: [`with_text_insets`](Self::with_text_insets)
    /// when set, otherwise `padding` on every side.
    pub fn text_insets(&self) -> Insets {
        self.text_insets.unwrap_or(Insets {
            left: self.padding,
            right: self.padding,
            top: self.padding,
            bottom: self.padding,
        })
    }

    /// The height `layout` reports before the `min_size` floor.
    /// [`LineBox::Standard`]: 2.4 em, at least 28 px.  [`LineBox::Em`]: one
    /// em of text plus the top and bottom insets — agg-sharp's
    /// `TextEditWidget` is one em tall and `ThemedTextEditWidget` pads it on
    /// every side (raise it to a field design height with `with_min_size`,
    /// which `layout` honours).
    pub(super) fn natural_height(&self) -> f64 {
        match self.line_box.unwrap_or_else(current_line_box) {
            LineBox::Standard => (self.font_size * 2.4).max(28.0),
            LineBox::Em => {
                let i = self.text_insets();
                self.font_size + i.top + i.bottom
            }
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
    /// Floor for the size `layout` reports (width and height); the default
    /// `Size::ZERO` leaves the natural size untouched.
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }
}
