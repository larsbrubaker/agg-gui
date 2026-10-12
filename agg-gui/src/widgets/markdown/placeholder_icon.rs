//! The icon a `MarkdownView` image shows until its picture arrives, from
//! MarkdigAgg's `Inlines/ImageLinkSimpleX.cs` (its globe,
//! `internet.svg`) and agg-sharp's `ImageProcessing/GrayToColorProcess.cs`.
//!
//! agg-gui ships no MatterCAD artwork, so the icon is a hook
//! ([`MarkdownView::with_link_placeholder_icon`]): an application passes
//! its own [`IconImage`] (MatterCAD its `internet.svg`).  Without one the
//! placeholder stays the shaded box with the alt text.  With one, the icon
//! draws at the start of that box, recoloured as C# does: the icon file's
//! gray ink becomes the theme's text colour (the colour of the link text
//! beside it), so a black icon stays visible on a dark theme.
//! `cell_images.rs` paints the placeholder, `layout.rs` sizes it.

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::icon_image::IconImage;

use super::MarkdownView;

/// Space either side of the placeholder icon inside the box.
pub(super) const ICON_GAP: f64 = 4.0;

impl MarkdownView {
    /// Shows `icon` in every image placeholder (C#'s `ImageLinkSimpleX`
    /// globe), drawn in the theme's text colour.  The icon's logical size
    /// is its draw size; C#'s globe is 16 x 16.
    pub fn with_link_placeholder_icon(mut self, icon: IconImage) -> Self {
        self.link_placeholder_icon = Some(icon);
        self
    }

    /// The width the placeholder icon adds to an image placeholder: the
    /// icon and the gap after it, or nothing without an icon.
    pub(super) fn placeholder_icon_room(&self) -> f64 {
        self.link_placeholder_icon
            .as_ref()
            .map_or(0.0, |icon| icon.size().width + ICON_GAP)
    }

    /// Draws the placeholder icon, if any, at the left of the placeholder
    /// box at `x`, `y` (bottom-left), `height` tall, vertically centred, in
    /// `color`.  Returns how far the label moves right to clear it.
    pub(super) fn paint_placeholder_icon(
        &self,
        ctx: &mut dyn DrawCtx,
        x: f64,
        y: f64,
        height: f64,
        color: Color,
    ) -> f64 {
        let Some(icon) = &self.link_placeholder_icon else {
            return 0.0;
        };
        let size = icon.size();
        let Some((raster, w, h)) = icon.raster_at_scale(crate::device_scale()) else {
            return 0.0;
        };
        let mut pixels = raster.as_ref().clone();
        gray_to_color(&mut pixels, color);
        ctx.draw_image_rgba(
            &pixels,
            w,
            h,
            x + ICON_GAP,
            y + (height - size.height) * 0.5,
            size.width,
            size.height,
        );
        self.placeholder_icon_room()
    }
}

/// C#'s `GrayToColor(color, DestIntensity.FromColor)` over straight-alpha
/// RGBA8: every gray pixel (HSL saturation under 1%), black and white
/// included, takes `color`'s red, green and blue; coloured pixels keep
/// theirs; every alpha is kept, and a fully transparent pixel is cleared.
pub(super) fn gray_to_color(rgba: &mut [u8], color: Color) {
    let to_byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (red, green, blue) = (to_byte(color.r), to_byte(color.g), to_byte(color.b));
    for pixel in rgba.chunks_exact_mut(4) {
        if pixel[3] == 0 {
            pixel[..3].fill(0);
        } else if hsl_saturation(pixel[0], pixel[1], pixel[2]) < 0.01 {
            pixel[0] = red;
            pixel[1] = green;
            pixel[2] = blue;
        }
    }
}

/// HSL saturation of an 8-bit colour (C#'s `ColorF.GetHSL`).
fn hsl_saturation(r: u8, g: u8, b: u8) -> f64 {
    let (r, g, b) = (
        f64::from(r) / 255.0,
        f64::from(g) / 255.0,
        f64::from(b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    if delta == 0.0 {
        0.0
    } else if lightness <= 0.5 {
        delta / (max + min)
    } else {
        delta / (2.0 - max - min)
    }
}
