//! Leading-icon support for [`super::Button`]: the [`ButtonIcon`] glyph
//! type, the `with_icon*` / `with_image_icon` builders, the width the icon
//! reserves in layout, and its painting.  Split out of `button.rs` (pulled in
//! via `#[path]` as a child module, like `button_events.rs`) so the parent
//! stays under the 800-line cap and keeps private-field access.
//!
//! A button shows at most one leading icon: an [`IconImage`] when one is set
//! (artwork icons, drawn at device resolution), otherwise the glyph.

use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::{Rect, Size};
use crate::icon_image::IconImage;
use crate::text::{measure_advance, measure_text_metrics, Font};

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

    /// Places a glyph icon in its font's em box instead of centring the
    /// glyph's own outline: the box is the glyph's advance wide and the
    /// face's ascent to descent tall, centred on the button, with its left
    /// edge and baseline on whole device pixels (MarkdigAgg's
    /// `MarkdownFormatButton.EmBox`).  The baseline is the same for every
    /// glyph of the face, so a row of icon-only buttons reads as one line.
    /// Meant for icon-only buttons; image icons are unaffected.
    pub fn with_icon_em_box(mut self) -> Self {
        self.icon_em_box = true;
        self
    }

    /// Draws the icon with `paint` instead of a glyph or an image: it gets the
    /// context in the button's local space, the button's size and the icon
    /// colour, and places the icon itself (so it can snap to the device grid
    /// at any scale).  Meant for icon-only buttons, where it takes the whole
    /// button; the painter takes precedence over every other icon.
    pub fn with_icon_painter(
        mut self,
        paint: impl Fn(&mut dyn DrawCtx, Size, Color) + 'static,
    ) -> Self {
        self.icon_painter = Some(Rc::new(paint));
        self
    }

    /// Colours the glyph icon of a [`with_flat`](Self::with_flat) button: `f`
    /// gets the current visuals on every paint and returns the colour, or
    /// `None` for the theme's text colour.  Lets an owner that paints its
    /// own states tint the icon (a pressed style's deeper accent, a disabled
    /// fade) without the button's own background logic.
    pub fn with_icon_color_fn(
        mut self,
        f: impl Fn(&crate::theme::Visuals) -> Option<Color> + 'static,
    ) -> Self {
        self.icon_color_fn = Some(Rc::new(f));
        self
    }

    /// A button that paints only its icon: no fill, outline, hover or press
    /// shade, no focus ring and no disabled cover.  Its owner draws the
    /// background and reads [`is_hovered`](Self::is_hovered) /
    /// [`is_pressed`](Self::is_pressed) to shade it, which is how a toolbar
    /// of grouped buttons gives its end cells the group's rounded corners.
    pub fn with_flat(mut self) -> Self {
        self.flat = true;
        self
    }

    /// Whether the pointer is over the button.
    pub fn is_hovered(&self) -> bool {
        self.hovered
    }

    /// Whether the button is held down by the pointer.
    pub fn is_pressed(&self) -> bool {
        self.pressed
    }

    /// The glyph icon's em box in the button's local Y-up coordinates: the
    /// glyph's advance wide, the face's ascent to descent tall, standing on
    /// the baseline the icon is drawn on (C#'s `IconEmBox`).  `None` without
    /// a glyph icon, or when an image icon replaces it.
    pub fn icon_em_box(&self) -> Option<Rect> {
        if self.icon_image.is_some() {
            return None;
        }
        let icon = self.icon.as_ref()?;
        let (w, h) = (self.bounds.width, self.bounds.height);
        let (left, baseline) = self.glyph_origin(icon, self.icon_left_x(), w, h);
        let metrics = measure_text_metrics(&icon.font, "", icon.font_size);
        let advance = measure_advance(&icon.font, &icon.glyph.to_string(), icon.font_size);
        Some(Rect::new(
            left,
            baseline - metrics.descent,
            advance,
            metrics.ascent + metrics.descent,
        ))
    }

    /// Where the leading icon's left edge sits: left of the label child by
    /// the icon block (as `paint` places it).
    pub(super) fn icon_left_x(&self) -> f64 {
        let label_x = self
            .children
            .first()
            .map(|c| c.bounds().x)
            .unwrap_or_default();
        (label_x - self.icon_block_w()).max(0.0)
    }

    /// The glyph icon's pen origin (left, baseline) for a button `w` x `h`
    /// whose icon block starts at `x`.  By default the glyph's own outline
    /// is centred vertically; in em-box mode the
    /// em box is centred both ways and snapped to whole device pixels with
    /// C#'s `Math.Round` (halves to even).
    fn glyph_origin(&self, icon: &ButtonIcon, x: f64, w: f64, h: f64) -> (f64, f64) {
        if !self.icon_em_box {
            // Centre the glyph using its *actual* outline bounding box, not
            // the font's worst-case ascender/descender.  Icon fonts (Font
            // Awesome especially) place each glyph in a sub-rectangle of the
            // design space; centring by the font metric leaves the glyph
            // visibly high on the button (the "icons floating to the top"
            // regression we've hit repeatedly).  (y_min, y_max) is the
            // glyph's extent relative to the baseline, Y-up; pick the
            // baseline so that baseline + (y_min + y_max) / 2 == h / 2.
            // Fall back to the font metric only if the glyph has no outline
            // (e.g. a space or a missing glyph).
            let baseline = match icon.font.glyph_visual_bounds(icon.glyph, icon.font_size) {
                Some((y_min, y_max)) => (h * 0.5 - (y_min + y_max) * 0.5).max(0.0),
                None => ((h - icon.font_size) * 0.5).max(0.0),
            };
            return (x, baseline);
        }
        let metrics = measure_text_metrics(&icon.font, "", icon.font_size);
        let advance = measure_advance(&icon.font, &icon.glyph.to_string(), icon.font_size);
        let descent = -metrics.descent;
        let height = metrics.ascent + metrics.descent;
        let scale = crate::ux_scale::effective_scale().max(f64::MIN_POSITIVE);
        let snap = |v: f64| round_half_even(v * scale) / scale;
        (
            snap(w * 0.5 - advance / 2.0),
            snap(h * 0.5 - (descent + height / 2.0)),
        )
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
        if self.icon_painter.is_some() {
            return 0.0;
        }
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
        if let Some(paint) = &self.icon_painter {
            let size = Size::new(self.bounds.width, button_h);
            if enabled {
                paint(ctx, size, color);
            } else {
                ctx.save();
                ctx.set_global_alpha(DISABLED_IMAGE_ALPHA);
                paint(ctx, size, color);
                ctx.restore();
            }
            return;
        }
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
        let Some(icon) = &self.icon else { return };
        let (left, baseline) = self.glyph_origin(icon, x, self.bounds.width, button_h);
        ctx.set_font(Arc::clone(&icon.font));
        ctx.set_font_size(icon.font_size);
        ctx.set_fill_color(color);
        if self.icon_em_box {
            // The em box is snapped to whole device pixels in the button's own
            // space; snapping the button's origin too keeps the glyph's left
            // edge and baseline on pixel boundaries wherever the button sits.
            ctx.save();
            ctx.snap_to_pixel();
            ctx.fill_text(&icon.glyph.to_string(), left, baseline);
            ctx.restore();
        } else {
            ctx.fill_text(&icon.glyph.to_string(), left, baseline);
        }
    }
}

/// C# `Math.Round`: halves round to the even neighbour (hand-written, as in
/// `grid_pixels`, because `f64::round_ties_even` is newer than agg-gui's
/// MSRV).
fn round_half_even(x: f64) -> f64 {
    let rounded = x.round();
    if (rounded - x).abs() == 0.5 {
        2.0 * (x / 2.0).round()
    } else {
        rounded
    }
}
