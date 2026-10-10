//! Enabled state for [`super::ToggleSwitch`] — the agg-gui counterpart of
//! agg-sharp's `GuiWidget.Enabled` on a switch.
//!
//! A switch built with [`ToggleSwitch::with_enabled_fn`] asks its predicate
//! live (the same convention as `Button::with_enabled_fn` and
//! `ComboBox::with_enabled_fn`).  While it answers `false` the switch reports
//! `is_enabled() == false`, is not focusable, ignores pointer and keyboard
//! input, and paints dimmed:
//!
//! - by default as agg-sharp's `SelectionControlStyle.DrawSwitch` does: the
//!   bar and the knob keep their colours with the alpha multiplied by
//!   `DisabledOpacity` (0.4), and no hover tint;
//! - with [`ToggleSwitchStyle::disabled_color`] set, as MatterCAD's
//!   `RoundedToggleSwitch` does: the bar is only a 1 px outline and the knob
//!   a filled circle, both in that colour (its `inactiveBarColor`).
//!
//! `toggle_switch.rs` calls into here from `paint` and `on_event`.

use super::*;

/// agg-sharp's `SelectionControlStyle.DisabledOpacity`: a disabled switch's
/// colours keep this fraction of their alpha.
pub const TOGGLE_DISABLED_OPACITY: f32 = 0.4;

impl ToggleSwitch {
    /// Gate the switch on a live predicate (`None`, the default, is always
    /// enabled).  See the module docs for what a disabled switch does.
    pub fn with_enabled_fn(mut self, f: impl Fn() -> bool + 'static) -> Self {
        self.enabled_fn = Some(Rc::new(f));
        self
    }

    pub(super) fn enabled_now(&self) -> bool {
        self.enabled_fn.as_ref().map(|f| f()).unwrap_or(true)
    }

    /// agg-sharp's `Dim`: `color` with its alpha scaled by
    /// [`TOGGLE_DISABLED_OPACITY`].
    pub(super) fn dim(color: Color) -> Color {
        color.with_alpha(color.a * TOGGLE_DISABLED_OPACITY)
    }

    /// Drop hover and press state (and retract the ripple) once the
    /// predicate has turned `false`, so re-enabling never resumes a stale
    /// press.
    pub(super) fn clear_interaction(&mut self) {
        if self.hovered || self.pressed {
            crate::animation::request_draw();
        }
        self.hovered = false;
        self.pressed = false;
        self.press_anim.set_target(0.0);
    }

    /// Paint the disabled switch at interpolated position `t` (see the
    /// module docs for the two looks).
    pub(super) fn paint_disabled(&mut self, ctx: &mut dyn DrawCtx, t: f64) {
        let v = ctx.visuals();
        let (pill_x, pill_y) = self.pill_origin();
        let (pill_w, pill_h) = (self.pill_w(), self.pill_h());
        let pill_r = pill_h * 0.5;
        let (cx, cy, r) = (self.circle_cx_at(t), self.circle_cy(), self.knob_r());

        if let Some(color) = self.style.disabled_color {
            // RoundedToggleSwitch: Stroke(backgroundBar, 1) + Circle, both
            // in inactiveBarColor.
            ctx.set_stroke_color(color);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rounded_rect(pill_x, pill_y, pill_w, pill_h, pill_r);
            ctx.stroke();
            ctx.set_fill_color(color);
            ctx.begin_path();
            ctx.circle(cx, cy, r);
            ctx.fill();
            return;
        }

        let (off_color, on_color) = self.track_colors(&v);
        ctx.set_fill_color(Self::dim(lerp_color(off_color, on_color, t as f32)));
        ctx.begin_path();
        ctx.rounded_rect(pill_x, pill_y, pill_w, pill_h, pill_r);
        ctx.fill();
        if let Some(outline) = self.style.track_outline {
            ctx.set_stroke_color(Self::dim(outline));
            ctx.set_line_width(self.style.track_outline_width.unwrap_or(1.0));
            ctx.begin_path();
            ctx.rounded_rect(pill_x, pill_y, pill_w, pill_h, pill_r);
            ctx.stroke();
        }
        let (knob_off, knob_on) = self.knob_colors();
        ctx.set_fill_color(Self::dim(lerp_color(knob_off, knob_on, t as f32)));
        ctx.begin_path();
        ctx.circle(cx, cy, r);
        ctx.fill();
        if let Some(outline) = self.style.knob_outline {
            ctx.set_stroke_color(Self::dim(outline));
            ctx.set_line_width(self.style.knob_outline_width.unwrap_or(1.0));
            ctx.begin_path();
            ctx.circle(cx, cy, r);
            ctx.stroke();
        }
    }
}
