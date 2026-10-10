//! Per-instance styling for [`super::ToggleSwitch`].
//!
//! By default the switch paints an iOS-style pill (32 x 18) that blends from
//! `Visuals::widget_stroke` (off) to `Visuals::accent` (on), with a white knob
//! inset inside it.  A [`ToggleSwitchStyle`] overrides any of the colours and
//! the geometry; every field is optional and `None` keeps the default, so an
//! all-`None` style paints and lays out exactly like an unstyled switch.
//!
//! The options exist so an app can draw MatterCAD's `RoundedToggleSwitch`: a
//! thin bar (12.6 px tall) with a knob larger than the bar (radius 9) that
//! overhangs it, a grey knob on a light bar when off and an accent knob on a
//! translucent accent bar when on.  The knob's centre always sits at the
//! centre of the bar's rounded end cap (as in both the default pill and
//! MatterCAD's switch), so a larger knob simply overhangs the bar.

use super::*;

/// Optional per-instance overrides for a [`ToggleSwitch`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToggleSwitchStyle {
    /// Bar colour when off.  Default `Visuals::widget_stroke`
    /// (`Visuals::widget_bg_hovered` under the mouse).  When set it is used
    /// in every hover state.
    pub track_off: Option<Color>,
    /// Bar colour when on.  Default `Visuals::accent`
    /// (`Visuals::accent_hovered` under the mouse).  When set it is used in
    /// every hover state.
    pub track_on: Option<Color>,
    /// Knob colour when off.  Default white.  Also tints the press ripple.
    pub knob_off: Option<Color>,
    /// Knob colour when on.  Default white.  Also tints the press ripple.
    pub knob_on: Option<Color>,
    /// Colour of an outline stroked around the bar.  Default: no outline.
    pub track_outline: Option<Color>,
    /// Width of the bar outline.  Default 1 when `track_outline` is set.
    pub track_outline_width: Option<f64>,
    /// Colour of an outline stroked around the knob.  Default: no outline.
    pub knob_outline: Option<Color>,
    /// Width of the knob outline.  Default 1 when `knob_outline` is set.
    pub knob_outline_width: Option<f64>,
    /// Bar length.  Default 32.
    pub track_width: Option<f64>,
    /// Bar thickness; its ends are fully rounded.  Default 18.
    pub track_height: Option<f64>,
    /// Knob radius.  Default 6.5 (the bar's half height less a 2.5 margin).
    pub knob_radius: Option<f64>,
    /// MatterCAD `RoundedToggleSwitch`'s disabled look: when set, a disabled
    /// switch strokes the bar as a 1 px outline and fills the knob, both in
    /// this colour (its `inactiveBarColor`).  Default: agg-sharp's dimmed
    /// switch (see `toggle_switch/enabled.rs`).
    pub disabled_color: Option<Color>,
}

impl ToggleSwitch {
    /// Apply per-instance styling.  See [`ToggleSwitchStyle`].
    pub fn with_style(mut self, style: ToggleSwitchStyle) -> Self {
        self.style = style;
        self
    }

    /// The current per-instance style overrides.
    pub fn style(&self) -> ToggleSwitchStyle {
        self.style
    }

    /// Resolved bar length.
    pub(super) fn pill_w(&self) -> f64 {
        self.style.track_width.unwrap_or(PILL_W).max(0.0)
    }

    /// Resolved bar thickness.
    pub(super) fn pill_h(&self) -> f64 {
        self.style.track_height.unwrap_or(PILL_H).max(0.0)
    }

    /// Resolved knob radius.
    pub(super) fn knob_r(&self) -> f64 {
        self.style.knob_radius.unwrap_or(CIRCLE_R).max(0.0)
    }

    /// How far the knob overhangs the bar on each side (0 when it fits).
    fn overhang(&self) -> f64 {
        (self.knob_r() - self.pill_h() * 0.5).max(0.0)
    }

    /// Widget-local origin of the bar: the halo margin plus any knob
    /// overhang, so the whole knob stays inside the widget's clip.
    pub(super) fn pill_origin(&self) -> (f64, f64) {
        let o = PILL_HALO + self.overhang();
        (o, o)
    }

    /// The widget's laid-out size: the bar, the knob overhang and the halo
    /// margin on every side.
    pub(super) fn outer_size(&self) -> Size {
        let pad = 2.0 * (PILL_HALO + self.overhang());
        Size::new(self.pill_w() + pad, self.pill_h() + pad)
    }

    /// Bar colours (off, on) for the current hover state.
    pub(super) fn track_colors(&self, v: &crate::theme::Visuals) -> (Color, Color) {
        let off = self.style.track_off.unwrap_or(if self.hovered {
            v.widget_bg_hovered
        } else {
            v.widget_stroke
        });
        let on = self.style.track_on.unwrap_or(if self.hovered {
            v.accent_hovered
        } else {
            v.accent
        });
        (off, on)
    }

    /// Knob colours (off, on).
    pub(super) fn knob_colors(&self) -> (Color, Color) {
        (
            self.style.knob_off.unwrap_or(Color::white()),
            self.style.knob_on.unwrap_or(Color::white()),
        )
    }

    /// Press-ripple colour for the current state: the knob override when
    /// set, otherwise the default accent / `widget_stroke`.
    pub(super) fn ripple_color(&self, v: &crate::theme::Visuals) -> Color {
        if self.is_on() {
            self.style.knob_on.unwrap_or(v.accent)
        } else {
            self.style.knob_off.unwrap_or(v.widget_stroke)
        }
    }
}
