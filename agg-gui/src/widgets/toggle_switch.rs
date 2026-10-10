//! `ToggleSwitch` — an iOS-style pill-shaped boolean toggle widget.
//!
//! Renders as a rounded-rectangle (pill) with a sliding white circle inside.
//! The pill is gray when off and blue when on.  Supports keyboard activation
//! (Space / Enter) and an optional shared [`Cell<bool>`] for two-way binding
//! with external state.  Colours and geometry can be overridden per instance
//! with a [`ToggleSwitchStyle`] (`toggle_switch/style.rs`).  A switch gated
//! with `with_enabled_fn` paints dimmed and ignores input while disabled
//! (`toggle_switch/enabled.rs`).

use std::cell::Cell;
use std::rc::Rc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

mod enabled;
mod style;
pub use enabled::TOGGLE_DISABLED_OPACITY;
pub use style::ToggleSwitchStyle;

// ── Geometry constants ─────────────────────────────────────────────────────
//
// Sized to fit within a typical 16-18 px text line (13-14 px font) so the
// switch sits flush beside a label without inflating the row height.

const PILL_W: f64 = 32.0;
const PILL_H: f64 = 18.0;
/// Gap between the pill edge and the circle edge.
const CIRCLE_MARGIN: f64 = 2.5;
/// Circle radius derived from pill height and the margin.
const CIRCLE_R: f64 = PILL_H / 2.0 - CIRCLE_MARGIN;
/// Duration of the on/off slide animation in seconds.
const ANIM_SECS: f64 = 0.14;
/// Inset on each side between the widget's outer bounds and the pill
/// geometry.  The halo-AA pipeline extrudes the pill's filled edges one
/// pixel outward; without a margin that halo sits outside the widget's
/// own bounds and gets clipped by the parent's `clip_rect(0, 0, w, h)` —
/// the bottom edge loses its AA fade and looks flat-cut.  One pixel is
/// enough to keep the full halo inside the clip.
const PILL_HALO: f64 = 1.0;

// ── Press-ring overlay ───────────────────────────────────────────────────
//
// Matches MatterCAD's `RoundedToggleSwitch`: on mouse-down a translucent
// disc centred on the toggle circle expands outward; on mouse-up it fades
// back.  The MatterCAD version used a radius ratio of ~2.44× the circle
// radius (22 vs 9 px) and ~50/255 alpha with quadratic ease-out.

/// Maximum radius of the press-ring overlay as a multiple of the knob
/// radius (~2.4×).
const RING_MAX_RATIO: f64 = 2.4;
/// Peak alpha of the press-ring at full expansion.
const RING_PEAK_ALPHA: f32 = 0.20;
/// Duration of the press-ring expand / retract animation in seconds.
const RING_ANIM_SECS: f64 = 0.22;

// Colors are resolved from ctx.visuals() at paint time.

// ── Struct ─────────────────────────────────────────────────────────────────

/// Inspector-visible properties of a [`ToggleSwitch`].
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[derive(Clone, Debug, Default)]
pub struct ToggleSwitchProps {
    /// Internal on/off state, used when `state_cell` is `None`.
    pub on: bool,
}

/// An iOS-style boolean toggle.
///
/// Displays a pill-shaped background that switches from gray (off) to blue (on)
/// with a white circle that slides to the opposite end.
pub struct ToggleSwitch {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,
    pub props: ToggleSwitchProps,
    /// When set, this cell is the authoritative state; `paint` reads from it
    /// and `toggle` writes to it so external changes are reflected immediately.
    state_cell: Option<Rc<Cell<bool>>>,
    hovered: bool,
    /// Interpolates between 0.0 (off) and 1.0 (on) for smooth colour/circle
    /// position transitions; driven by `animation::Tween`.
    anim: crate::animation::Tween,
    pressed: bool,
    /// Interpolates 0.0 → 1.0 while the mouse is pressed (ring expand) and
    /// back to 0.0 on release (ring fade).  Mirrors MatterCAD's
    /// `RoundedToggleSwitch` ripple overlay.
    press_anim: crate::animation::Tween,
    on_change: Option<Box<dyn FnMut(bool)>>,
    /// Per-instance colour / geometry overrides — see [`ToggleSwitchStyle`].
    style: ToggleSwitchStyle,
    /// Live enabled gate — see [`ToggleSwitch::with_enabled_fn`]
    /// (`toggle_switch/enabled.rs`).  `None` = always enabled.
    enabled_fn: Option<Rc<dyn Fn() -> bool>>,
}

// ── Constructors & builder methods ─────────────────────────────────────────

