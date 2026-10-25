//! Visual styling — colors, radii, spacings — keyed by the user's input
//! profile (iOS / Android / generic mobile) and the active app theme
//! (light / dark, read from [`crate::theme::current_visuals`]).
//!
//! Kept as a flat data struct so the layout engine never branches on the
//! profile — it just reads tokens off the [`Style`]. Each profile ships
//! a light and a dark palette over shared metrics; the Android palettes
//! are matched to the Gboard screenshots in `docs/Android Keyboard/`.

use crate::color::Color;
use crate::input_profile::InputProfile;

/// Tokens consumed by the keyboard painters.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    // ── Panel
    /// Color behind all keys.
    pub panel_bg: Color,
    /// Hairline at the top edge of the panel.
    pub panel_top_border: Color,
    /// Padding around the key grid.
    pub panel_padding_horizontal: f64,
    pub panel_padding_top: f64,
    pub panel_padding_bottom: f64,
    /// Total height target per row of keys (logical pixels).
    pub row_height: f64,
    /// Gap between adjacent keys in a row.
    pub key_h_gap: f64,
    /// Gap between rows.
    pub key_v_gap: f64,
    /// Key corner radius.
    pub key_corner_radius: f64,
    /// Height of the strip below the keys that holds the hide-keyboard
    /// chevron (where the phone shows its own hide control).
    pub bottom_strip_height: f64,
    /// Height of the toolbar row above the keys that holds the microphone
    /// key while a voice provider is registered (Gboard's toolbar; `0.0`
    /// where the platform puts the microphone elsewhere, as the iPhone's
    /// strip below the keys). See `voice_paint.rs`.
    pub toolbar_height: f64,
    /// Diameter of the microphone key's round face in the toolbar.
    pub voice_key_diameter: f64,

    // ── Standard key
    pub key_face_bg: Color,
    pub key_face_bg_pressed: Color,
    pub key_face_text: Color,
    pub key_face_text_pressed: Color,
    pub key_shadow: Color,
    pub key_shadow_offset_y: f64,
    /// Font size for letter caps (digits + symbols inherit this).
    pub letter_font_size: f64,
    /// Font size for utility key labels ("ABC", "?123", "=\\<").
    pub utility_font_size: f64,

    // ── Utility key (shift / backspace / mode / return)
    pub util_key_bg: Color,
    pub util_key_bg_pressed: Color,
    pub util_key_text: Color,
    /// Accent applied to the Return key (matches platform send-button color).
    pub return_key_bg: Color,
    pub return_key_text: Color,
    pub return_key_bg_pressed: Color,
    /// Face of the preview bubble shown over a pressed letter (and of the
    /// long-press alternates popup).
    pub preview_bg: Color,
    /// One ring of the soft shadow under the Gboard preview bubble; the
    /// rings overlap, so it darkens towards the bubble's edge.
    pub preview_shadow: Color,
    /// The highlighted alternate's fill in the long-press popup, and its
    /// letter.
    pub alt_highlight_bg: Color,
    pub alt_highlight_text: Color,
    /// Laid over the keys while the alternates popup is open (Gboard dims
    /// the rest of the keyboard; transparent where the platform doesn't).
    pub alt_dim: Color,
}

impl Style {
    /// Pick the style appropriate for the active input profile and the
    /// app theme currently installed via [`crate::theme::set_visuals`].
    ///
    /// The metrics are the platform keyboard's own, in dp / pt (CSS
    /// pixels): the keyboard draws at the phone keyboard's size whatever
    /// UX zoom ([`crate::ux_scale`]) the app's widgets use, so they are
    /// divided by the zoom here, and the paint's zoom scales them back.
    pub fn for_profile(profile: InputProfile) -> Self {
        Self::for_profile_unzoomed(profile).unzoomed(crate::ux_scale::ux_scale())
    }

    /// The metrics divided by the UX `zoom`, so that drawn at that zoom
    /// they are the platform's sizes.
    fn unzoomed(self, zoom: f64) -> Self {
        let k = if zoom > 1e-6 { 1.0 / zoom } else { 1.0 };
        Self {
            panel_padding_horizontal: self.panel_padding_horizontal * k,
            panel_padding_top: self.panel_padding_top * k,
            panel_padding_bottom: self.panel_padding_bottom * k,
            row_height: self.row_height * k,
            key_h_gap: self.key_h_gap * k,
            key_v_gap: self.key_v_gap * k,
            key_corner_radius: self.key_corner_radius * k,
            bottom_strip_height: self.bottom_strip_height * k,
            toolbar_height: self.toolbar_height * k,
            voice_key_diameter: self.voice_key_diameter * k,
            key_shadow_offset_y: self.key_shadow_offset_y * k,
            letter_font_size: self.letter_font_size * k,
            utility_font_size: self.utility_font_size * k,
            ..self
        }
    }

