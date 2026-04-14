//! `DragValue` — a numeric scrubber that lets the user drag left/right to change a value.
//!
//! Displays the current value as formatted text centered inside a lightly
//! tinted rectangle.  Clicking and dragging horizontally adjusts the value at
//! a configurable speed; the value is clamped to `[min, max]` and optionally
//! snapped to a step interval.
//!
//! Typical use-case: property panels, inspector rows, compact parameter editors.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

// ── Geometry constants ─────────────────────────────────────────────────────

const WIDGET_H: f64 = 28.0;
/// Half-width of the left/right arrow indicator text.
const ARROW_MARGIN: f64 = 8.0;

// Colors are now resolved from ctx.visuals() at paint time.

// ── Struct ─────────────────────────────────────────────────────────────────

/// A horizontal drag-to-scrub numeric value widget.
///
/// The user clicks and drags left or right to decrease or increase the value.
/// The current value is displayed as formatted text in the center of the widget.
/// Left ("◀") and right ("▶") arrow indicators are drawn at the edges as a
/// visual affordance for the drag direction.
pub struct DragValue {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,

    value: f64,
    min: f64,
    max: f64,

    /// Number of logical pixels the user must drag to change `value` by one
    /// unit (with `step == 0`).  Default: `1.0`.
    speed: f64,
    /// Snap interval; values are rounded to the nearest multiple of `step`
    /// after each drag update.  `0.0` means no snapping.
    step: f64,
    /// Number of decimal places used when formatting the displayed value.
    decimals: usize,

    font: Arc<Font>,
    font_size: f64,

    /// Whether the user is currently dragging.
    dragging: bool,
    /// X position (in local widget coordinates) where the drag began.
    drag_start_x: f64,
    /// Value captured at the moment the drag began.
    drag_start_value: f64,

    hovered: bool,
    on_change: Option<Box<dyn FnMut(f64)>>,
}

// ── Constructors & builder methods ─────────────────────────────────────────

impl DragValue {
    /// Create a new `DragValue` with initial `value` clamped to `[min, max]`.
    pub fn new(value: f64, min: f64, max: f64, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            value: value.clamp(min, max),
            min,
            max,
            speed: 1.0,
            step: 0.0,
            decimals: 2,
            font,
            font_size: 13.0,
            dragging: false,
            drag_start_x: 0.0,
            drag_start_value: 0.0,
            hovered: false,
            on_change: None,
        }
    }

    /// Set the display font size in logical pixels.
    pub fn with_font_size(mut self, s: f64) -> Self { self.font_size = s; self }

    /// Set a snap step.  Values are rounded to the nearest multiple of `step`
    /// during dragging.  Pass `0.0` to disable snapping (the default).
    pub fn with_step(mut self, step: f64) -> Self { self.step = step; self }

    /// Set the drag speed: how many value units change per logical pixel of
    /// horizontal drag movement.  Default is `1.0`.
    pub fn with_speed(mut self, speed: f64) -> Self { self.speed = speed; self }

    /// Set the number of decimal places shown in the formatted text.
    pub fn with_decimals(mut self, d: usize) -> Self { self.decimals = d; self }

    /// Register a callback invoked with the new value on every drag update.
    pub fn on_change(mut self, cb: impl FnMut(f64) + 'static) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

    // ── State accessor ─────────────────────────────────────────────────────

    /// Returns the current value.
    pub fn value(&self) -> f64 { self.value }

    // ── Internal helpers ───────────────────────────────────────────────────

    /// Format the value for display.
    fn format_value(&self) -> String {
        format!("{:.prec$}", self.value, prec = self.decimals)
    }

    /// Apply step snapping to a raw value, then clamp to `[min, max]`.
    fn apply_step_and_clamp(&self, raw: f64) -> f64 {
        let snapped = if self.step > 0.0 {
            (raw / self.step).round() * self.step
        } else {
            raw
        };
        snapped.clamp(self.min, self.max)
    }

    /// Compute the new value from a drag position and fire the callback.
    fn update_from_drag(&mut self, current_x: f64) {
        let delta = (current_x - self.drag_start_x) * self.speed;
        let raw   = self.drag_start_value + delta;
        self.value = self.apply_step_and_clamp(raw);
        let v = self.value;
        if let Some(cb) = self.on_change.as_mut() { cb(v); }
    }
}

// ── Widget impl ────────────────────────────────────────────────────────────

impl Widget for DragValue {
    fn type_name(&self) -> &'static str { "DragValue" }

    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn is_focusable(&self) -> bool { false }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    /// Fixed height of 28 px; width fills the available space offered by the parent.
    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, WIDGET_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Derive drag-value background from accent with varying opacity.
        let a = v.accent;
        let bg = if self.dragging {
            Color::rgba(a.r, a.g, a.b, 0.22)
        } else if self.hovered {
            Color::rgba(a.r, a.g, a.b, 0.14)
        } else {
            Color::rgba(a.r, a.g, a.b, 0.08)
        };
        let border = Color::rgba(a.r, a.g, a.b, 0.35);
        let arrow   = Color::rgba(a.r, a.g, a.b, 0.45);

        // ── Background ─────────────────────────────────────────────────────
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
        ctx.fill();

        // ── Border ─────────────────────────────────────────────────────────
        ctx.set_stroke_color(border);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
        ctx.stroke();

        // ── Arrow indicators ───────────────────────────────────────────────
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        ctx.set_fill_color(arrow);

        // Left arrow ("◀") near the left edge.
        if let Some(lm) = ctx.measure_text("◀") {
            let ly = h * 0.5 - (lm.ascent - lm.descent) * 0.5 + lm.descent;
            ctx.fill_text("◀", ARROW_MARGIN, ly);
        }

        // Right arrow ("▶") near the right edge.
        if let Some(rm) = ctx.measure_text("▶") {
            let rx = w - ARROW_MARGIN - rm.width;
            let ry = h * 0.5 - (rm.ascent - rm.descent) * 0.5 + rm.descent;
            ctx.fill_text("▶", rx, ry);
        }

        // ── Centered value text ────────────────────────────────────────────
        let label = self.format_value();
        ctx.set_fill_color(v.text_color);
        if let Some(m) = ctx.measure_text(&label) {
            let tx = (w - m.width) * 0.5;
            let ty = h * 0.5 - (m.ascent - m.descent) * 0.5 + m.descent;
            ctx.fill_text(&label, tx, ty);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.hovered = self.hit_test(*pos);
                if self.dragging {
                    self.update_from_drag(pos.x);
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown { button: MouseButton::Left, pos, .. } => {
                self.dragging         = true;
                self.drag_start_x     = pos.x;
                self.drag_start_value = self.value;
                EventResult::Consumed
            }
            Event::MouseUp { button: MouseButton::Left, .. } => {
                self.dragging = false;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}
