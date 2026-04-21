//! `ToggleSwitch` — an iOS-style pill-shaped boolean toggle widget.
//!
//! Renders as a rounded-rectangle (pill) with a sliding white circle inside.
//! The pill is gray when off and blue when on.  Supports keyboard activation
//! (Space / Enter) and an optional shared [`Cell<bool>`] for two-way binding
//! with external state.

use std::cell::Cell;
use std::rc::Rc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

// ── Geometry constants ─────────────────────────────────────────────────────
//
// Sized to fit within a typical 16-18 px text line (13-14 px font) so the
// switch sits flush beside a label without inflating the row height.

const PILL_W: f64 = 32.0;
const PILL_H: f64 = 18.0;
/// Corner radius of the pill — a full semicircle on each end.
const PILL_R: f64 = PILL_H / 2.0;
/// Gap between the pill edge and the circle edge.
const CIRCLE_MARGIN: f64 = 2.5;
/// Circle radius derived from pill height and the margin.
const CIRCLE_R: f64 = PILL_H / 2.0 - CIRCLE_MARGIN;
/// Duration of the on/off slide animation in seconds.
const ANIM_SECS: f64 = 0.14;

// ── Press-ring overlay ───────────────────────────────────────────────────
//
// Matches MatterCAD's `RoundedToggleSwitch`: on mouse-down a translucent
// disc centred on the toggle circle expands outward; on mouse-up it fades
// back.  The MatterCAD version used a radius ratio of ~2.44× the circle
// radius (22 vs 9 px) and ~50/255 alpha with quadratic ease-out.

/// Maximum radius of the press-ring overlay (~2.4× the circle radius).
const RING_MAX_R: f64 = CIRCLE_R * 2.4;
/// Peak alpha of the press-ring at full expansion.
const RING_PEAK_ALPHA: f32 = 0.20;
/// Duration of the press-ring expand / retract animation in seconds.
const RING_ANIM_SECS:  f64 = 0.22;

// Colors are resolved from ctx.visuals() at paint time.

// ── Struct ─────────────────────────────────────────────────────────────────

/// An iOS-style boolean toggle.
///
/// Displays a pill-shaped background that switches from gray (off) to blue (on)
/// with a white circle that slides to the opposite end.
pub struct ToggleSwitch {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,
    /// Internal on/off state, used when `state_cell` is `None`.
    on: bool,
    /// When set, this cell is the authoritative state; `paint` reads from it
    /// and `toggle` writes to it so external changes are reflected immediately.
    state_cell: Option<Rc<Cell<bool>>>,
    hovered: bool,
    /// Interpolates between 0.0 (off) and 1.0 (on) for smooth colour/circle
    /// position transitions; driven by `animation::Tween`.
    anim:      crate::animation::Tween,
    /// Interpolates 0.0 → 1.0 while the mouse is pressed (ring expand) and
    /// back to 0.0 on release (ring fade).  Mirrors MatterCAD's
    /// `RoundedToggleSwitch` ripple overlay.
    press_anim: crate::animation::Tween,
    on_change: Option<Box<dyn FnMut(bool)>>,
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
            on,
            state_cell: None,
            hovered: false,
            anim: crate::animation::Tween::new(initial, ANIM_SECS),
            press_anim: crate::animation::Tween::new(0.0, RING_ANIM_SECS),
            on_change: None,
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

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

    // ── State accessors ────────────────────────────────────────────────────

    /// Returns the authoritative on/off state: the cell value if bound,
    /// otherwise the internal `on` field.
    pub fn is_on(&self) -> bool {
        if let Some(ref cell) = self.state_cell { cell.get() } else { self.on }
    }

    // ── Internal helpers ───────────────────────────────────────────────────

    fn toggle(&mut self) {
        let new_val = !self.is_on();
        self.on = new_val;
        if let Some(ref cell) = self.state_cell { cell.set(new_val); }
        if let Some(cb) = self.on_change.as_mut() { cb(new_val); }
    }

    /// X-center of the sliding circle given an interpolated position `t`
    /// in `[0, 1]` (0 = off, 1 = on).
    fn circle_cx_at(t: f64) -> f64 {
        let x_off = CIRCLE_MARGIN + CIRCLE_R;
        let x_on  = PILL_W - CIRCLE_MARGIN - CIRCLE_R;
        x_off + (x_on - x_off) * t.clamp(0.0, 1.0)
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
    fn type_name(&self) -> &'static str { "ToggleSwitch" }

    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn is_focusable(&self) -> bool { true }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    /// Always returns the fixed pill size; the available space is ignored.
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(PILL_W, PILL_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();

        // Retarget the tween each paint so external state-cell writes are
        // picked up (e.g. a checkbox-style binding toggled from outside), then
        // advance it to get this frame's interpolated position.
        self.anim.set_target(if self.is_on() { 1.0 } else { 0.0 });
        let t = self.anim.tick();

        // Origin (0,0) is the widget's bottom-left; framework has translated.
        let pill_x = 0.0_f64;
        let pill_y = 0.0_f64;

        // ── Pill background ────────────────────────────────────────────────
        // Interpolate between the off colour (gray) and the on colour (accent);
        // a separate hover tint is applied as a multiplicative brighten.
        let off_color = v.widget_stroke;
        let on_color  = v.accent;
        let mut bg = lerp_color(off_color, on_color, t as f32);
        if self.hovered {
            let hover_off = v.widget_bg_hovered;
            let hover_on  = v.accent_hovered;
            bg = lerp_color(hover_off, hover_on, t as f32);
        }
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(pill_x, pill_y, PILL_W, PILL_H, PILL_R);
        ctx.fill();

        // ── Sliding white circle ───────────────────────────────────────────
        let cx = Self::circle_cx_at(t);
        let cy = PILL_H * 0.5;
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.circle(cx, cy, CIRCLE_R);
        ctx.fill();

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
        if ring_t <= 0.001 { return; }

        let v  = ctx.visuals();
        let cx = Self::circle_cx_at(self.anim.value());
        let cy = PILL_H * 0.5;
        let toggle_color = if self.is_on() { v.accent } else { v.widget_stroke };
        let alpha        = RING_PEAK_ALPHA * (ring_t as f32);

        ctx.save();
        ctx.reset_clip();
        ctx.set_fill_color(Color::rgba(
            toggle_color.r, toggle_color.g, toggle_color.b, alpha));
        ctx.begin_path();
        ctx.circle(cx, cy, RING_MAX_R * ring_t);
        ctx.fill();
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                if was != self.hovered { crate::animation::request_tick(); }
                EventResult::Ignored
            }
            Event::MouseDown { button: MouseButton::Left, .. } => {
                // Consume on down so the widget "captures" the gesture, and
                // start the press-ring expand animation.
                self.press_anim.set_target(1.0);
                crate::animation::request_tick();
                EventResult::Consumed
            }
            Event::MouseUp { button: MouseButton::Left, pos, .. } => {
                if self.hit_test(*pos) { self.toggle(); }
                // Ring fades back out whether or not the release landed on us.
                self.press_anim.set_target(0.0);
                crate::animation::request_tick();
                EventResult::Consumed
            }
            Event::KeyDown { key: Key::Char(' '), .. }
            | Event::KeyDown { key: Key::Enter, .. } => {
                self.toggle();
                crate::animation::request_tick();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Hit test restricted to the pill bounds (matches the visible shape).
    fn hit_test(&self, local_pos: crate::geometry::Point) -> bool {
        local_pos.x >= 0.0 && local_pos.x <= PILL_W
            && local_pos.y >= 0.0 && local_pos.y <= PILL_H
    }
}