    /// [`Style::for_profile`] before the UX zoom is taken out.
    fn for_profile_unzoomed(profile: InputProfile) -> Self {
        let dark = crate::theme::current_visuals().is_dark();
        match profile {
            InputProfile::MobileIOS => ios(dark),
            InputProfile::MobileAndroid => android(dark),
            // Desktop never paints the keyboard, but fall through to
            // a neutral default in case the host enabled it anyway
            // (e.g. an unrecognised tablet, or testing on desktop).
            InputProfile::Desktop | InputProfile::MobileOther => neutral(dark),
        }
    }
}

fn ios(dark: bool) -> Style {
    let base = Style {
        // iPhone metrics (points, on a 390 pt wide phone): 3 pt side
        // margins, keys 33 pt wide on 6 pt gaps and 42 pt tall on a 54 pt
        // row pitch, 5 pt corners and a 1 pt shadow; below the keys, the
        // row that holds the emoji key. The home-indicator inset below
        // that is the host's (Safari's own toolbar covers it in a browser).
        panel_bg: Color::from_rgb8(0xD1, 0xD5, 0xDB), // iOS light gray tray
        panel_top_border: Color::from_rgba8(0, 0, 0, 0x22),
        panel_padding_horizontal: 3.0,
        panel_padding_top: 8.0,
        panel_padding_bottom: 4.0,
        row_height: 42.0,
        key_h_gap: 6.0,
        key_v_gap: 12.0,
        key_corner_radius: 5.0,
        bottom_strip_height: 44.0,
        // The iPhone's microphone sits in the strip below the keys.
        toolbar_height: 0.0,
        voice_key_diameter: 0.0,
        key_face_bg: Color::from_rgb8(0xFE, 0xFE, 0xFE),
        key_face_bg_pressed: Color::from_rgb8(0xBC, 0xC0, 0xC9),
        key_face_text: Color::from_rgb8(0x10, 0x10, 0x18),
        key_face_text_pressed: Color::from_rgb8(0x10, 0x10, 0x18),
        key_shadow: Color::from_rgba8(0, 0, 0, 0x4D),
        key_shadow_offset_y: -1.0, // Y-up: shadow paints below = -Y
        letter_font_size: 23.0,
        utility_font_size: 16.0,
        util_key_bg: Color::from_rgb8(0xAB, 0xB0, 0xBC),
        util_key_bg_pressed: Color::from_rgb8(0xFE, 0xFE, 0xFE),
        util_key_text: Color::from_rgb8(0x10, 0x10, 0x18),
        return_key_bg: Color::from_rgb8(0x00, 0x7A, 0xFF), // iOS system blue
        return_key_text: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        return_key_bg_pressed: Color::from_rgb8(0x00, 0x57, 0xBE),
        preview_bg: Color::from_rgb8(0xFE, 0xFE, 0xFE),
        preview_shadow: Color::from_rgba8(0, 0, 0, 0x10),
        alt_highlight_bg: Color::from_rgb8(0x00, 0x7A, 0xFF),
        alt_highlight_text: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        alt_dim: Color::from_rgba8(0, 0, 0, 0),
    };
    if !dark {
        return base;
    }
    Style {
        panel_bg: Color::from_rgb8(0x2C, 0x2C, 0x2E), // iOS dark tray
        panel_top_border: Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x1E),
        key_face_bg: Color::from_rgb8(0x68, 0x68, 0x6B),
        key_face_bg_pressed: Color::from_rgb8(0x8E, 0x8E, 0x93),
        key_face_text: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        key_face_text_pressed: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        key_shadow: Color::from_rgba8(0, 0, 0, 0x66),
        util_key_bg: Color::from_rgb8(0x46, 0x46, 0x4A),
        util_key_bg_pressed: Color::from_rgb8(0x68, 0x68, 0x6B),
        util_key_text: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        return_key_bg: Color::from_rgb8(0x0A, 0x84, 0xFF), // iOS dark system blue
        return_key_bg_pressed: Color::from_rgb8(0x06, 0x5F, 0xBD),
        preview_bg: Color::from_rgb8(0x68, 0x68, 0x6B),
        alt_highlight_bg: Color::from_rgb8(0x0A, 0x84, 0xFF),
        ..base
    }
}

