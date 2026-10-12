//! Enabled state and thumb geometry for [`super::Slider`].
//!
//! The enabled gate is the agg-gui counterpart of agg-sharp's
//! `GuiWidget.Enabled` on a `Slider` (Gui/Slider/Slider.cs): a slider built
//! with [`Slider::with_enabled_fn`] asks its predicate live (the convention
//! of `Button::with_enabled_fn` and `ComboBox::with_enabled_fn`).  While it
//! answers `false` the slider reports `is_enabled() == false`, is not
//! focusable, drops pointer and keyboard input (ending a drag in flight with
//! its release), and paints as `SlideView.DoDrawAfterChildren` does for a
//! disabled slider: the accent goes grey (the track colour at twice its
//! alpha), the rail fades to half its alpha, and the thumb no longer shades
//! for hover or drag.
//!
//! [`Slider::thumb_rect`] is agg-sharp's `GetThumbHitBounds`: the thumb's
//! box in the slider's local (Y-up) coordinates, in logical units.
//!
//! Split out of `slider/mod.rs` to keep that file under the 800-line limit;
//! `paint.rs` consumes [`Slider::enabled`] and the disabled colours.

use super::*;
use crate::color::Color;

impl Slider {
    /// Gate the slider on a live predicate (`None`, the default, is always
    /// enabled).  See the module docs for what a disabled slider does.
    pub fn with_enabled_fn(mut self, f: impl Fn() -> bool + 'static) -> Self {
        self.enabled_fn = Some(Rc::new(f));
        self
    }

    /// Whether the slider takes input now.
    pub(super) fn enabled(&self) -> bool {
        self.enabled_fn.as_ref().is_none_or(|f| f())
    }

    /// The thumb's box in local coordinates (agg-sharp `GetThumbHitBounds`):
    /// centred on the thumb, as wide and tall as the handle paints.
    pub fn thumb_rect(&self) -> Rect {
        let r = self.thumb_radius();
        let (along, across) = match self.handle_shape {
            HandleShape::Circle => (r, r),
            HandleShape::Rect { aspect_ratio } => (r * aspect_ratio, r * 0.9),
        };
        let pos = self.thumb_pos();
        if self.is_vertical() {
            Rect::new(r - across, pos - along, across * 2.0, along * 2.0)
        } else {
            let cy = self.bounds.height * 0.5;
            Rect::new(pos - along, cy - across, along * 2.0, across * 2.0)
        }
    }

    /// The (fill / thumb, rail) colours a disabled slider paints with, from
    /// the rail colour `track` (C# `TrackColor`): the accent becomes the rail
    /// at twice its alpha, the rail itself half its alpha.
    pub(super) fn disabled_colors(track: Color) -> (Color, Color) {
        let a = (track.a * 255.0).round();
        let accent = track.with_alpha((a * 2.0).min(255.0) / 255.0);
        // C# integer division: `Alpha0To255 / 2` truncates.
        let rail = track.with_alpha(((a as i32) / 2) as f32 / 255.0);
        (accent, rail)
    }
}