impl ToggleSwitch {
    /// Create a new toggle switch with an initial on/off state.
    pub fn new(on: bool) -> Self {
        let initial = if on { 1.0 } else { 0.0 };
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            props: ToggleSwitchProps { on },
            state_cell: None,
            hovered: false,
            anim: crate::animation::Tween::new(initial, ANIM_SECS),
            pressed: false,
            press_anim: crate::animation::Tween::new(0.0, RING_ANIM_SECS),
            on_change: None,
            style: ToggleSwitchStyle::default(),
            enabled_fn: None,
        }
    }

    /// Bind the toggle state to a shared [`Cell<bool>`].
    ///
    /// When set, `paint` reads from the cell (so external writes are reflected
    /// immediately) and `toggle` writes to it in both directions.
    pub fn with_state_cell(mut self, cell: Rc<Cell<bool>>) -> Self {
        self.state_cell = Some(cell);
        self
    }

    /// Register a callback invoked with the new state whenever the switch
    /// is toggled.
    pub fn on_change(mut self, cb: impl FnMut(bool) + 'static) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }

    // ── State accessors ────────────────────────────────────────────────────

    /// Returns the authoritative on/off state: the cell value if bound,
    /// otherwise the internal `on` field.
    pub fn is_on(&self) -> bool {
        if let Some(ref cell) = self.state_cell {
            cell.get()
        } else {
            self.props.on
        }
    }

    // ── Internal helpers ───────────────────────────────────────────────────

    fn toggle(&mut self) {
        let new_val = !self.is_on();
        self.props.on = new_val;
        if let Some(ref cell) = self.state_cell {
            cell.set(new_val);
        }
        if let Some(cb) = self.on_change.as_mut() {
            cb(new_val);
        }
    }

    /// X-center of the sliding circle given an interpolated position `t`
    /// in `[0, 1]` (0 = off, 1 = on).  Expressed in widget-local coords,
    /// so the `PILL_HALO` inset is baked in — callers don't need to know
    /// about it.  The centre sits on the centre of the bar's end cap
    /// (`CIRCLE_MARGIN + CIRCLE_R == PILL_H / 2` for the default pill).
    fn circle_cx_at(&self, t: f64) -> f64 {
        let (ox, _) = self.pill_origin();
        let half_h = self.pill_h() * 0.5;
        let x_off = ox + half_h;
        let x_on = ox + self.pill_w() - half_h;
        x_off + (x_on - x_off) * t.clamp(0.0, 1.0)
    }

    /// Y-center of the knob (the bar's centre line).
    fn circle_cy(&self) -> f64 {
        self.pill_origin().1 + self.pill_h() * 0.5
    }
}

/// Linear interpolation between two colours, component-wise.
fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

// ── Widget impl ────────────────────────────────────────────────────────────

