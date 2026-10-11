//! WCAG contrast helpers on [`Color`] — a port of agg-sharp's
//! `ColorExtensionMethods` (`agg/Primitives/Color.cs`): `WithContrast`,
//! `Contrast`, `Luminance0To1`, `OverlayOn` and `AdjustLightness`, with the
//! HSL conversions they go through (`ColorF.GetHSL` / `ColorF.FromHSL`,
//! `agg/Primitives/ColorF.cs`).
//!
//! agg-sharp's `IColorType` is implemented by the 8-bit `Color` and the float
//! `ColorF`; agg-gui's [`Color`] is float, so these read the channels as
//! they are.  `WithContrast` returns a C# `Color` (each step goes through
//! `ColorF.ToColor()`), so [`Color::with_contrast`] quantises every step to
//! 8-bit channels the same way, and lands on the same colour as C#.
//!
//! C# bugs not ported (each pinned by a `rust_only_*` test):
//! - `OverlayOn` assigned the boxed interface (`var overlaid = thisA`) and
//!   mutated it, so in `Contrast`'s translucent branch `thisA.OverlayOn(Black)`
//!   overwrote `thisA` itself and `onWhite` became the colour over black
//!   again.  Here colours are values: the colour over white is over white.
//! - `AdjustLightness` rebuilt the colour with `FromHSL`'s default alpha of 1,
//!   dropping the alpha.  Here the alpha is kept.
//!
//! [`super::Color`]'s other methods live in `color.rs`.

use super::Color;

/// C# `Math.Round(value, digits)` (round half to even after scaling by
/// 10^digits).  Hand-written because `f64::round_ties_even` is newer than
/// agg-gui's MSRV.
fn round_digits(value: f64, digits: i32) -> f64 {
    let power10 = 10f64.powi(digits);
    let scaled = value * power10;
    let rounded = scaled.round();
    let even = if (scaled - scaled.trunc()).abs() == 0.5 {
        2.0 * (scaled / 2.0).round()
    } else {
        rounded
    };
    even / power10
}

