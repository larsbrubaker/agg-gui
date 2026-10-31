//! How many image pixels a picture spends on one logical point: its pixel
//! density, as the picture file itself says.
//!
//! A picture made on (or for) a high-density screen holds two pixels per
//! logical point in each direction, so a 1480-pixel-wide screenshot of a
//! 740-point window is meant to show 740 points wide. Drawn one pixel per
//! point it shows at twice its intended size, on every display. The file
//! says which it is in its PNG `pHYs` chunk: pixels per metre on each axis,
//! which [`density_from_pixels_per_metre`] turns into a whole density with the
//! rule below, and [`image_pixel_density`] reads from a file's bytes.
//!
//! **The rule.** A picture is 2x, 3x or 4x only when both axes say exactly that
//! screen density: within 1 ppi of 144, 216 or 288 ppi (72 points per inch
//! times the density; 144 ppi is 5669 px/m, the convention high-density
//! screenshots are saved with), the two axes agreeing. Everything else is 1:
//! no chunk, an unknown unit, 72 or 96 ppi (the common "ordinary screen"
//! values), and print resolutions such as 120, 150 or 300 dpi that an image
//! editor writes, which describe paper, not a screen, so such a picture keeps
//! showing one pixel per point rather than shrinking. Only PNG carries the
//! chunk here; every other format is 1.
//!
//! [`encode_png_rgba_at_density`] writes a PNG that says `density`, the
//! inverse of the rule, so a picture maker and the views that show its
//! pictures agree. The markdown view (`widgets/markdown/layout.rs`) and the
//! rich editor's image cache lay a picture out at its pixel size divided by
//! its density, then shrink it to fit as before.

use crate::screenshot::ScreenshotExportError;

/// The highest density a picture can say.
pub const MAX_IMAGE_DENSITY: u32 = 4;

/// How far, in pixels per inch, an axis may sit from a screen density's exact
/// value and still say it: room for the rounding of pixels per metre to a
/// whole number (144 ppi is 5669.29 px/m, stored as 5669), not for a
/// neighbouring print resolution.
const PPI_TOLERANCE: f64 = 1.0;

/// Points per inch: the logical unit a density of 1 maps one pixel onto.
const POINTS_PER_INCH: f64 = 72.0;

/// Metres per inch, for the `pHYs` chunk's pixels-per-metre unit.
const METRES_PER_INCH: f64 = 0.0254;

/// The density a `pHYs` chunk in metres says, from its pixels per metre on
/// the x and y axes, by the rule in the module header.
pub fn density_from_pixels_per_metre(x_per_metre: u32, y_per_metre: u32) -> u32 {
    let x_ppi = f64::from(x_per_metre) * METRES_PER_INCH;
    let y_ppi = f64::from(y_per_metre) * METRES_PER_INCH;
    (2..=MAX_IMAGE_DENSITY)
        .find(|&density| {
            let exact = f64::from(density) * POINTS_PER_INCH;
            (x_ppi - exact).abs() <= PPI_TOLERANCE && (y_ppi - exact).abs() <= PPI_TOLERANCE
        })
        .unwrap_or(1)
}

/// The pixels per metre a `pHYs` chunk holds for `density` (144 ppi for 2,
/// i.e. 5669 px/m): the value [`density_from_pixels_per_metre`] reads back as
/// `density`.
pub fn pixels_per_metre_for_density(density: u32) -> u32 {
    let ppi = f64::from(density.clamp(1, MAX_IMAGE_DENSITY)) * POINTS_PER_INCH;
    // At most 4 * 72 / 0.0254, about 11339: far inside u32.
    (ppi / METRES_PER_INCH).round() as u32
}

/// The density the picture file `bytes` says (see the module header): a PNG's
/// `pHYs` chunk in metres, else 1. Only the header and the chunks before the
/// first image data are read.
pub fn image_pixel_density(bytes: &[u8]) -> u32 {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return 1;
    }
    let Ok(reader) = png::Decoder::new(bytes).read_info() else {
        return 1;
    };
    match reader.info().pixel_dims {
        Some(png::PixelDimensions {
            xppu,
            yppu,
            unit: png::Unit::Meter,
        }) => density_from_pixels_per_metre(xppu, yppu),
        _ => 1,
    }
}

/// Encode a top-down RGBA8 image as a PNG whose `pHYs` chunk says `density`
/// (clamped to 1..=[`MAX_IMAGE_DENSITY`]); a density of 1 writes 72 ppi.
pub fn encode_png_rgba_at_density(
    rgba: &[u8],
    width: u32,
    height: u32,
    density: u32,
) -> Result<Vec<u8>, ScreenshotExportError> {
    let expected = width as usize * height as usize * 4;
    if rgba.len() != expected {
        return Err(ScreenshotExportError::InvalidBuffer {
            expected,
            actual: rgba.len(),
        });
    }
    let per_metre = pixels_per_metre_for_density(density);
    let mut out = Vec::with_capacity(rgba.len() / 2);
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_pixel_dims(Some(png::PixelDimensions {
            xppu: per_metre,
            yppu: per_metre,
            unit: png::Unit::Meter,
        }));
        let mut writer = encoder
            .write_header()
            .map_err(|e| ScreenshotExportError::Encode(e.to_string()))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| ScreenshotExportError::Encode(e.to_string()))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
