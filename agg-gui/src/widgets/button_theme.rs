//! Visual theme for [`super::Button`] — colours, corner radius, focus ring.
//!
//! Split out of `button.rs` so the parent stays under the 800-line cap.

use crate::color::Color;

/// A theme for [`super::Button`] visual states.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonTheme {
    pub background: Color,
    pub background_hovered: Color,
    pub background_pressed: Color,
    pub label_color: Color,
    pub border_radius: f64,
    pub focus_ring_color: Color,
    pub focus_ring_width: f64,
}

impl Default for ButtonTheme {
    fn default() -> Self {
        Self {
            background: Color::rgb(0.22, 0.45, 0.88),
            background_hovered: Color::rgb(0.30, 0.52, 0.92),
            background_pressed: Color::rgb(0.16, 0.36, 0.72),
            label_color: Color::white(),
            border_radius: 6.0,
            focus_ring_color: Color::rgba(0.22, 0.45, 0.88, 0.55),
            focus_ring_width: 2.5,
        }
    }
}