fn android(dark: bool) -> Style {
    // Light palette matched to the Gboard screenshots — a Material You
    // "dynamic color" theme: peach tray, white letter keys, light-blue
    // utility keys, flat (no key shadow). Metrics measured off a Gboard
    // screenshot on a 411 dp wide phone: keys 32.5 dp wide with 6 dp
    // gaps, 15 dp side margins, 45 dp key height on a 54.5 dp row pitch,
    // gently rounded (about 6 dp) keys.
    let base = Style {
        panel_bg: Color::from_rgb8(0xFB, 0xEA, 0xDF),
        panel_top_border: Color::from_rgba8(0x00, 0x00, 0x00, 0x10),
        panel_padding_horizontal: 15.0,
        panel_padding_top: 10.0,
        panel_padding_bottom: 6.0,
        row_height: 45.0,
        key_h_gap: 6.0,
        key_v_gap: 9.5,
        key_corner_radius: 6.0,
        bottom_strip_height: 32.0,
        // Gboard's toolbar: 43 dp from the panel's top to the keys' top
        // padding, its microphone a 34 dp circle (measured off
        // `standard.png`).
        toolbar_height: 43.0,
        voice_key_diameter: 34.0,
        key_face_bg: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        key_face_bg_pressed: Color::from_rgb8(0xE3, 0xCF, 0xC2),
        key_face_text: Color::from_rgb8(0x4C, 0x39, 0x28),
        key_face_text_pressed: Color::from_rgb8(0x4C, 0x39, 0x28),
        key_shadow: Color::from_rgba8(0, 0, 0, 0x00), // Gboard keys are flat
        key_shadow_offset_y: 0.0,
        letter_font_size: 24.0,
        utility_font_size: 16.0,
        util_key_bg: Color::from_rgb8(0xC8, 0xDD, 0xF5),
        util_key_bg_pressed: Color::from_rgb8(0xA3, 0xC2, 0xE8),
        util_key_text: Color::from_rgb8(0x3F, 0x48, 0x54),
        return_key_bg: Color::from_rgb8(0xBC, 0xD5, 0xF2),
        return_key_text: Color::from_rgb8(0x3F, 0x48, 0x54),
        return_key_bg_pressed: Color::from_rgb8(0x93, 0xB8, 0xE4),
        preview_bg: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        preview_shadow: Color::from_rgba8(0, 0, 0, 0x10),
        // The dynamic theme's primary, as on the phone's popup.
        alt_highlight_bg: Color::from_rgb8(0x8D, 0x4E, 0x2A),
        alt_highlight_text: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        alt_dim: Color::from_rgba8(0xFB, 0xEA, 0xDF, 0x99),
    };
    if !dark {
        return base;
    }
    // Dark counterpart of the same dynamic-color scheme: warm dark
    // tray, slate-blue utility keys, pale-blue enter with dark glyph
    // (Material You keeps the accent light in dark mode).
    Style {
        panel_bg: Color::from_rgb8(0x21, 0x1E, 0x1C),
        panel_top_border: Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x14),
        key_face_bg: Color::from_rgb8(0x39, 0x34, 0x30),
        key_face_bg_pressed: Color::from_rgb8(0x55, 0x4E, 0x48),
        key_face_text: Color::from_rgb8(0xEC, 0xE3, 0xDC),
        key_face_text_pressed: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        util_key_bg: Color::from_rgb8(0x41, 0x4A, 0x58),
        util_key_bg_pressed: Color::from_rgb8(0x59, 0x66, 0x79),
        util_key_text: Color::from_rgb8(0xD5, 0xE0, 0xF0),
        return_key_bg: Color::from_rgb8(0xA8, 0xC8, 0xF0),
        return_key_text: Color::from_rgb8(0x20, 0x2A, 0x38),
        return_key_bg_pressed: Color::from_rgb8(0x84, 0xAC, 0xDE),
        preview_bg: Color::from_rgb8(0x4A, 0x44, 0x3F),
        preview_shadow: Color::from_rgba8(0, 0, 0, 0x30),
        alt_highlight_bg: Color::from_rgb8(0xFF, 0xB6, 0x8F),
        alt_highlight_text: Color::from_rgb8(0x3A, 0x1A, 0x08),
        alt_dim: Color::from_rgba8(0x21, 0x1E, 0x1C, 0x99),
        ..base
    }
}

