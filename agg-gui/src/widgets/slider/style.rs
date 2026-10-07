//! Per-instance styling for [`super::Slider`].
//!
//! By default a `Slider` paints its rail from `Visuals::track_bg` with a
//! fully-rounded (pill) end cap and its thumb from the accent colours.  Apps
//! that theme individual controls attach a [`SliderStyle`] via
//! [`super::Slider::with_style`]; every field is optional and `None` keeps
//! the default, so an all-`None` style paints exactly like an unstyled
//! slider.  The painters in `paint.rs` resolve these overrides, and the
//! geometry helpers here (`track_height`, `thumb_radius`) feed both painting
//! and the pointer mapping in `mod.rs`.
//!
//! The geometry and thumb options exist so an app can draw agg-sharp's
//! `SlideView` as MatterCAD configures it for property sliders: a 1 px track
//! (`track_height`) in the text colour, a 5 px thumb radius, and the thumb
//! drawn as an accent ring around the background colour (`thumb_center`) or,
//! with `thumb_hollow`, as a bare outline the track shows through.

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
    /// Rail thickness in logical px.  Default 4.
    pub track_height: Option<f64>,
    /// Thumb radius in logical px (the circle's radius; a rect handle scales
    /// from it).  The track's ends are inset by it so the thumb never
    /// overhangs the widget.  Default 7.
    pub thumb_radius: Option<f64>,
    /// Width of the circle thumb's ring (outer radius minus the centre
    /// disc's radius, or the outline width when `thumb_hollow`).  Default 2.5.
    pub thumb_ring_width: Option<f64>,
    /// Fill of the circle thumb's centre disc.  Default `Visuals::widget_bg`.
    /// Ignored when `thumb_hollow` is set.
    pub thumb_center: Option<Color>,
    /// Draw the circle thumb as an outline only (a stroked ring of
    /// `thumb_ring_width`), leaving its centre unpainted so the track shows
    /// through.  Default `false`: an opaque ring around `thumb_center`.
    pub thumb_hollow: bool,
    /// Trailing-fill colour (the rail from its start to the thumb).
    /// Default `Visuals::accent`.
    pub fill: Option<Color>,
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
        self.style
            .track_radius
            .unwrap_or(self.track_height() * 0.5)
            .max(0.0)
    }

    /// Resolved rail thickness.
    pub(super) fn track_height(&self) -> f64 {
        self.style.track_height.unwrap_or(TRACK_H).max(0.0)
    }

    /// Leading edge (bottom for a horizontal rail, left for a vertical one)
    /// of a rail of `track_height` centred on `center`.  An explicit
    /// `track_height` is snapped to whole pixels so a thin (1 px) rail
    /// stays one crisp row instead of two half-covered ones; the default
    /// rail keeps its exact, unsnapped position.
    pub(super) fn track_start(&self, center: f64) -> f64 {
        let edge = center - self.track_height() * 0.5;
        if self.style.track_height.is_some() {
            (edge + 0.5).floor()
        } else {
            edge
        }
    }

    /// Resolved thumb radius.
    pub(super) fn thumb_radius(&self) -> f64 {
        self.style.thumb_radius.unwrap_or(THUMB_R).max(0.0)
    }

    /// Resolved circle-thumb ring width, never more than the radius.
    pub(super) fn thumb_ring_width(&self) -> f64 {
        self.style
            .thumb_ring_width
            .unwrap_or(THUMB_RING_W)
            .clamp(0.0, self.thumb_radius())
    }
}
