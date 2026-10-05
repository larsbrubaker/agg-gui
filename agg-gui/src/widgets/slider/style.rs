//! Per-instance styling for [`super::Slider`].
//!
//! By default a `Slider` paints its rail from `Visuals::track_bg` with a
//! fully-rounded (pill) end cap and its thumb from the accent colours.  Apps
//! that theme individual controls attach a [`SliderStyle`] via
//! [`super::Slider::with_style`]; every field is optional and `None` keeps
//! the default, so an all-`None` style paints exactly like an unstyled
//! slider.  The painters in `paint.rs` resolve these overrides.

use super::*;
use crate::color::Color;

/// Optional per-instance overrides for a [`Slider`].
///
/// `track` → `Visuals::track_bg`, `track_radius` → half the track
/// thickness (pill ends), `thumb` → `Visuals::accent` /
/// `accent_hovered` / `accent_pressed` by interaction state.  A `thumb`
/// override is used for every interaction state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SliderStyle {
    /// Rail (background track) colour.
    pub track: Option<Color>,
    /// Corner radius of the rail and the trailing fill.
    pub track_radius: Option<f64>,
    /// Handle colour (the ring of a circle handle, or the whole rect handle).
    pub thumb: Option<Color>,
}

impl Slider {
    /// Apply per-instance styling.  See [`SliderStyle`].
    pub fn with_style(mut self, style: SliderStyle) -> Self {
        self.style = style;
        self
    }

    /// The current per-instance style overrides.
    pub fn style(&self) -> SliderStyle {
        self.style
    }

    /// Resolved rail corner radius.
    pub(super) fn track_radius(&self) -> f64 {
        self.style.track_radius.unwrap_or(TRACK_H * 0.5).max(0.0)
    }
}
