//! `ProgressBar` — a read-only horizontal progress indicator.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

const BAR_H: f64 = 18.0;
const WIDGET_H: f64 = 24.0;

/// Inspector-visible properties of a [`ProgressBar`].
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[derive(Clone, Debug)]
pub struct ProgressBarProps {
    /// Progress in `[0.0, 1.0]`.
    pub value: f64,
    pub show_text: bool,
    pub font_size: f64,
    pub fill_color: Option<Color>,
}

impl Default for ProgressBarProps {
    fn default() -> Self {
        Self {
            value: 0.0,
            show_text: true,
            font_size: 11.0,
            fill_color: None,
        }
    }
}

/// A horizontal progress bar. `value` is in `[0.0, 1.0]`.
pub struct ProgressBar {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,
    pub props: ProgressBarProps,
    font: Arc<Font>,
}

impl ProgressBar {
    pub fn new(value: f64, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            props: ProgressBarProps {
                value: value.clamp(0.0, 1.0),
                ..ProgressBarProps::default()
            },
            font,
        }
    }

    pub fn with_show_text(mut self, show: bool) -> Self {
        self.props.show_text = show;
        self
    }
    pub fn with_fill_color(mut self, color: Color) -> Self {
        self.props.fill_color = Some(color);
        self
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }

    pub fn set_value(&mut self, v: f64) {
        self.props.value = v.clamp(0.0, 1.0);
    }

    pub fn value(&self) -> f64 {
        self.props.value
    }
}

impl Widget for ProgressBar {
    fn type_name(&self) -> &'static str {
        "ProgressBar"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    #[cfg(feature = "reflect")]
    fn as_reflect(&self) -> Option<&dyn bevy_reflect::Reflect> {
        Some(&self.props)
    }
    #[cfg(feature = "reflect")]
    fn as_reflect_mut(&mut self) -> Option<&mut dyn bevy_reflect::Reflect> {
        Some(&mut self.props)
    }

    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }
    fn min_size(&self) -> Size {
        self.base.min_size
    }
    fn max_size(&self) -> Size {
        self.base.max_size
    }

    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, WIDGET_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let bar_y = (h - BAR_H) * 0.5;
        let r = BAR_H * 0.5;

        // Track
        ctx.set_fill_color(v.track_bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, bar_y, w, BAR_H, r);
        ctx.fill();

        // Fill — use explicit fill_color if set, otherwise fall back to accent.
        let fill_color = self.props.fill_color.unwrap_or(v.accent);
        let fill_w = (w * self.props.value).max(0.0);
        if fill_w >= 1.0 {
            ctx.set_fill_color(fill_color);
            ctx.begin_path();
            ctx.rounded_rect(0.0, bar_y, fill_w, BAR_H, r);
            ctx.fill();
        }

        // Percentage text centered over bar
        if self.props.show_text {
            let label = format!("{:.0}%", self.props.value * 100.0);
            ctx.set_font(Arc::clone(&self.font));
            ctx.set_font_size(self.props.font_size);
            // Text color: always use theme text contrasted against the bar.
            let mid = w * 0.5;
            let text_color = if fill_w > mid {
                Color::rgba(1.0, 1.0, 1.0, 0.9)
            } else {
                v.text_dim
            };
            ctx.set_fill_color(text_color);
            if let Some(m) = ctx.measure_text(&label) {
                let tx = (w - m.width) * 0.5;
                let ty = bar_y + BAR_H * 0.5 - (m.ascent - m.descent) * 0.5;
                ctx.fill_text(&label, tx, ty);
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
