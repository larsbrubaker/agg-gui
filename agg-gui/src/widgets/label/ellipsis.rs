//! `Label`'s ellipsis settings: agg-sharp's `TextWidget.EllipsisIfClipped`
//! ("..." at the end) and the middle / path-aware middle modes of
//! [`EllipsisMode`].  The cutting itself is [`crate::text::elide_text`];
//! `Label::paint` (in `label.rs`) calls it with the paint context's
//! measurement, and [`Label::shown_text`] with the label's own font metrics.

use super::Label;
use crate::text::EllipsisMode;

impl Label {
    /// agg-sharp `TextWidget.EllipsisIfClipped`: `true` shortens a too-wide
    /// single line at its end ([`EllipsisMode::End`]); `false` turns any
    /// ellipsis mode off.
    pub fn with_ellipsis_if_clipped(mut self, on: bool) -> Self {
        self.set_ellipsis_if_clipped(on);
        self
    }

    /// agg-sharp `TextWidget.EllipsisIfClipped` setter; the same as
    /// `set_ellipsis_mode(on.then_some(EllipsisMode::End))`.
    pub fn set_ellipsis_if_clipped(&mut self, on: bool) {
        self.set_ellipsis_mode(on.then_some(EllipsisMode::End));
    }

    /// Shorten a too-wide single line with `mode` — e.g.
    /// [`EllipsisMode::PathMiddle`] for file paths
    /// (`/Users/alex/…/node_modules/esm`).  Wrapped labels never ellipsize.
    pub fn with_ellipsis_mode(mut self, mode: EllipsisMode) -> Self {
        self.set_ellipsis_mode(Some(mode));
        self
    }

    /// Set (`Some`) or turn off (`None`) the ellipsis mode.
    pub fn set_ellipsis_mode(&mut self, mode: Option<EllipsisMode>) {
        if self.ellipsis != mode {
            self.ellipsis = mode;
            self.cache.invalidate();
        }
    }

    /// The current ellipsis mode, `None` when the label is clipped instead.
    pub fn ellipsis_mode(&self) -> Option<EllipsisMode> {
        self.ellipsis
    }

    /// agg-sharp `TextWidget.EllipsisActive`: ellipsis is on and the full text is
    /// wider than the label's laid-out bounds (single-line labels only).
    pub fn ellipsis_active(&self) -> bool {
        self.ellipsis.is_some()
            && !self.wrap
            && self.layout_text == self.text
            && self.layout_width > self.bounds.width
    }

    /// The text a paint draws now: the full text, or its ellipsized form while
    /// [`Self::ellipsis_active`].
    pub fn shown_text(&self) -> String {
        match self.ellipsis {
            Some(mode) if self.ellipsis_active() => crate::text::elide_to_width(
                &self.active_font(),
                &self.text,
                self.active_font_size(),
                self.bounds.width,
                mode,
            ),
            _ => self.text.clone(),
        }
    }
}
