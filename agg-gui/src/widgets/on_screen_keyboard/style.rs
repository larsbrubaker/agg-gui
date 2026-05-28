//! Visual styling — colors, radii, spacings — keyed by the user's input
//! profile (iOS / Android / generic mobile).
//!
//! Kept as a flat data struct so the layout engine never branches on the
//! profile — it just reads tokens off the [`Style`]. Future themes
//! (dark mode, high-contrast, brand variants) plug in here.

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
    /// Font size for utility key labels ("space", "return", "ABC", "123").
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
    /// Pick the style appropriate for the active input profile.
    pub fn for_profile(profile: InputProfile) -> Self {
        match profile {
            InputProfile::MobileIOS => ios(),
            InputProfile::MobileAndroid => android(),
            // Desktop never paints the keyboard, but fall through to
            // a neutral default in case the host enabled it anyway
            // (e.g. an unrecognised tablet, or testing on desktop).
            InputProfile::Desktop | InputProfile::MobileOther => neutral(),
        }
    }
}

fn ios() -> Style {
    Style {
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
    }
}

fn android() -> Style {
    Style {
        panel_bg: Color::from_rgb8(0x20, 0x20, 0x24), // Material dark surface
        panel_top_border: Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x14),
        panel_padding_horizontal: 4.0,
        panel_padding_top: 6.0,
        panel_padding_bottom: 14.0,
        row_height: 48.0,
        key_h_gap: 4.0,
        key_v_gap: 6.0,
        key_corner_radius: 6.0,
        key_face_bg: Color::from_rgb8(0x2C, 0x2C, 0x32), // Material slightly raised
        key_face_bg_pressed: Color::from_rgb8(0x43, 0x47, 0x55),
        key_face_text: Color::from_rgb8(0xE8, 0xEA, 0xF0),
        key_face_text_pressed: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        key_shadow: Color::from_rgba8(0, 0, 0, 0x60),
        key_shadow_offset_y: -1.0,
        letter_font_size: 20.0,
        utility_font_size: 14.0,
        util_key_bg: Color::from_rgb8(0x1A, 0x1A, 0x1E),
        util_key_bg_pressed: Color::from_rgb8(0x33, 0x36, 0x3E),
        util_key_text: Color::from_rgb8(0xC4, 0xCB, 0xDB),
        return_key_bg: Color::from_rgb8(0x1A, 0x73, 0xE8), // Material blue 600
        return_key_text: Color::from_rgb8(0xFF, 0xFF, 0xFF),
        return_key_bg_pressed: Color::from_rgb8(0x12, 0x5A, 0xC0),
    }
}

fn neutral() -> Style {
    Style {
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
    }
}
