//! `Slider` — a horizontal range slider with a draggable thumb.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

const TRACK_H: f64 = 4.0;
const THUMB_R: f64 = 9.0;
const WIDGET_H: f64 = 36.0;

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
    font: Arc<Font>,
    font_size: f64,
    dragging: bool,
    focused: bool,
    hovered: bool,
    on_change: Option<Box<dyn FnMut(f64)>>,
}

impl Slider {
    pub fn new(value: f64, min: f64, max: f64, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            value: value.clamp(min, max),
            min,
            max,
            step: (max - min) / 100.0,
            show_value: true,
            font,
            font_size: 12.0,
            dragging: false,
            focused: false,
            hovered: false,
            on_change: None,
        }
    }

    pub fn with_step(mut self, step: f64) -> Self { self.step = step; self }
    pub fn with_show_value(mut self, show: bool) -> Self { self.show_value = show; self }

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
    }

    fn fire(&mut self) {
        let v = self.value;
        if let Some(cb) = self.on_change.as_mut() { cb(v); }
    }

    /// Pixel X of the thumb center within the track area.
    fn thumb_x(&self) -> f64 {
        let track_left  = THUMB_R;
        let track_right = self.bounds.width - THUMB_R;
        let t = if self.max > self.min {
            (self.value - self.min) / (self.max - self.min)
        } else {
            0.0
        };
        track_left + t * (track_right - track_left)
    }

    fn value_from_x(&self, x: f64) -> f64 {
        let track_left  = THUMB_R;
        let track_right = self.bounds.width - THUMB_R;
        let t = ((x - track_left) / (track_right - track_left)).clamp(0.0, 1.0);
        let raw = self.min + t * (self.max - self.min);
        // Snap to step
        let snapped = (raw / self.step).round() * self.step;
        snapped.clamp(self.min, self.max)
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
        Size::new(available.width, WIDGET_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let cy = h * 0.5;

        // Track (background)
        ctx.set_fill_color(Color::rgb(0.85, 0.86, 0.88));
        ctx.begin_path();
        ctx.rounded_rect(THUMB_R, cy - TRACK_H * 0.5, w - THUMB_R * 2.0, TRACK_H, TRACK_H * 0.5);
        ctx.fill();

        // Track (filled portion up to thumb)
        let tx = self.thumb_x();
        if tx > THUMB_R {
            ctx.set_fill_color(Color::rgb(0.22, 0.45, 0.88));
            ctx.begin_path();
            ctx.rounded_rect(THUMB_R, cy - TRACK_H * 0.5, tx - THUMB_R, TRACK_H, TRACK_H * 0.5);
            ctx.fill();
        }

        // Focus ring
        if self.focused {
            ctx.set_stroke_color(Color::rgba(0.22, 0.45, 0.88, 0.45));
            ctx.set_line_width(2.0);
            ctx.begin_path();
            ctx.circle(tx, cy, THUMB_R + 3.0);
            ctx.stroke();
        }

        // Thumb
        let thumb_color = if self.dragging || self.focused {
            Color::rgb(0.16, 0.36, 0.72)
        } else if self.hovered {
            Color::rgb(0.26, 0.50, 0.92)
        } else {
            Color::rgb(0.22, 0.45, 0.88)
        };
        ctx.set_fill_color(thumb_color);
        ctx.begin_path();
        ctx.circle(tx, cy, THUMB_R);
        ctx.fill();

        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.circle(tx, cy, THUMB_R - 3.5);
        ctx.fill();

        // Value label
        if self.show_value {
            let label = if self.step >= 1.0 {
                format!("{:.0}", self.value)
            } else if self.step >= 0.1 {
                format!("{:.1}", self.value)
            } else {
                format!("{:.2}", self.value)
            };
            ctx.set_font(Arc::clone(&self.font));
            ctx.set_font_size(self.font_size);
            ctx.set_fill_color(Color::rgb(0.4, 0.4, 0.42));
            if let Some(m) = ctx.measure_text(&label) {
                let lx = w - m.width;
                let ly = self.font_size * 0.5 + 1.0;
                ctx.fill_text(&label, lx, ly);
            }
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
