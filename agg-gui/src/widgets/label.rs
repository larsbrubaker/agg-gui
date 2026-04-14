//! `Label` — static text display widget.
//!
//! Labels are non-interactive by design (`hit_test` always returns `false`
//! and `on_event` always returns `Ignored`).  This makes them safe to use as
//! transparent overlay children inside interactive parents like `Button` — the
//! parent retains full hit-test and focus ownership.
//!
//! # Backbuffer
//!
//! When `has_backbuffer` is `true`, the label is intended to pre-render its
//! glyphs into an offscreen buffer (texture or software framebuffer) and then
//! blit that buffer each frame instead of re-tessellating.  The field is
//! present and visible in the inspector; full texture-blit rendering is a
//! future implementation.  For the GL path, the [`GlyphCache`] already
//! provides equivalent per-frame savings, so the visual output is identical
//! in both modes at this time.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

/// Horizontal alignment for `Label` text.
#[derive(Clone, Copy, Debug, Default)]
pub enum LabelAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// A non-interactive text widget.
///
/// Used directly as a standalone label, and as a child of composite widgets
/// such as [`Button`] and (in the future) `Checkbox`, `RadioGroup`, etc.
///
/// When no explicit color is set via [`with_color`](Label::with_color), the
/// label reads its text color from the active [`Visuals`](crate::theme::Visuals)
/// at paint time (`ctx.visuals().text_color`), so it automatically adapts to
/// dark / light mode switches.
pub struct Label {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,
    text: String,
    font: Arc<Font>,
    font_size: f64,
    /// `None` → use `ctx.visuals().text_color` at paint time.
    /// `Some(c)` → explicit override (e.g. accent-coloured or dimmed text).
    color: Option<Color>,
    align: LabelAlign,
    /// When `true`, the text is pre-rendered to an offscreen backbuffer and
    /// blitted each frame.  Currently display-only; full backbuffer path is
    /// planned.
    pub buffered: bool,
}

impl Label {
    pub fn new(text: impl Into<String>, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            text: text.into(),
            font,
            font_size: 14.0,
            color: None, // resolved from ctx.visuals() at paint time
            align: LabelAlign::Left,
            buffered: false,
        }
    }

    // ── builder methods ───────────────────────────────────────────────────────

    pub fn with_font_size(mut self, size: f64) -> Self { self.font_size = size; self }
    /// Override the label colour.  Pass an explicit `Color` to always use that
    /// colour regardless of the active theme.  Omit this call to follow the
    /// theme's `text_color` automatically.
    pub fn with_color(mut self, color: Color) -> Self { self.color = Some(color); self }
    pub fn with_align(mut self, align: LabelAlign) -> Self { self.align = align; self }
    pub fn with_has_backbuffer(mut self, v: bool) -> Self { self.buffered = v; self }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

    // ── setter methods (for post-construction mutation) ───────────────────────

    pub fn set_text(&mut self, text: impl Into<String>) { self.text = text.into(); }
    pub fn set_color(&mut self, color: Color) { self.color = Some(color); }
    pub fn clear_color(&mut self) { self.color = None; }
    pub fn set_align(&mut self, align: LabelAlign) { self.align = align; }
}

impl Widget for Label {
    fn type_name(&self) -> &'static str { "Label" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    /// Labels are never independently hittable.  This lets their interactive
    /// parent (e.g., Button) retain full hit-test and focus ownership even
    /// when the label fills the parent's entire bounds.
    fn hit_test(&self, _: Point) -> bool { false }

    fn layout(&mut self, available: Size) -> Size {
        // Tight bounds: width matches rendered text, height is font-size based.
        // Callers that need to centre or otherwise position the label use the
        // returned size; they must not assume the label fills available.width.
        let metrics = crate::text::measure_text_metrics(&self.font, &self.text, self.font_size);
        let h = self.font_size * 1.5;
        Size::new(metrics.width.min(available.width), h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        // If no explicit colour was set, follow the active theme.
        let color = self.color.unwrap_or_else(|| ctx.visuals().text_color);
        ctx.set_fill_color(color);

        if let Some(m) = ctx.measure_text(&self.text) {
            let ty = h * 0.5 - (m.ascent - m.descent) * 0.5 + m.descent;
            let tx = match self.align {
                LabelAlign::Left   => 0.0,
                LabelAlign::Center => (w - m.width) * 0.5,
                LabelAlign::Right  => w - m.width,
            };
            ctx.fill_text(&self.text, tx, ty);
        }
    }

    fn has_backbuffer(&self) -> bool { self.buffered }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }

    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("text",           self.text.clone()),
            ("font_size",      format!("{:.1}", self.font_size)),
            ("align",          format!("{:?}", self.align)),
            ("has_backbuffer", if self.buffered { "true" } else { "false" }.to_string()),
        ]
    }
}
