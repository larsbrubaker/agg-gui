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

const PILL_W: f64 = 48.0;
const PILL_H: f64 = 26.0;
/// Corner radius of the pill — a full semicircle on each end.
const PILL_R: f64 = PILL_H / 2.0;
/// Gap between the pill edge and the circle edge.
const CIRCLE_MARGIN: f64 = 3.0;
/// Circle radius derived from pill height and the margin.
const CIRCLE_R: f64 = PILL_H / 2.0 - CIRCLE_MARGIN;

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
    focused: bool,
    on_change: Option<Box<dyn FnMut(bool)>>,
}

// ── Constructors & builder methods ─────────────────────────────────────────

impl ToggleSwitch {
    /// Create a new toggle switch with an initial on/off state.
    pub fn new(on: bool) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            on,
            state_cell: None,
            hovered: false,
            focused: false,
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

    /// X-center of the sliding circle in local pill coordinates.
    fn circle_cx(&self) -> f64 {
        if self.is_on() {
            PILL_W - CIRCLE_MARGIN - CIRCLE_R
        } else {
            CIRCLE_MARGIN + CIRCLE_R
        }
    }
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
        let on = self.is_on();

        // The pill is drawn at (0, 0) in local coordinates; the framework has
        // already translated the context to this widget's bottom-left corner.
        let pill_x = 0.0_f64;
        let pill_y = 0.0_f64;

        // ── Focus ring (drawn first, behind the pill) ──────────────────────
        if self.focused {
            ctx.set_stroke_color(v.accent_focus);
            ctx.set_line_width(2.5);
            ctx.begin_path();
            ctx.rounded_rect(
                pill_x - 2.5,
                pill_y - 2.5,
                PILL_W + 5.0,
                PILL_H + 5.0,
                PILL_R + 2.5,
            );
            ctx.stroke();
        }

        // ── Pill background ────────────────────────────────────────────────
        // Off state uses the widget_bg / widget_bg_hovered colors.
        let bg = match (on, self.hovered) {
            (true,  true)  => v.accent_hovered,
            (true,  false) => v.accent,
            (false, true)  => v.widget_bg_hovered,
            (false, false) => v.widget_stroke, // mid-gray pill when off
        };
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(pill_x, pill_y, PILL_W, PILL_H, PILL_R);
        ctx.fill();

        // ── Sliding white circle ───────────────────────────────────────────
        let cx = self.circle_cx();
        let cy = PILL_H * 0.5;
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.circle(cx, cy, CIRCLE_R);
        ctx.fill();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.hovered = self.hit_test(*pos);
                EventResult::Ignored
            }
            Event::MouseDown { button: MouseButton::Left, .. } => {
                // Consume on down so the widget "captures" the gesture.
                EventResult::Consumed
            }
            Event::MouseUp { button: MouseButton::Left, pos, .. } => {
                if self.hit_test(*pos) { self.toggle(); }
                EventResult::Consumed
            }
            Event::KeyDown { key: Key::Char(' '), .. }
            | Event::KeyDown { key: Key::Enter, .. } => {
                self.toggle();
                EventResult::Consumed
            }
            Event::FocusGained => { self.focused = true;  EventResult::Ignored }
            Event::FocusLost   => { self.focused = false; EventResult::Ignored }
            _ => EventResult::Ignored,
        }
    }

    /// Hit test restricted to the pill bounds (matches the visible shape).
    fn hit_test(&self, local_pos: crate::geometry::Point) -> bool {
        local_pos.x >= 0.0 && local_pos.x <= PILL_W
            && local_pos.y >= 0.0 && local_pos.y <= PILL_H
    }
}
