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
use crate::widget::{paint_subtree, Widget};
use crate::widgets::label::{Label, LabelAlign};

/// Format a numeric value as a string with the given decimal places.
/// Free function so `DragValue::new` can call it before `self` exists.
fn format_value(value: f64, decimals: usize) -> String {
    format!("{:.prec$}", value, prec = decimals)
}

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

    /// Font-size kept only so `with_font_size` can forward into the
    /// child label.  All actual glyph work happens in `value_label`.
    font_size: f64,

    /// Whether the user is currently dragging.
    dragging: bool,
    /// X position (in local widget coordinates) where the drag began.
    drag_start_x: f64,
    /// Value captured at the moment the drag began.
    drag_start_value: f64,

    hovered: bool,
    on_change: Option<Box<dyn FnMut(f64)>>,

    /// Formatted-value text lives in a `Label` field — DragValue draws
    /// bg + border + arrow triangles; the label handles the value text
    /// (including its own LCD cache).  Kept as a typed field rather
    /// than in `children` so we can call `set_text` on value change
    /// without downcasting.
    value_label: Label,
}

// ── Constructors & builder methods ─────────────────────────────────────────

impl DragValue {
    /// Create a new `DragValue` with initial `value` clamped to `[min, max]`.
    pub fn new(value: f64, min: f64, max: f64, font: Arc<Font>) -> Self {
        let clamped = value.clamp(min, max);
        let initial_text = format_value(clamped, 2);
        let value_label = Label::new(initial_text, font)
            .with_font_size(13.0)
            .with_align(LabelAlign::Center);
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            value: clamped,
            min,
            max,
            speed: 1.0,
            step: 0.0,
            decimals: 2,
            font_size: 13.0,
            dragging: false,
            drag_start_x: 0.0,
            drag_start_value: 0.0,
            hovered: false,
            on_change: None,
            value_label,
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
    pub fn with_decimals(mut self, d: usize) -> Self {
        self.decimals = d;
        self.sync_label();
        self
    }

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
        format_value(self.value, self.decimals)
    }

    /// Push the currently-formatted value into the child label.  Called
    /// whenever `value` or `decimals` changes.
    fn sync_label(&mut self) {
        self.value_label.set_text(self.format_value());
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
        self.sync_label();
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
        let arrow  = Color::rgba(a.r, a.g, a.b, 0.45);

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

        // ── Arrow triangles (shape fills, not text) ────────────────────────
        // Text glyphs for "◀" / "▶" would force a path that either uses
        // direct fill_text (skipping the LCD backbuffer architecture) or
        // needs its own Label cache per arrow.  Triangles are cheaper
        // and semantically correct — they're affordance marks, not text.
        let mid = h * 0.5;
        let tri_half = 4.0;
        let tri_w    = 6.0;
        ctx.set_fill_color(arrow);
        // Left: point at ARROW_MARGIN, base at ARROW_MARGIN + tri_w.
        ctx.begin_path();
        ctx.move_to(ARROW_MARGIN, mid);
        ctx.line_to(ARROW_MARGIN + tri_w, mid - tri_half);
        ctx.line_to(ARROW_MARGIN + tri_w, mid + tri_half);
        ctx.close_path();
        ctx.fill();
        // Right: point at w - ARROW_MARGIN, base at w - ARROW_MARGIN - tri_w.
        ctx.begin_path();
        ctx.move_to(w - ARROW_MARGIN, mid);
        ctx.line_to(w - ARROW_MARGIN - tri_w, mid - tri_half);
        ctx.line_to(w - ARROW_MARGIN - tri_w, mid + tri_half);
        ctx.close_path();
        ctx.fill();

        // ── Centred value text — painted via the Label child ───────────────
        // Layout the label to get its natural size, centre it inside the
        // widget, then recurse paint.  Label handles its own LCD cache /
        // per-channel blit.
        let avail_w = (w - (ARROW_MARGIN + tri_w + 4.0) * 2.0).max(1.0);
        let lsz = self.value_label.layout(Size::new(avail_w, h));
        let lx = (w - lsz.width)  * 0.5;
        let ly = (h - lsz.height) * 0.5;
        self.value_label.set_bounds(Rect::new(0.0, 0.0, lsz.width, lsz.height));
        ctx.save();
        ctx.translate(lx, ly);
        paint_subtree(&mut self.value_label, ctx);
        ctx.restore();
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