impl Widget for ToggleSwitch {
    crate::widgets::widget_as_any!();
    fn type_name(&self) -> &'static str {
        "ToggleSwitch"
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    #[cfg(feature = "reflect")]
    fn as_reflect(&self) -> Option<&dyn bevy_reflect::Reflect> {
        Some(&self.props)
    }
    #[cfg(feature = "reflect")]
    fn as_reflect_mut(&mut self) -> Option<&mut dyn bevy_reflect::Reflect> {
        Some(&mut self.props)
    }
    fn is_enabled(&self) -> bool {
        self.enabled_now()
    }
    fn is_focusable(&self) -> bool {
        self.enabled_now()
    }

    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }
    fn min_size(&self) -> Size {
        self.base.min_size
    }
    fn max_size(&self) -> Size {
        self.base.max_size
    }

    /// Always returns the fixed pill size (plus a 1 px halo margin on
    /// every side); the available space is ignored.  See [`PILL_HALO`]
    /// for why the margin is needed.
    fn layout(&mut self, _available: Size) -> Size {
        self.outer_size()
    }

    fn needs_draw(&self) -> bool {
        if !self.is_visible() {
            return false;
        }
        self.anim.is_animating()
            || self.press_anim.is_animating()
            || self.children().iter().any(|c| c.needs_draw())
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();

        // Retarget the tween each paint so external state-cell writes are
        // picked up (e.g. a checkbox-style binding toggled from outside), then
        // advance it to get this frame's interpolated position.
        self.anim.set_target(if self.is_on() { 1.0 } else { 0.0 });
        let t = self.anim.tick();
        if !self.enabled_now() {
            self.clear_interaction();
            self.paint_disabled(ctx, t);
            return;
        }

        // Inset the pill by the halo margin so halo-AA has room inside
        // the widget's own clip.  Origin (0,0) is the widget's bottom-
        // left in Y-up; the framework has already translated there.
        // (Plus any knob overhang from a style's oversized knob.)
        let (pill_x, pill_y) = self.pill_origin();
        let (pill_w, pill_h) = (self.pill_w(), self.pill_h());
        let pill_r = pill_h * 0.5;

        // ── Pill background ────────────────────────────────────────────────
        // Interpolate between the off colour (gray) and the on colour (accent);
        // under the mouse both ends switch to their hover tints (unless a
        // style pins them).
        let (off_color, on_color) = self.track_colors(&v);
        let bg = lerp_color(off_color, on_color, t as f32);
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(pill_x, pill_y, pill_w, pill_h, pill_r);
        ctx.fill();
        if let Some(outline) = self.style.track_outline {
            ctx.set_stroke_color(outline);
            ctx.set_line_width(self.style.track_outline_width.unwrap_or(1.0));
            ctx.begin_path();
            ctx.rounded_rect(pill_x, pill_y, pill_w, pill_h, pill_r);
            ctx.stroke();
        }

        // ── Sliding circle (white by default) ──────────────────────────────
        let cx = self.circle_cx_at(t);
        let cy = self.circle_cy();
        let (knob_off, knob_on) = self.knob_colors();
        ctx.set_fill_color(lerp_color(knob_off, knob_on, t as f32));
        ctx.begin_path();
        ctx.circle(cx, cy, self.knob_r());
        ctx.fill();
        if let Some(outline) = self.style.knob_outline {
            ctx.set_stroke_color(outline);
            ctx.set_line_width(self.style.knob_outline_width.unwrap_or(1.0));
            ctx.begin_path();
            ctx.circle(cx, cy, self.knob_r());
            ctx.stroke();
        }

        // The press-ring itself is drawn in `paint_overlay` — it needs to
        // expand beyond the widget's own bounds, which requires escaping the
        // parent-set clip that `paint` runs under.
    }

    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        // ── Press-ring overlay (ripple) ────────────────────────────────────
        // Translucent disc centred on the toggle circle.  At full expansion
        // the ring is ~2.4× the circle radius and would be cropped by the
        // pill-sized widget clip if drawn in `paint()`.  We therefore draw it
        // in `paint_overlay` and temporarily lift the parent's clip via
        // `reset_clip` so the ring can render the full ripple geometry (then
        // `restore` puts the saved clip state back before returning).
        let ring_t = self.press_anim.tick();
        if ring_t <= 0.001 || !self.enabled_now() {
            return;
        }

        let v = ctx.visuals();
        let cx = self.circle_cx_at(self.anim.value());
        let cy = self.circle_cy();
        let toggle_color = self.ripple_color(&v);
        let alpha = RING_PEAK_ALPHA * (ring_t as f32);

        ctx.save();
        ctx.reset_clip();
        ctx.set_fill_color(Color::rgba(
            toggle_color.r,
            toggle_color.g,
            toggle_color.b,
            alpha,
        ));
        ctx.begin_path();
        ctx.circle(cx, cy, self.knob_r() * RING_MAX_RATIO * ring_t);
        ctx.fill();
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if !self.enabled_now() {
            self.clear_interaction();
            return EventResult::Ignored;
        }
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                if was != self.hovered {
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Left,
                ..
            } => {
                // Consume on down so the widget "captures" the gesture, and
                // start the press-ring expand animation.
                self.pressed = true;
                self.press_anim.set_target(1.0);
                crate::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseUp {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                if self.hit_test(*pos) {
                    self.toggle();
                }
                // Ring fades back out whether or not the release landed on us.
                self.pressed = false;
                self.press_anim.set_target(0.0);
                crate::animation::request_draw();
                EventResult::Consumed
            }
            Event::KeyDown {
                key: Key::Char(' '),
                ..
            }
            | Event::KeyDown {
                key: Key::Enter, ..
            } => {
                self.toggle();
                crate::animation::request_draw();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Hit test restricted to the pill bounds (matches the visible shape).
    /// The halo margin is excluded so the ~1 px ring around the pill
    /// doesn't register as pointer-over.
    /// With a style's oversized knob the knob's overhang counts too.
    fn hit_test(&self, local_pos: crate::geometry::Point) -> bool {
        let o = self.pill_origin().0;
        let oh = o - PILL_HALO; // knob overhang
        local_pos.x >= o - oh
            && local_pos.x <= o + self.pill_w() + oh
            && local_pos.y >= o - oh
            && local_pos.y <= o + self.pill_h() + oh
    }
}

#[cfg(test)]
mod tests;
