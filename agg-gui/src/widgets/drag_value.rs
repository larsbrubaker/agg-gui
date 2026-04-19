//! `DragValue` — a numeric scrubber that lets the user drag left/right to change a value.
//!
//! Displays the current value as formatted text centered inside a lightly
//! tinted rectangle.  Clicking and dragging horizontally adjusts the value at
//! a configurable speed; the value is clamped to `[min, max]` and optionally
//! snapped to a step interval.
//!
//! A plain click (no significant drag) enters an inline edit mode: the widget
//! shows a cursor and accepts keyboard input.  Pressing Enter or losing focus
//! commits the edit; Escape cancels it.
//!
//! Typical use-case: property panels, inspector rows, compact parameter editors.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, MouseButton};
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
/// Horizontal drag distance (logical px) before a press is treated as a drag.
const DRAG_THRESHOLD: f64 = 3.0;

// ── Struct ─────────────────────────────────────────────────────────────────

/// A horizontal drag-to-scrub numeric value widget.
///
/// The user clicks and drags left or right to decrease or increase the value.
/// A plain click (no drag) enters inline edit mode for direct keyboard entry.
/// The current value is displayed as formatted text in the center of the widget.
/// Left ("◀") and right ("▶") arrow triangles are drawn at the edges as an
/// affordance for the drag direction.
pub struct DragValue {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,

    value: f64,
    min: f64,
    max: f64,

    /// How many value units change per logical pixel of horizontal drag.
    speed: f64,
    /// Snap interval; values are rounded to the nearest multiple of `step`
    /// after each drag update.  `0.0` means no snapping.
    step: f64,
    /// Number of decimal places used when formatting the displayed value.
    decimals: usize,

    font: Arc<Font>,
    font_size: f64,

    // ── Drag state ────────────────────────────────────────────────────────
    /// True once the drag-threshold has been exceeded after a mouse-down.
    dragging: bool,
    /// True from mouse-down until mouse-up (covers both pre-threshold and drag phases).
    mouse_pressed: bool,
    /// X position where the mouse was pressed.
    press_x: f64,
    /// X position used as drag origin once the threshold is crossed.
    drag_start_x: f64,
    /// Value captured at the start of the confirmed drag.
    drag_start_value: f64,

    // ── Inline edit state ─────────────────────────────────────────────────
    focused: bool,
    editing: bool,
    edit_text: String,
    /// Cursor position as a char index into `edit_text`.
    edit_cursor: usize,

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
        let value_label = Label::new(initial_text, Arc::clone(&font))
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
            font,
            font_size: 13.0,
            dragging: false,
            mouse_pressed: false,
            press_x: 0.0,
            drag_start_x: 0.0,
            drag_start_value: 0.0,
            focused: false,
            editing: false,
            edit_text: String::new(),
            edit_cursor: 0,
            hovered: false,
            on_change: None,
            value_label,
        }
    }

    /// Set the display font size in logical pixels.
    pub fn with_font_size(mut self, s: f64) -> Self {
        self.font_size = s;
        self.value_label.set_font_size(s);
        self
    }

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

    fn format_value(&self) -> String {
        format_value(self.value, self.decimals)
    }

    fn sync_label(&mut self) {
        self.value_label.set_text(self.format_value());
    }

    fn apply_step_and_clamp(&self, raw: f64) -> f64 {
        let snapped = if self.step > 0.0 {
            (raw / self.step).round() * self.step
        } else {
            raw
        };
        snapped.clamp(self.min, self.max)
    }

    fn update_from_drag(&mut self, current_x: f64) {
        let delta = (current_x - self.drag_start_x) * self.speed;
        let raw   = self.drag_start_value + delta;
        self.value = self.apply_step_and_clamp(raw);
        self.sync_label();
        let v = self.value;
        if let Some(cb) = self.on_change.as_mut() { cb(v); }
    }

    fn enter_edit_mode(&mut self) {
        self.editing = true;
        self.edit_text = self.format_value();
        self.edit_cursor = self.edit_text.chars().count();
    }

    fn commit_edit(&mut self) {
        self.editing = false;
        if let Ok(raw) = self.edit_text.trim().parse::<f64>() {
            self.value = self.apply_step_and_clamp(raw);
        }
        // Always sync label back to actual value (parse success or failure).
        self.sync_label();
        let v = self.value;
        if let Some(cb) = self.on_change.as_mut() { cb(v); }
    }

    fn cancel_edit(&mut self) {
        self.editing = false;
        self.sync_label();
    }

    /// Convert a char-index cursor position to a byte offset in `edit_text`.
    fn cursor_byte_offset(&self, char_idx: usize) -> usize {
        self.edit_text
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.edit_text.len())
    }
}

// ── Widget impl ────────────────────────────────────────────────────────────

impl Widget for DragValue {
    fn type_name(&self) -> &'static str { "DragValue" }

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

    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, WIDGET_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let a = v.accent;

