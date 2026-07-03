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
}

impl Style {
    /// Pick the style appropriate for the active input profile and the
    /// app theme currently installed via [`crate::theme::set_visuals`].
    pub fn for_profile(profile: InputProfile) -> Self {
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
        panel_bg: Color::from_rgb8(0xD1, 0xD5, 0xDB), // iOS light gray tray
        panel_top_border: Color::from_rgba8(0, 0, 0, 0x33),
        panel_padding_horizontal: 4.0,
        panel_padding_top: 8.0,
        panel_padding_bottom: 18.0, // home-indicator safe area
        row_height: 44.0,
        key_h_gap: 6.0,
        key_v_gap: 10.0,
        key_corner_radius: 5.0,
        key_face_bg: Color::from_rgb8(0xFE, 0xFE, 0xFE),
        key_face_bg_pressed: Color::from_rgb8(0xBC, 0xC0, 0xC9),
        key_face_text: Color::from_rgb8(0x10, 0x10, 0x18),
        key_face_text_pressed: Color::from_rgb8(0x10, 0x10, 0x18),
        key_shadow: Color::from_rgba8(0, 0, 0, 0x4D),
        key_shadow_offset_y: -1.0, // Y-up: shadow paints below = -Y
        letter_font_size: 22.0,
        utility_font_size: 15.0,
        util_key_bg: Color::from_rgb8(0xAB, 0xB0, 0xBC),
        util_key_bg_pressed: Color::from_rgb8(0xFE, 0xFE, 0xFE),
        util_key_text: Color::from_rgb8(0x10, 0x10, 0x18),
        return_key_bg: Color::from_rgb8(0x00, 0x7A, 0xFF), // iOS system blue
        return_key_text: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        return_key_bg_pressed: Color::from_rgb8(0x00, 0x57, 0xBE),
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
        ..base
    }
}

fn android(dark: bool) -> Style {
    // Light palette matched to the Gboard screenshots — a Material You
    // "dynamic color" theme: peach tray, white letter keys, light-blue
    // utility keys, flat (no key shadow).
    let base = Style {
        panel_bg: Color::from_rgb8(0xFB, 0xEA, 0xDF),
        panel_top_border: Color::from_rgba8(0x00, 0x00, 0x00, 0x10),
        panel_padding_horizontal: 4.0,
        panel_padding_top: 10.0,
        panel_padding_bottom: 14.0,
        row_height: 48.0,
        key_h_gap: 6.0,
        key_v_gap: 9.0,
        key_corner_radius: 10.0,
        key_face_bg: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        key_face_bg_pressed: Color::from_rgb8(0xE3, 0xCF, 0xC2),
        key_face_text: Color::from_rgb8(0x4C, 0x39, 0x28),
        key_face_text_pressed: Color::from_rgb8(0x4C, 0x39, 0x28),
        key_shadow: Color::from_rgba8(0, 0, 0, 0x00), // Gboard keys are flat
        key_shadow_offset_y: 0.0,
        letter_font_size: 22.0,
        utility_font_size: 16.0,
        util_key_bg: Color::from_rgb8(0xC8, 0xDD, 0xF5),
        util_key_bg_pressed: Color::from_rgb8(0xA3, 0xC2, 0xE8),
        util_key_text: Color::from_rgb8(0x3F, 0x48, 0x54),
        return_key_bg: Color::from_rgb8(0xBC, 0xD5, 0xF2),
        return_key_text: Color::from_rgb8(0x3F, 0x48, 0x54),
        return_key_bg_pressed: Color::from_rgb8(0x93, 0xB8, 0xE4),
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
