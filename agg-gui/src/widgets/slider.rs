//! `Slider` — a horizontal range slider with a draggable thumb.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::{Widget, paint_subtree};
use crate::widgets::label::{Label, LabelAlign};

const TRACK_H: f64 = 4.0;
const THUMB_R: f64 = 7.0;
/// Total widget height.  Needs to fit the thumb (diameter `2 * THUMB_R`)
/// plus a little breathing room for the focus ring — `22 px` keeps rows
/// compact in settings-style panels while still being easy to grab.
const WIDGET_H: f64 = 22.0;
/// Default pixel budget reserved on the right for the numeric value
/// label.  Wide enough for 4-5 glyphs at the slider's default font
/// size.  Set to `0.0` via [`Slider::with_show_value(false)`] to hide.
const VALUE_W:  f64 = 44.0;
/// Gap between the track's right edge and the value label's left edge.
const VALUE_GAP: f64 = 6.0;

/// A horizontal slider for a `f64` value within `[min, max]`.
pub struct Slider {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    show_value: bool,
    /// Fixed decimals for the value label — overrides the step-based
    /// auto-format when `Some`.  Matches `DragValue::with_decimals`.
    decimals: Option<usize>,
    font: Arc<Font>,
    font_size: f64,
    dragging: bool,
    focused: bool,
    hovered: bool,
    on_change: Option<Box<dyn FnMut(f64)>>,
    /// Optional external mirror of `value`.  When `Some`, `layout()` re-reads
    /// the cell every frame so a second widget that writes the same cell
    /// drives this slider live; `set_value` writes back.  Mirrors the
    /// `ToggleSwitch::with_state_cell` pattern — the cell is the source-of-
    /// truth so multiple widgets can reflect the same value bidirectionally.
    value_cell: Option<Rc<Cell<f64>>>,
    /// Backbuffered Label that renders the numeric value.  Updated in
    /// `layout()` with the current formatted value so the text follows
    /// drags live.  Uses the same proven text-cache path every other
    /// Label in the app does, which matters: previous attempts to
    /// render the value via direct `ctx.fill_text` were unreliable on
    /// some rendering paths.
    value_label: Label,
    /// Tracks the string last pushed into `value_label` so we only
    /// invalidate its cache when the displayed value actually changes.
    last_value_text: String,
}

impl Slider {
    pub fn new(value: f64, min: f64, max: f64, font: Arc<Font>) -> Self {
        let v = value.clamp(min, max);
        let font_size = 12.0;
        let value_label = Label::new("", Arc::clone(&font))
            .with_font_size(font_size)
            .with_align(LabelAlign::Right);
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            value: v,
            min,
            max,
            step: (max - min) / 100.0,
            show_value: true,
            decimals: None,
            font,
            font_size,
            dragging: false,
            focused: false,
            hovered: false,
            on_change: None,
            value_cell: None,
            value_label,
            last_value_text: String::new(),
        }
    }

    pub fn with_step(mut self, step: f64) -> Self { self.step = step; self }

    /// Bind this slider's value to an external `Rc<Cell<f64>>`.
    ///
    /// The cell becomes the source-of-truth: `layout()` reads it every
    /// frame so any other widget (or code path) that writes the cell
    /// will drive this slider live; drag interactions here write back
    /// to the cell too.  Pattern mirrors `ToggleSwitch::with_state_cell`.
    pub fn with_value_cell(mut self, cell: Rc<Cell<f64>>) -> Self {
        self.value = cell.get().clamp(self.min, self.max);
        self.value_cell = Some(cell);
        self
    }
    pub fn with_show_value(mut self, show: bool) -> Self { self.show_value = show; self }

    /// Force a specific decimal count for the numeric value label.  When
    /// unset, the format falls back to a heuristic based on `step`.
    pub fn with_decimals(mut self, decimals: usize) -> Self {
        self.decimals = Some(decimals); self
    }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

    pub fn on_change(mut self, cb: impl FnMut(f64) + 'static) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }

    pub fn value(&self) -> f64 { self.value }

    pub fn set_value(&mut self, v: f64) {
        self.value = v.clamp(self.min, self.max);
        if let Some(cell) = &self.value_cell { cell.set(self.value); }
    }

    fn fire(&mut self) {
        let v = self.value;
        if let Some(cell) = &self.value_cell { cell.set(v); }
        if let Some(cb) = self.on_change.as_mut() { cb(v); }
    }

    /// Pixel X of the track's right edge.  The value label (when shown)
    /// lives in a reserved strip to the right of this, outside the track
    /// so a thumb at max doesn't overdraw the digits.
    fn track_right(&self) -> f64 {
        let reserved = if self.show_value { VALUE_W + VALUE_GAP } else { 0.0 };
        (self.bounds.width - reserved - THUMB_R).max(THUMB_R + 1.0)
    }

    /// Pixel X of the thumb center within the track area.
    fn thumb_x(&self) -> f64 {
        let track_left  = THUMB_R;
        let track_right = self.track_right();
        let t = if self.max > self.min {
            (self.value - self.min) / (self.max - self.min)
        } else {
            0.0
        };
        track_left + t * (track_right - track_left)
    }

    fn value_from_x(&self, x: f64) -> f64 {
        let track_left  = THUMB_R;
        let track_right = self.track_right();
        let t = ((x - track_left) / (track_right - track_left)).clamp(0.0, 1.0);
        let raw = self.min + t * (self.max - self.min);
        // Snap to step
        let snapped = (raw / self.step).round() * self.step;
        snapped.clamp(self.min, self.max)
    }

    /// Format `self.value` using `decimals` if set, otherwise heuristic
    /// based on `step`.
    fn format_value(&self) -> String {
        if let Some(d) = self.decimals {
            return format!("{:.*}", d, self.value);
        }
        if self.step >= 1.0 {
            format!("{:.0}", self.value)
        } else if self.step >= 0.1 {
            format!("{:.1}", self.value)
        } else if self.step >= 0.01 {
            format!("{:.2}", self.value)
        } else {
            format!("{:.3}", self.value)
        }
    }
}