        if self.editing {
            // ── Edit-mode appearance ──────────────────────────────────────
            let bg = Color::rgba(a.r, a.g, a.b, 0.10);
            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
            ctx.fill();

            // Bright accent border signals active editing.
            ctx.set_stroke_color(Color::rgba(a.r, a.g, a.b, 0.80));
            ctx.set_line_width(1.5);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
            ctx.stroke();

            // Render current edit_text via the value label.
            self.value_label.set_text(self.edit_text.clone());
            let avail_w = (w - 8.0).max(1.0);
            let lsz = self.value_label.layout(Size::new(avail_w, h));
            let lx  = (w - lsz.width)  * 0.5;
            let ly  = (h - lsz.height) * 0.5;
            self.value_label.set_bounds(Rect::new(0.0, 0.0, lsz.width, lsz.height));
            ctx.save();
            ctx.translate(lx, ly);
            paint_subtree(&mut self.value_label, ctx);
            ctx.restore();

            // Draw cursor: measure text up to edit_cursor to find x position.
            let prefix: String = self.edit_text.chars().take(self.edit_cursor).collect();
            ctx.set_font(Arc::clone(&self.font));
            ctx.set_font_size(self.font_size);
            let prefix_w  = ctx.measure_text(&prefix).map(|m| m.width).unwrap_or(0.0);
            // Cursor sits at the right edge of the prefix inside the label's x offset.
            let text_x    = lx + (lsz.width - ctx.measure_text(&self.edit_text).map(|m| m.width).unwrap_or(lsz.width)) * 0.5;
            let cursor_x  = text_x + prefix_w;
            ctx.set_fill_color(Color::rgba(v.text_color.r, v.text_color.g, v.text_color.b, 0.85));
            ctx.begin_path();
            ctx.rect(cursor_x, ly + 2.0, 1.5, lsz.height - 4.0);
            ctx.fill();
        } else {
            // ── Normal drag-value appearance ──────────────────────────────
            let bg = if self.dragging {
                Color::rgba(a.r, a.g, a.b, 0.22)
            } else if self.hovered {
                Color::rgba(a.r, a.g, a.b, 0.14)
            } else {
                Color::rgba(a.r, a.g, a.b, 0.08)
            };
            let border = Color::rgba(a.r, a.g, a.b, 0.35);
            let arrow  = Color::rgba(a.r, a.g, a.b, 0.45);

            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
            ctx.fill();

            ctx.set_stroke_color(border);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
            ctx.stroke();

            // Arrow triangles as drag affordances.
            let mid      = h * 0.5;
            let tri_half = 4.0;
            let tri_w    = 6.0;
            ctx.set_fill_color(arrow);
            ctx.begin_path();
            ctx.move_to(ARROW_MARGIN, mid);
            ctx.line_to(ARROW_MARGIN + tri_w, mid - tri_half);
            ctx.line_to(ARROW_MARGIN + tri_w, mid + tri_half);
            ctx.close_path();
            ctx.fill();
            ctx.begin_path();
            ctx.move_to(w - ARROW_MARGIN, mid);
            ctx.line_to(w - ARROW_MARGIN - tri_w, mid - tri_half);
            ctx.line_to(w - ARROW_MARGIN - tri_w, mid + tri_half);
            ctx.close_path();
            ctx.fill();

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
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            // ── Keyboard (edit mode) ──────────────────────────────────────
            Event::KeyDown { key, .. } if self.editing => {
                match key {
                    Key::Char(c) => {
                        // Only allow numeric input: digits, '.', '-'.
                        if c.is_ascii_digit() || *c == '.' || (*c == '-' && self.edit_cursor == 0) {
                            let byte = self.cursor_byte_offset(self.edit_cursor);
                            self.edit_text.insert(byte, *c);
                            self.edit_cursor += 1;
                        }
                    }
                    Key::Backspace => {
                        if self.edit_cursor > 0 {
                            self.edit_cursor -= 1;
                            let byte = self.cursor_byte_offset(self.edit_cursor);
                            self.edit_text.remove(byte);
                        }
                    }
                    Key::Delete => {
                        let n = self.edit_text.chars().count();
                        if self.edit_cursor < n {
                            let byte = self.cursor_byte_offset(self.edit_cursor);
                            self.edit_text.remove(byte);
                        }
                    }
                    Key::ArrowLeft => {
                        if self.edit_cursor > 0 { self.edit_cursor -= 1; }
                    }
                    Key::ArrowRight => {
                        let n = self.edit_text.chars().count();
                        if self.edit_cursor < n { self.edit_cursor += 1; }
                    }
                    Key::Enter => { self.commit_edit(); }
                    Key::Escape => { self.cancel_edit(); }
                    _ => {}
                }
                EventResult::Consumed
            }

            // ── Mouse events ──────────────────────────────────────────────
            Event::MouseMove { pos } => {
                self.hovered = self.hit_test(*pos);
                if self.mouse_pressed && !self.editing {
                    let dx = (pos.x - self.press_x).abs();
                    if !self.dragging && dx >= DRAG_THRESHOLD {
                        // Confirm drag: anchor at original press so no dead-zone jump.
                        self.dragging         = true;
                        self.drag_start_x     = self.press_x;
                        self.drag_start_value = self.value;
                    }
                    if self.dragging {
                        self.update_from_drag(pos.x);
                        return EventResult::Consumed;
                    }
                }
                EventResult::Ignored
            }
            Event::MouseDown { button: MouseButton::Left, pos, .. } => {
                if self.editing {
                    // Already in edit mode — consume to keep focus, don't start drag.
                    return EventResult::Consumed;
                }
                self.mouse_pressed = true;
                self.dragging      = false;
                self.press_x       = pos.x;
                EventResult::Consumed
            }
            Event::MouseUp { button: MouseButton::Left, .. } => {
                let was_drag    = self.dragging;
                let was_pressed = self.mouse_pressed;
                self.dragging      = false;
                self.mouse_pressed = false;
                if was_pressed && !was_drag && !self.editing {
                    self.enter_edit_mode();
                }
                EventResult::Consumed
            }

            // ── Focus ─────────────────────────────────────────────────────
            Event::FocusGained => {
                self.focused = true;
                EventResult::Ignored
            }
            Event::FocusLost => {
                self.focused = false;
                if self.editing { self.commit_edit(); }
                self.dragging      = false;
                self.mouse_pressed = false;
                EventResult::Ignored
            }

            _ => EventResult::Ignored,
        }
    }
}
