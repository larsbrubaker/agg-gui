//! Color type for agg-gui.
//!
//! Colors are stored as f32 RGBA in linear space. Conversion to AGG's `Rgba8`
//! happens at the rasterizer boundary.

use agg_rust::color::Rgba8;

/// An RGBA color with f32 components in [0.0, 1.0].
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// `Color` from 8-bit sRGB-style channels at full alpha.
    ///
    /// Each `u8` is divided by 255 into an f32 component. Convenient when
    /// transcribing CSS / SVG / Canvas color literals — `Color::from_rgb8(0, 255, 242)`
    /// instead of `Color::rgb(0.0, 1.0, 242.0 / 255.0)`.
    pub const fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }

    /// `Color` from 8-bit RGBA channels (each `u8` divided by 255).
    pub const fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    /// Per-channel linear interpolation toward `other` by `t` in `[0, 1]`.
    ///
    /// `t = 0` returns `self`; `t = 1` returns `other`. Values outside `[0, 1]`
    /// extrapolate (callers may want to clamp first).
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
        }
    }

    pub const fn white() -> Self {
        Self::rgb(1.0, 1.0, 1.0)
    }

    pub const fn black() -> Self {
        Self::rgb(0.0, 0.0, 0.0)
    }

    pub const fn transparent() -> Self {
        Self::rgba(0.0, 0.0, 0.0, 0.0)
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Convert to AGG's 8-bit RGBA format (used at the rasterizer boundary).
    pub(crate) fn to_rgba8(self) -> Rgba8 {
        Rgba8::new(
            (self.r * 255.0).clamp(0.0, 255.0) as u32,
            (self.g * 255.0).clamp(0.0, 255.0) as u32,
            (self.b * 255.0).clamp(0.0, 255.0) as u32,
            (self.a * 255.0).clamp(0.0, 255.0) as u32,
        )
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::black()
    }
}
