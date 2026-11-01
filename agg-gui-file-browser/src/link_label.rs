//! Text drawn as a link - underlined until the pointer is over it, so the hover itself shows the
//! link is live - that runs its action on a left click, with an optional tooltip saying where it
//! goes. Each folder in the breadcrumb trail (`bread_crumb.rs`) is one. Unlike agg-gui's
//! `Hyperlink` it draws in the colour it is given (the browser's text colour) rather than the
//! theme's link blue, and its line box is one em tall with the text centred on it
//! ([`centered_text_baseline`]), so it lines up with the labels and buttons beside it.

use std::sync::Arc;

use agg_gui::cursor::{set_cursor_icon, CursorIcon};
use agg_gui::text::{measure_advance, measure_text_metrics, Font};
use agg_gui::{
    Color, DrawCtx, Event, EventResult, HAnchor, MouseButton, Rect, Size, VAnchor, Widget,
};

use crate::fitted_text::centered_text_baseline;

/// A clickable text link.
pub struct LinkLabel {
    id: String,
    text: String,
    font: Arc<Font>,
    font_size: f64,
    color: Color,
    hovered: bool,
    pressed: bool,
    on_click: Box<dyn FnMut()>,
    h_anchor: HAnchor,
    v_anchor: VAnchor,
    tooltip: Option<String>,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl LinkLabel {
    /// A link reading `text` at `font_size` in `color`, named `id`.
    pub fn new(
        id: impl Into<String>,
        text: impl Into<String>,
        font: Arc<Font>,
        font_size: f64,
        color: Color,
        on_click: impl FnMut() + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            font,
            font_size,
            color,
            hovered: false,
            pressed: false,
            on_click: Box::new(on_click),
            h_anchor: HAnchor::FIT,
            v_anchor: VAnchor::FIT,
            tooltip: None,
            bounds: Rect::default(),
            children: Vec::new(),
        }
    }

    /// The horizontal anchor.
    pub fn with_h_anchor(mut self, h_anchor: HAnchor) -> Self {
        self.h_anchor = h_anchor;
        self
    }

    /// The vertical anchor.
    pub fn with_v_anchor(mut self, v_anchor: VAnchor) -> Self {
        self.v_anchor = v_anchor;
        self
    }

    /// The tooltip shown while the pointer rests on the link.
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// The colour the link's text and underline draw in.
    pub fn color(&self) -> Color {
        self.color
    }

    /// Whether the underline is drawn: while the pointer is not over the link.
    pub fn underlined(&self) -> bool {
        !self.hovered
    }
}

impl Widget for LinkLabel {
    fn type_name(&self) -> &'static str {
        "LinkLabel"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.id)
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn h_anchor(&self) -> HAnchor {
        self.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.v_anchor
    }
    fn tooltip_text(&self) -> Option<&str> {
        self.tooltip.as_deref()
    }
    fn layout(&mut self, _available: Size) -> Size {
        let width = measure_advance(&self.font, &self.text, self.font_size).ceil();
        // Fits its text: as wide as the text, one em tall.
        let line = measure_text_metrics(&self.font, &self.text, self.font_size).line_height;
        Size::new(width, line)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let height = self.bounds.height;
        let baseline = centered_text_baseline(&self.font, self.font_size, height);
        ctx.set_fill_color(self.color);
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        ctx.fill_text(&self.text, 0.0, baseline);
        if self.underlined() {
            // One pixel under the baseline, the text's width.
            ctx.begin_path();
            ctx.rect(0.0, baseline - 2.0, self.bounds.width, 1.0);
            ctx.fill();
        }
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseEnter => {
                self.hovered = true;
                agg_gui::animation::request_draw();
                EventResult::Ignored
            }
            Event::MouseLeave => {
                self.hovered = false;
                self.pressed = false;
                agg_gui::animation::request_draw();
                EventResult::Ignored
            }
            // The pointing hand tells the user the text is clickable.
            Event::MouseMove { .. } => {
                set_cursor_icon(CursorIcon::PointingHand);
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Left,
                ..
            } => {
                self.pressed = true;
                EventResult::Consumed
            }
            Event::MouseUp {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                let inside = pos.x >= 0.0
                    && pos.y >= 0.0
                    && pos.x <= self.bounds.width
                    && pos.y <= self.bounds.height;
                if std::mem::take(&mut self.pressed) && inside {
                    (self.on_click)();
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
    /// Sets the text colour (a link that flashes fades it).
    fn set_label_color(&mut self, color: Color) {
        if self.color != color {
            self.color = color;
            agg_gui::animation::request_draw();
        }
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("text", self.text.clone()),
            ("underlined", self.underlined().to_string()),
            // 0-255, the byte tests compare.
            (
                "text_alpha",
                ((self.color.a * 255.0).round() as u8).to_string(),
            ),
        ]
    }
}
