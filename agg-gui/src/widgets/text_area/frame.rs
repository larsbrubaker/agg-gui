//! The frame of a [`TextArea`]: its background fill and border, and the switch
//! that turns both off so a host can make its own frame the field (agg-sharp's
//! `TextEditWidget` with `Border = 0` and a transparent background, as
//! MatterCAD's AI chat bar uses it).
//!
//! Split out of `widget_impl.rs` (the paint pass that calls
//! [`TextArea::paint_background`]) next to `band.rs`, which holds the border
//! stroke and the band-mode frame overlay that also honour the switch.
//!
//! A frameless area keeps its padding, so text sits exactly where it does in a
//! framed one. It draws no focus ring (the framed border's focus colour): the
//! host frame shows focus. It also never rasters an over-scan band: the band
//! relies on an opaque background to overwrite stale pixels in its retained
//! buffer and on the padding-ring fill to hide text scrolled into the padding,
//! neither of which a transparent area can paint. Without the band the raster
//! is clipped to the padded inner rect at the live scroll offset, and a scroll
//! re-rasters (the cache signature tracks the offset then).
//!
//! The per-widget [`TextFieldTheme`] colours (`with_theme`) live here too, with
//! the helpers the paint pass and `band.rs` resolve them through.

use super::*;
use crate::color::Color;
use crate::widgets::TextFieldTheme;

impl TextArea {
    /// Paint the background and border (`true`, the default) or leave both to
    /// the host (`false`). The text insets stay the same either way.
    pub fn with_frame(mut self, frame: bool) -> Self {
        self.frame = frame;
        self
    }

    /// Switch the frame on or off after construction (see
    /// [`with_frame`](Self::with_frame)). The next layout re-rasters.
    pub fn set_frame(&mut self, frame: bool) {
        if self.frame != frame {
            self.frame = frame;
            crate::animation::request_draw();
        }
    }

    /// Whether the background and border are painted.
    pub fn has_frame(&self) -> bool {
        self.frame
    }

    /// Fill the background into the backbuffer. In band mode fill the WHOLE
    /// buffer (incl. the over-scan margins that sit outside the widget bounds)
    /// with a plain opaque rect so scrolling never reveals an un-painted gap;
    /// the rounded frame + border + padding-ring are re-established, fixed, in
    /// `paint_overlay`. For a strip re-raster (`strip` is its `(lo, hi)` extent)
    /// fill only the strip rows (full width, opaque) so the changed lines' old
    /// pixels are replaced while the retained buffer keeps every other row.
    /// Otherwise bake the rounded fill into the cache. Frameless: nothing (no
    /// band or strip runs then, so the fresh buffer is already transparent).
    pub(super) fn paint_background(
        &self,
        ctx: &mut dyn DrawCtx,
        v: &crate::theme::Visuals,
        strip: Option<(f64, f64)>,
    ) {
        if !self.frame {
            return;
        }
        let w = self.bounds.width;
        let h = self.bounds.height;
        ctx.set_fill_color(self.background_color(v));
        ctx.begin_path();
        if let Some((lo, hi)) = strip {
            ctx.rect(0.0, lo, w, (hi - lo).max(0.0));
        } else if self.band.active {
            let bg_lo = -self.band.over_bottom;
            let bg_h = h + self.band.over_top + self.band.over_bottom;
            ctx.rect(0.0, bg_lo, w, bg_h.max(0.0));
        } else {
            ctx.rounded_rect(0.0, 0.0, w, h, self.corner_radius());
        }
        ctx.fill();
    }

    /// Install a [`TextFieldTheme`] of per-widget colour overrides, read as a
    /// [`TextField`](crate::widgets::TextField) reads them: background, text
    /// and border per state (focus over hover over normal), the hint colours,
    /// the selection and caret colours, the corner radius (default 4) and the
    /// focused border width. Unset colours fall back to the ambient visuals.
    /// A text area always shows its hint when empty, so
    /// `show_placeholder_when_focused` does not apply.
    pub fn with_theme(mut self, theme: TextFieldTheme) -> Self {
        self.theme = theme;
        self
    }

    /// Replace the colour overrides after construction (a live theme switch).
    /// The next layout re-rasters.
    pub fn set_theme(&mut self, theme: TextFieldTheme) {
        self.theme = theme;
        self.last_sig = None;
        crate::animation::request_draw();
    }

    /// The colour overrides in use.
    pub fn theme(&self) -> &TextFieldTheme {
        &self.theme
    }

    /// The background for the current focus and hover state.
    pub(super) fn background_color(&self, v: &crate::theme::Visuals) -> Color {
        self.theme
            .background_for(self.focused, self.hovered, v.widget_bg)
    }

    /// The text colour for the current focus and hover state.
    pub(super) fn text_color(&self, v: &crate::theme::Visuals) -> Color {
        self.theme
            .text_color_for(self.focused, self.hovered, v.text_color)
    }

    /// The frame's corner radius: the theme's, else 4.
    pub(super) fn corner_radius(&self) -> f64 {
        self.theme.border_radius.unwrap_or(4.0)
    }
}