/// One sRGB channel linearised for luminance (WCAG 2.0).
fn linear_channel(c: f64) -> f64 {
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// C# `ColorF.Red0To255` and friends (`uround(clamp(c) * 255f)`), then the
/// `Color(int, int, int, int)` constructor's clamp to a byte.
fn channel_to_byte(c: f32) -> u8 {
    let scaled = (c.clamp(0.0, 255.0) * 255.0f32) as f64;
    // C# `(int)(uint)(v + 0.5)`; v is never negative here.
    let rounded = (scaled + 0.5) as u32;
    rounded.min(255) as u8
}

impl Color {
    /// This colour quantised to 8-bit channels, as C# `ColorF.ToColor()`
    /// makes a `Color`.
    pub fn quantized_to_rgba8(self) -> Color {
        Color::from_rgba8(
            channel_to_byte(self.r),
            channel_to_byte(self.g),
            channel_to_byte(self.b),
            channel_to_byte(self.a),
        )
    }

    /// Relative luminance, 0 to 1 (C# `Luminance0To1`, WCAG 2.0).
    pub fn luminance(self) -> f64 {
        let r = linear_channel(self.r as f64);
        let g = linear_channel(self.g as f64);
        let b = linear_channel(self.b as f64);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    /// This colour composited over `under` (C# `OverlayOn`); an opaque colour
    /// is returned unchanged.
    pub fn overlay_on(self, under: Color) -> Color {
        let alpha = self.a;
        if alpha >= 1.0 {
            return self;
        }

        Color {
            r: self.r * alpha + under.r * under.a * (1.0 - alpha),
            g: self.g * alpha + under.g * under.a * (1.0 - alpha),
            b: self.b * alpha + under.b * under.a * (1.0 - alpha),
            a: alpha + under.a * (1.0 - alpha),
        }
    }

    /// The WCAG contrast ratio of this colour against `other`, rounded to one
    /// decimal place, from 1 to 21 (C# `Contrast`).  A translucent `other` is
    /// composited over this colour first; when this colour is itself
    /// translucent the result is the middle of the range it could show over
    /// black or white, rounded to two places.
    pub fn contrast(self, other: Color) -> f64 {
        // Formula: http://www.w3.org/TR/2008/REC-WCAG20-20081211/#contrast-ratiodef
        let alpha = self.a;
        if alpha >= 1.0 {
            let other = if other.a < 1.0 {
                other.overlay_on(self)
            } else {
                other
            };

            let l1 = self.luminance() + 0.05;
            let l2 = other.luminance() + 0.05;
            let mut ratio = l1 / l2;
            if l2 > l1 {
                ratio = 1.0 / ratio;
            }

            return round_digits(ratio, 1);
        }

        // A semi-transparent colour: the text colour may or may not be
        // semi-transparent, but that doesn't matter.
        let on_black = self.overlay_on(Color::black());
        let on_white = self.overlay_on(Color::white());
        let contrast_on_black = on_black.contrast(other);
        let contrast_on_white = on_white.contrast(other);
        let max = contrast_on_black.max(contrast_on_white);

        let mut min = 1.0;
        if on_black.luminance() > other.luminance() {
            min = contrast_on_black;
        } else if on_white.luminance() < other.luminance() {
            min = contrast_on_white;
        }

        round_digits((min + max) / 2.0, 2)
    }

    /// Hue, saturation and lightness, each 0 to 1 (C# `ColorF.GetHSL`).
    pub fn to_hsl(self) -> (f64, f64, f64) {
        let red = self.r as f64;
        let green = self.g as f64;
        let blue = self.b as f64;
        let max_rgb = red.max(green.max(blue));
        let min_rgb = red.min(green.min(blue));
        let delta = max_rgb - min_rgb;

        let lightness = (min_rgb + max_rgb) / 2.0;
        if lightness <= 0.0 {
            return (0.0, 0.0, lightness);
        }

        let mut saturation = delta;
        if saturation > 0.0 {
            saturation /= if lightness <= 0.5 {
                max_rgb + min_rgb
            } else {
                2.0 - max_rgb - min_rgb
            };
        } else {
            return (0.0, saturation, lightness);
        }

        let r2 = (max_rgb - red) / delta;
        let g2 = (max_rgb - green) / delta;
        let b2 = (max_rgb - blue) / delta;
        let hue = if red == max_rgb {
            if green == min_rgb {
                5.0 + b2
            } else {
                1.0 - g2
            }
        } else if green == max_rgb {
            if blue == min_rgb {
                1.0 + r2
            } else {
                3.0 - b2
            }
        } else if red == min_rgb {
            3.0 + g2
        } else {
            5.0 - r2
        };
        (hue / 6.0, saturation, lightness)
    }

    /// The colour at hue, saturation and lightness (each 0 to 1) and `alpha`
    /// (C# `ColorF.FromHSL`).  Lightness past 1 is not clamped, as in C#.
    pub fn from_hsl(hue: f64, saturation: f64, lightness: f64, alpha: f64) -> Color {
        let alpha = alpha.min(1.0);
        let (mut r, mut g, mut b) = (lightness, lightness, lightness);
        let v = if lightness <= 0.5 {
            lightness * (1.0 + saturation)
        } else {
            lightness + saturation - lightness * saturation
        };

        if v > 0.0 {
            let m = lightness + lightness - v;
            let sv = (v - m) / v;
            let hue = hue * 6.0;
            // C# `(int)hue0To1` truncates.
            let sextant = hue as i32;
            let fract = hue - sextant as f64;
            let vsf = v * sv * fract;
            let mid1 = m + vsf;
            let mid2 = v - vsf;
            match sextant {
                0 | 6 => (r, g, b) = (v, mid1, m),
                1 => (r, g, b) = (mid2, v, m),
                2 => (r, g, b) = (m, v, mid1),
                3 => (r, g, b) = (m, mid2, v),
                4 => (r, g, b) = (mid1, m, v),
                5 => (r, g, b) = (v, m, mid2),
                _ => {}
            }
        }

        Color::rgba(r as f32, g as f32, b as f32, alpha as f32)
    }

    /// This colour with its HSL lightness multiplied by `multiplier` (C#
    /// `AdjustLightness`), keeping its alpha.
    pub fn adjust_lightness(self, multiplier: f64) -> Color {
        let (hue, saturation, lightness) = self.to_hsl();
        Color::from_hsl(hue, saturation, lightness * multiplier, self.a as f64)
    }

    /// This colour lightened (on a dark `fixed` colour) or darkened (on a
    /// light one) 5% at a time, at most 30 times, until its
    /// [`contrast`](Self::contrast) with `fixed` reaches
    /// `minimum_required_contrast` (C# `WithContrast`; C#'s default minimum
    /// is 3).  Each step is quantised to 8-bit channels as C#'s `ToColor()`.
    pub fn with_contrast(self, fixed: Color, minimum_required_contrast: f64) -> Color {
        let mut adjusted = self;
        let mut contrast = adjusted.contrast(fixed);
        let mut tries = 0;
        while contrast < minimum_required_contrast && tries < 30 {
            tries += 1;
            let multiplier = if fixed.luminance() < 0.5 { 1.05 } else { 0.95 };
            adjusted = adjusted.adjust_lightness(multiplier).quantized_to_rgba8();
            contrast = adjusted.contrast(fixed);
        }

        adjusted
    }
}

#[cfg(test)]
#[path = "contrast_tests.rs"]
mod tests;
