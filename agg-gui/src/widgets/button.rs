//! `Button` — a clickable button with hover, pressed, and focus states.
//!
//! The label is rendered directly in `paint()` using `GfxCtx::fill_text`.
//! Phase 5 will replace this with a proper `TextWidget` child once flex layout
//! is in place.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::Widget;

/// A theme for [`Button`] visual states.
pub struct ButtonTheme {
    pub background:         Color,
    pub background_hovered: Color,
    pub background_pressed: Color,
    pub label_color:        Color,
    pub border_radius:      f64,
    pub focus_ring_color:   Color,
    pub focus_ring_width:   f64,
}

impl Default for ButtonTheme {
    fn default() -> Self {
        Self {
            background:         Color::rgb(0.22, 0.45, 0.88),
            background_hovered: Color::rgb(0.30, 0.52, 0.92),
            background_pressed: Color::rgb(0.16, 0.36, 0.72),
            label_color:        Color::white(),
            border_radius:      6.0,
            focus_ring_color:   Color::rgba(0.22, 0.45, 0.88, 0.55),
            focus_ring_width:   2.5,
        }
    }
}

/// A clickable button.
///
/// Build with [`Button::new`] and optionally chain builder methods.
pub struct Button {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty — label is drawn directly
    label: String,
    font: Arc<Font>,
    font_size: f64,
    pub theme: ButtonTheme,
    on_click: Option<Box<dyn FnMut()>>,

    hovered: bool,
    pressed: bool,
    focused: bool,
}

impl Button {
    /// Create a button with the given label. The font is used for label sizing
    /// and rendering.
    pub fn new(label: impl Into<String>, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            label: label.into(),
            font,
            font_size: 14.0,
            theme: ButtonTheme::default(),
            on_click: None,
            hovered: false,
            pressed: false,
            focused: false,
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }

    pub fn with_theme(mut self, theme: ButtonTheme) -> Self {
        self.theme = theme;
        self
    }

    pub fn on_click(mut self, cb: impl FnMut() + 'static) -> Self {
        self.on_click = Some(Box::new(cb));
        self
    }

    fn fire_click(&mut self) {
        if let Some(cb) = self.on_click.as_mut() {
            cb();
        }
    }
}

impl Widget for Button {
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, bounds: Rect) { self.bounds = bounds; }

    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn is_focusable(&self) -> bool { true }

    fn layout(&mut self, available: Size) -> Size {
        // Fixed height based on font size; full available width.
        let height = (self.font_size * 2.4).max(28.0);
        Size::new(available.width, height)
    }

    fn paint(&mut self, ctx: &mut GfxCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let r = self.theme.border_radius;

        // Focus ring (painted behind the button)
        if self.focused {
            let ring = self.theme.focus_ring_width;
            ctx.set_stroke_color(self.theme.focus_ring_color);
            ctx.set_line_width(ring);
            ctx.begin_path();
            ctx.rounded_rect(-ring * 0.5, -ring * 0.5, w + ring, h + ring, r + ring * 0.5);
            ctx.stroke();
        }

        // Background
        let bg = if self.pressed {
            self.theme.background_pressed
        } else if self.hovered {
            self.theme.background_hovered
        } else {
            self.theme.background
        };
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.fill();

        // Label — centered
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        ctx.set_fill_color(self.theme.label_color);
        if let Some(m) = ctx.measure_text(&self.label) {
            let tx = (w - m.width) * 0.5;
            let ty = h * 0.5 - (m.ascent - m.descent) * 0.5 + m.descent;
            ctx.fill_text(&self.label, tx, ty);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.hovered = self.hit_test(*pos);
                if !self.hovered {
                    self.pressed = false;
                }
                EventResult::Ignored
            }
            Event::MouseDown { button: MouseButton::Left, .. } => {
                self.pressed = true;
                EventResult::Consumed
            }
            Event::MouseUp { button: MouseButton::Left, .. } => {
                let was_pressed = self.pressed;
                self.pressed = false;
                if was_pressed && self.hovered {
                    self.fire_click();
                }
                EventResult::Consumed
            }
            Event::KeyDown { key, .. } => {
                use crate::event::Key;
                match key {
                    Key::Enter | Key::Char(' ') => {
                        self.fire_click();
                        EventResult::Consumed
                    }
                    _ => EventResult::Ignored,
                }
            }
            Event::FocusGained => {
                self.focused = true;
                EventResult::Ignored
            }
            Event::FocusLost => {
                self.focused = false;
                self.pressed = false;
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }
}
