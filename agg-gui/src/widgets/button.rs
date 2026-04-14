//! `Button` — a clickable button with hover, pressed, and focus states.
//!
//! # Composition
//!
//! Button is fully compositional: it always has exactly one child widget, a
//! [`Label`], which is responsible for rendering the button's text.  The
//! [`paint_subtree`] machinery handles the Label automatically after
//! [`Button::paint`] draws the background.
//!
//! ```text
//! Button (background + focus ring)
//!   └── Label (text, tight bounds, centred within button)
//! ```
//!
//! `Label::layout` returns tight text bounds.  `Button::layout` centres the
//! label within the button area.  Because [`Label::hit_test`] returns `false`,
//! the Label is invisible to the hit-test and event-routing system; the Button
//! retains full ownership of focus and click events.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::label::{Label, LabelAlign};

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
    /// Always exactly one child: the `Label` for the button's text.
    children: Vec<Box<dyn Widget>>,
    /// Source of truth for the label text, kept so `build_label` can rebuild.
    label_text: String,
    font: Arc<Font>,
    font_size: f64,
    pub theme: ButtonTheme,
    on_click: Option<Box<dyn FnMut()>>,

    hovered: bool,
    pressed: bool,
    focused: bool,
}

impl Button {
    /// Create a button with the given label.
    pub fn new(label: impl Into<String>, font: Arc<Font>) -> Self {
        let label_text: String = label.into();
        let font_size = 14.0;
        let theme = ButtonTheme::default();
        let child = Self::build_label(&label_text, &font, font_size, &theme);
        Self {
            bounds: Rect::default(),
            children: vec![child],
            label_text,
            font,
            font_size,
            theme,
            on_click: None,
            hovered: false,
            pressed: false,
            focused: false,
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self.children[0] = Self::build_label(&self.label_text, &self.font, size, &self.theme);
        self
    }

    pub fn with_theme(mut self, theme: ButtonTheme) -> Self {
        self.theme = theme;
        self.children[0] = Self::build_label(&self.label_text, &self.font, self.font_size, &self.theme);
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

    /// Construct a label child from the button's current state.
    ///
    /// Called from `new()`, `with_theme()`, and `with_font_size()` so the
    /// child always reflects the button's configuration.
    fn build_label(
        text:      &str,
        font:      &Arc<Font>,
        font_size: f64,
        theme:     &ButtonTheme,
    ) -> Box<dyn Widget> {
        Box::new(
            Label::new(text, Arc::clone(font))
                .with_font_size(font_size)
                .with_color(theme.label_color)
                .with_align(LabelAlign::Center),
        )
    }
}

impl Widget for Button {
    fn type_name(&self) -> &'static str { "Button" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, bounds: Rect) { self.bounds = bounds; }

    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn is_focusable(&self) -> bool { true }

    fn layout(&mut self, available: Size) -> Size {
        let height = (self.font_size * 2.4).max(28.0);
        let size = Size::new(available.width, height);
        // Label returns tight text bounds; centre it within the button area.
        let label_size = self.children[0].layout(size);
        let label_x = ((size.width  - label_size.width)  * 0.5).max(0.0);
        let label_y = ((size.height - label_size.height) * 0.5).max(0.0);
        self.children[0].set_bounds(Rect::new(label_x, label_y, label_size.width, label_size.height));
        size
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let r = self.theme.border_radius;

        // Focus ring (behind the button surface)
        if self.focused {
            let ring = self.theme.focus_ring_width;
            ctx.set_stroke_color(self.theme.focus_ring_color);
            ctx.set_line_width(ring);
            ctx.begin_path();
            ctx.rounded_rect(-ring * 0.5, -ring * 0.5, w + ring, h + ring, r + ring * 0.5);
            ctx.stroke();
        }

        // Background — color depends on interaction state.
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

        // Text is NOT drawn here. `paint_subtree` recurses into the Label
        // child automatically after this method returns.
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

    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("label",     self.label_text.clone()),
            ("font_size", format!("{:.1}", self.font_size)),
        ]
    }
}
