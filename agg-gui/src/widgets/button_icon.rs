//! Leading-icon support for [`super::Button`]: the [`ButtonIcon`] glyph
//! type, the `with_icon*` / `with_image_icon` builders, the width the icon
//! reserves in layout, and its painting.  Split out of `button.rs` (pulled in
//! via `#[path]` as a child module, like `button_events.rs`) so the parent
//! stays under the 800-line cap and keeps private-field access.
//!
//! A button shows at most one leading icon: an [`IconImage`] when one is set
//! (artwork icons, drawn at device resolution), otherwise the glyph.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::icon_image::IconImage;
use crate::text::{measure_advance, Font};

use super::Button;

/// Opacity of an image icon on a disabled button (glyph icons switch to the
/// disabled text colour instead).
const DISABLED_IMAGE_ALPHA: f64 = 0.45;

/// Icon glyph drawn at the leading edge of a [`Button`]'s label.
/// The glyph is rendered with a separate font so callers can pair
/// e.g. a Font Awesome glyph with a Latin-only text font.
#[derive(Clone)]
pub struct ButtonIcon {
    pub glyph: char,
    pub font: Arc<Font>,
    pub font_size: f64,
}

/// Spacing between the icon glyph and the label text, in pixels.
const ICON_GAP: f64 = 8.0;

impl Button {
    /// Paint an icon glyph at the leading edge of the label.
    /// `icon_font` carries the glyph (e.g. a Font Awesome face);
    /// the label text continues to render in the button's main
    /// font, so callers can pair a Latin text font with an
    /// icon-only font without merging them.
    ///
    /// Defaults `font_size` to the button's current `font_size`.
    /// Use [`with_icon_sized`](Self::with_icon_sized) to scale the
    /// icon independently.
    pub fn with_icon(mut self, glyph: char, icon_font: Arc<Font>) -> Self {
        let font_size = self.font_size;
        self.icon = Some(ButtonIcon {
            glyph,
            font: icon_font,
            font_size,
        });
        self
    }

    /// Like [`with_icon`](Self::with_icon) but with an explicit
    /// icon font size — useful when the icon font's glyphs read
    /// larger or smaller than the text at the same point size.
    pub fn with_icon_sized(mut self, glyph: char, icon_font: Arc<Font>, font_size: f64) -> Self {
        self.icon = Some(ButtonIcon {
            glyph,
            font: icon_font,
            font_size,
        });
        self
    }

    /// Paint `image` at the leading edge of the label instead of a glyph
    /// icon (it takes precedence over [`with_icon`](Self::with_icon)).  The
    /// image is drawn at its logical size, vertically centred, rasterised for
    /// the current device scale so it stays crisp on HiDPI displays.
    pub fn with_image_icon(mut self, image: IconImage) -> Self {
        self.icon_image = Some(image);
        self
    }

    /// Spacing reserved between the leading icon glyph and the label.
    /// Collapses to zero for icon-only buttons (empty label) so the glyph
    /// centres in the button instead of being shoved left by a gap that
    /// precedes no text.
    fn icon_gap(&self) -> f64 {
        if self.label_text.is_empty() {
            0.0
        } else {
            ICON_GAP
        }
    }

    /// Horizontal space the leading icon reserves (icon width + gap).
    /// Zero when no icon is configured.
    pub(super) fn icon_block_w(&self) -> f64 {
        if let Some(image) = &self.icon_image {
            return image.size().width + self.icon_gap();
        }
        self.icon
            .as_ref()
            .map(|i| measure_advance(&i.font, &i.glyph.to_string(), i.font_size) + self.icon_gap())
            .unwrap_or(0.0)
    }

    /// Paint the leading icon with its left edge at `x`.  Glyphs take
    /// `color`; images keep their own colours and fade when `enabled` is
    /// false.
    pub(super) fn paint_leading_icon(
        &self,
        ctx: &mut dyn DrawCtx,
        x: f64,
        button_h: f64,
        color: Color,
        enabled: bool,
    ) {
        if let Some(image) = &self.icon_image {
            let y = ((button_h - image.size().height) * 0.5).max(0.0);
            if !enabled {
                ctx.save();
                ctx.set_global_alpha(DISABLED_IMAGE_ALPHA);
            }
            image.draw(ctx, x, y);
            if !enabled {
                ctx.restore();
            }
            return;
        }
        Self::paint_glyph_icon(
            ctx,
            &self.icon,
            &self.font,
            self.font_size,
            x,
            button_h,
            color,
        );
    }

    /// Render the configured icon glyph centred vertically in the
    /// button using the glyph's *actual* outline bounding box — not
    /// the font's worst-case ascender/descender. Icon fonts (Font
    /// Awesome especially) place each glyph in a sub-rectangle of
    /// the design space; centring by the font metric leaves the glyph
    /// visibly high on the button (the "icons floating to the top"
    /// regression we've hit repeatedly). With the per-glyph bbox we
    /// solve for the baseline that puts the glyph's vertical midpoint
    /// at `button_h / 2`.
    fn paint_glyph_icon(
        ctx: &mut dyn DrawCtx,
        icon: &Option<ButtonIcon>,
        _label_font: &Arc<Font>,
        _label_font_size: f64,
        x: f64,
        button_h: f64,
        color: Color,
    ) {
        let Some(icon) = icon else { return };
        // (y_min, y_max) is the glyph's actual extent in pixels
        // relative to baseline, Y-up. y_min is usually negative
        // (descender region) or ~0, y_max is the cap-height of the
        // glyph. Pick the baseline so that
        //   baseline + (y_min + y_max) / 2  ==  button_h / 2
        // i.e. the glyph's midpoint sits at the button's midpoint.
        // Fall back to the font metric only if the glyph has no
        // outline (e.g. a space or a missing glyph).
        let baseline_y = match icon.font.glyph_visual_bounds(icon.glyph, icon.font_size) {
            Some((y_min, y_max)) => (button_h * 0.5 - (y_min + y_max) * 0.5).max(0.0),
            None => ((button_h - icon.font_size) * 0.5).max(0.0),
        };
        ctx.set_font(Arc::clone(&icon.font));
        ctx.set_font_size(icon.font_size);
        ctx.set_fill_color(color);
        ctx.fill_text(&icon.glyph.to_string(), x, baseline_y);
    }
}