fn neutral(dark: bool) -> Style {
    let base = Style {
        panel_bg: Color::from_rgb8(0x18, 0x18, 0x22),
        panel_top_border: Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x33),
        panel_padding_horizontal: 4.0,
        panel_padding_top: 8.0,
        panel_padding_bottom: 12.0,
        row_height: 46.0,
        key_h_gap: 5.0,
        key_v_gap: 8.0,
        key_corner_radius: 6.0,
        bottom_strip_height: 30.0,
        toolbar_height: 40.0,
        voice_key_diameter: 32.0,
        key_face_bg: Color::from_rgb8(0x2A, 0x2B, 0x36),
        key_face_bg_pressed: Color::from_rgb8(0x44, 0x46, 0x55),
        key_face_text: Color::from_rgb8(0xE7, 0xE8, 0xF0),
        key_face_text_pressed: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        key_shadow: Color::from_rgba8(0, 0, 0, 0x55),
        key_shadow_offset_y: -1.0,
        letter_font_size: 20.0,
        utility_font_size: 14.0,
        util_key_bg: Color::from_rgb8(0x1F, 0x20, 0x2A),
        util_key_bg_pressed: Color::from_rgb8(0x33, 0x36, 0x44),
        util_key_text: Color::from_rgb8(0xC4, 0xC7, 0xD5),
        return_key_bg: Color::from_rgb8(0x3B, 0x82, 0xF6),
        return_key_text: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        return_key_bg_pressed: Color::from_rgb8(0x2B, 0x66, 0xD0),
        preview_bg: Color::from_rgb8(0x44, 0x46, 0x55),
        preview_shadow: Color::from_rgba8(0, 0, 0, 0x30),
        alt_highlight_bg: Color::from_rgb8(0x3B, 0x82, 0xF6),
        alt_highlight_text: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        alt_dim: Color::from_rgba8(0x18, 0x18, 0x22, 0x99),
    };
    if dark {
        return base;
    }
    Style {
        panel_bg: Color::from_rgb8(0xE6, 0xE8, 0xEE),
        panel_top_border: Color::from_rgba8(0x00, 0x00, 0x00, 0x22),
        key_face_bg: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        key_face_bg_pressed: Color::from_rgb8(0xC5, 0xC9, 0xD4),
        key_face_text: Color::from_rgb8(0x14, 0x15, 0x1C),
        key_face_text_pressed: Color::from_rgb8(0x14, 0x15, 0x1C),
        key_shadow: Color::from_rgba8(0, 0, 0, 0x33),
        util_key_bg: Color::from_rgb8(0xC9, 0xCC, 0xD6),
        util_key_bg_pressed: Color::from_rgb8(0xAF, 0xB3, 0xC0),
        util_key_text: Color::from_rgb8(0x14, 0x15, 0x1C),
        preview_bg: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        preview_shadow: Color::from_rgba8(0, 0, 0, 0x10),
        alt_dim: Color::from_rgba8(0xE6, 0xE8, 0xEE, 0x99),
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{set_visuals, Visuals};

    fn luminance(c: Color) -> f32 {
        0.299 * c.r + 0.587 * c.g + 0.114 * c.b
    }

    #[test]
    fn keyboard_style_follows_app_theme() {
        set_visuals(Visuals::light());
        let light = Style::for_profile(InputProfile::MobileAndroid);
        set_visuals(Visuals::dark());
        let dark = Style::for_profile(InputProfile::MobileAndroid);

        assert!(
            luminance(light.panel_bg) > 0.5,
            "light theme must produce a light keyboard tray"
        );
        assert!(
            luminance(dark.panel_bg) < 0.5,
            "dark theme must produce a dark keyboard tray"
        );
        assert!(
            luminance(dark.key_face_text) > luminance(light.key_face_text),
            "text must invert with the theme"
        );
    }

    #[test]
    fn every_profile_has_light_and_dark_variants() {
        for profile in [
            InputProfile::MobileIOS,
            InputProfile::MobileAndroid,
            InputProfile::MobileOther,
        ] {
            set_visuals(Visuals::light());
            let light = Style::for_profile(profile);
            set_visuals(Visuals::dark());
            let dark = Style::for_profile(profile);
            assert!(
                luminance(light.panel_bg) > luminance(dark.panel_bg),
                "{profile:?}: light tray must be brighter than dark tray"
            );
            // Metrics are theme-independent — only colors flip.
            assert_eq!(light.row_height, dark.row_height);
            assert_eq!(light.key_corner_radius, dark.key_corner_radius);
        }
    }
}