impl Widget for Slider {
    fn type_name(&self) -> &'static str { "Slider" }
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
        // Re-read external cell every frame — another widget (e.g. the
        // System window's slider) may have written a new value.  Skip
        // while dragging so the user's in-flight drag isn't fought
        // back by rounding inside the source cell.
        if !self.dragging {
            if let Some(cell) = &self.value_cell {
                self.value = cell.get().clamp(self.min, self.max);
            }
        }

        // Refresh the value-label text only when the displayed string
        // actually changed — Label's `set_text` invalidates its cache
        // so we want to skip this when the value is unchanged (e.g.
        // idle frames between drags).
        if self.show_value {
            let new_text = self.format_value();
            if new_text != self.last_value_text {
                self.value_label.set_text(new_text.clone());
                self.last_value_text = new_text;
            }
            // Size the label to exactly the reserved strip; right-align
            // anchors the digits to the widget's right edge.
            let lh = self.font_size * 1.5;
            let _ = self.value_label.layout(Size::new(VALUE_W, lh));
            self.value_label.set_bounds(Rect::new(0.0, 0.0, VALUE_W, lh));
        }

        Size::new(available.width, WIDGET_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let cy = h * 0.5;

        let track_right = self.track_right();
        let track_w     = (track_right - THUMB_R).max(0.0);

        // Track (background)
        ctx.set_fill_color(v.track_bg);
        ctx.begin_path();
        ctx.rounded_rect(THUMB_R, cy - TRACK_H * 0.5, track_w, TRACK_H, TRACK_H * 0.5);
        ctx.fill();

        // Track (filled portion up to thumb)
        let tx = self.thumb_x();
        if tx > THUMB_R {
            ctx.set_fill_color(v.accent);
            ctx.begin_path();
            ctx.rounded_rect(THUMB_R, cy - TRACK_H * 0.5, tx - THUMB_R, TRACK_H, TRACK_H * 0.5);
            ctx.fill();
        }

        // Focus ring
        if self.focused {
            ctx.set_stroke_color(v.accent_focus);
            ctx.set_line_width(2.0);
            ctx.begin_path();
            ctx.circle(tx, cy, THUMB_R + 3.0);
            ctx.stroke();
        }

        // Thumb
        let thumb_color = if self.dragging || self.focused {
            v.accent_pressed
        } else if self.hovered {
            v.accent_hovered
        } else {
            v.accent
        };
        ctx.set_fill_color(thumb_color);
        ctx.begin_path();
        ctx.circle(tx, cy, THUMB_R);
        ctx.fill();

        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.circle(tx, cy, THUMB_R - 2.5);
        ctx.fill();

        // Value label — composed via backbuffered Label so it uses the
        // same text-raster path as every other label in the app.  The
        // Label is right-aligned inside its box and positioned in the
        // reserved strip to the right of the track.
        if self.show_value {
            self.value_label.set_color(v.text_color);
            let lb = self.value_label.bounds();
            let strip_left = track_right + VALUE_GAP;
            let ly = cy - lb.height * 0.5;
            self.value_label.set_bounds(Rect::new(strip_left, ly, lb.width, lb.height));
            ctx.save();
            ctx.translate(strip_left, ly);
            paint_subtree(&mut self.value_label, ctx);
            ctx.restore();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.hovered = self.hit_test(*pos);
                if self.dragging {
                    self.value = self.value_from_x(pos.x);
                    self.fire();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown { button: MouseButton::Left, pos, .. } => {
                self.dragging = true;
                self.value = self.value_from_x(pos.x);
                self.fire();
                EventResult::Consumed
            }
            Event::MouseUp { button: MouseButton::Left, .. } => {
                self.dragging = false;
                EventResult::Consumed
            }
            Event::KeyDown { key, .. } => {
                let changed = match key {
                    Key::ArrowLeft  => { self.value = (self.value - self.step).clamp(self.min, self.max); true }
                    Key::ArrowRight => { self.value = (self.value + self.step).clamp(self.min, self.max); true }
                    Key::ArrowDown  => { self.value = (self.value - self.step * 10.0).clamp(self.min, self.max); true }
                    Key::ArrowUp    => { self.value = (self.value + self.step * 10.0).clamp(self.min, self.max); true }
                    _ => false,
                };
                if changed { self.fire(); EventResult::Consumed } else { EventResult::Ignored }
            }
            Event::FocusGained => { self.focused = true;  EventResult::Ignored }
            Event::FocusLost   => { self.focused = false; self.dragging = false; EventResult::Ignored }
            _ => EventResult::Ignored,
        }
    }
}
