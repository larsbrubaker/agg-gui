//! `Label` — static text display widget.
//!
//! Labels are non-interactive by design (`hit_test` always returns `false`
//! and `on_event` always returns `Ignored`).  This makes them safe to use as
//! transparent overlay children inside interactive parents like `Button` — the
//! parent retains full hit-test and focus ownership.
//!
//! # Backbuffer
//!
//! When `buffered` is `true` AND the active `DrawCtx` supports image blitting
//! (`ctx.has_image_blit()` returns `true`, i.e. the software `GfxCtx` path),
//! the label pre-renders its glyphs into an offscreen `Framebuffer` on the
//! first `paint()` call — or whenever `text`, `font_size`, `color`, or `bounds`
//! change — and blits the cached pixels every subsequent frame via
//! `ctx.draw_image_rgba()`.  No font shaping or rasterisation occurs on cache
//! hits.
//!
//! On the GL path (`has_image_blit()` → false) the label falls back to the
//! direct `fill_text()` call; the GL path's `GlyphCache` provides equivalent
//! glyph-level savings there.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

/// Compare two `Color` values for equality within a small epsilon.
///
/// Used to detect cache invalidation when the resolved text color changes
/// (e.g. on a theme switch).  Colors whose components differ by less than
/// 1/255 (~0.004) are considered equal so that floating-point noise in
/// visuals does not force unnecessary re-renders.
fn colors_equal(a: Color, b: Color) -> bool {
    (a.r - b.r).abs() < 0.004_f32
        && (a.g - b.g).abs() < 0.004_f32
        && (a.b - b.b).abs() < 0.004_f32
        && (a.a - b.a).abs() < 0.004_f32
}

/// Break `text` into lines that each fit within `max_width` pixels at the given
/// font size.  Explicit `\n` characters always produce a new line.  Returns at
/// least one entry (possibly an empty string for blank text).
fn wrap_text(font: &Arc<Font>, text: &str, font_size: f64, max_width: f64) -> Vec<String> {
    use crate::text::measure_text_metrics;
    let mut result = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.trim().is_empty() {
            // Preserve explicit blank lines.
            result.push(String::new());
            continue;
        }
        let mut current: String = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else {
                let candidate = format!("{current} {word}");
                let w = measure_text_metrics(font, &candidate, font_size).width;
                if w <= max_width {
                    current = candidate;
                } else {
                    result.push(std::mem::replace(&mut current, word.to_string()));
                }
            }
        }
        if !current.is_empty() {
            result.push(current);
        }
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}

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
    /// When `true` (the default), and the active DrawCtx supports blitting
    /// (`has_image_blit()`), text is rendered once to an offscreen framebuffer
    /// and blitted each frame.  Set to `false` only for text that changes
    /// every frame (e.g. live counters) where caching adds overhead with no
    /// benefit.
    pub buffered: bool,
    /// When `true`, long lines are broken at word boundaries to fit
    /// `available.width`.  The label height expands to fit all lines.
    /// Disabled by default; enable with `.with_wrap(true)`.
    wrap: bool,

    // ── Layout measurement cache ──────────────────────────────────────────────
    /// Cached text advance width from last `measure_advance()` call.
    /// Avoids calling `rustybuzz::shape()` every frame — only re-measures
    /// when `text` or `font_size` changes.
    layout_text: String,
    layout_font_size: f64,
    layout_width: f64,
    /// Width used for the last word-wrap computation.
    wrap_at_width: f64,
    /// Lines produced by the last word-wrap computation.
    wrapped_lines: Vec<String>,

    // ── Backbuffer cache ──────────────────────────────────────────────────────
    /// Cached pixel data (top-row first, RGBA8).  `None` until first render.
    cache_pixels: Option<Vec<u8>>,
    /// Framebuffer dimensions used for the last cache render.
    cache_w: u32,
    cache_h: u32,
    /// Text, font_size, and color as of last cache render — used for
    /// invalidation detection.
    cache_text: String,
    cache_font_size: f64,
    /// The resolved color (possibly from visuals) used for the last cache render.
    cache_color: Color,
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
            buffered: true,
            wrap: false,
            layout_text: String::new(),
            layout_font_size: 0.0,
            layout_width: 0.0,
            wrap_at_width: -1.0,
            wrapped_lines: Vec::new(),
            cache_pixels: None,
            cache_w: 0,
            cache_h: 0,
            cache_text: String::new(),
            cache_font_size: 0.0,
            cache_color: Color::black(),
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
    /// Enable or disable word-wrapping.  When `true`, long lines are broken at
    /// word boundaries to fit the available width; the label height expands to
    /// accommodate all lines.  Newlines in the text are always honoured.
    pub fn with_wrap(mut self, wrap: bool) -> Self { self.wrap = wrap; self }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

    // ── getter methods ────────────────────────────────────────────────────────

    /// Return the current label text as a `&str`.
    pub fn text_str(&self) -> &str { &self.text }

    // ── setter methods (for post-construction mutation) ───────────────────────

    pub fn set_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text != self.text {
            self.text = text;
            self.cache_pixels = None; // invalidate pixel cache
            // layout_text mismatch will trigger remeasure on next layout()
        }
    }
    pub fn set_color(&mut self, color: Color) {
        // Only invalidate if the color actually changed (avoids per-frame rebuilds
        // for widgets that call set_color with the same value every paint).
        let changed = match self.color {
            None    => true,
            Some(c) => !colors_equal(c, color),
        };
        if changed {
            self.color = Some(color);
            self.cache_pixels = None;
        }
    }
    pub fn clear_color(&mut self) {
        if self.color.is_some() {
            self.color = None;
            self.cache_pixels = None; // invalidate cache
        }
    }
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
        let line_h = self.font_size * 1.5;

        if self.wrap && available.width > 0.0 {
            // Rebuild wrapped lines when text, font_size, or available width changes.
            let text_changed = self.layout_text != self.text
                || (self.layout_font_size - self.font_size).abs() > 0.01;
            let width_changed = (self.wrap_at_width - available.width).abs() > 1.0;
            if text_changed || width_changed {
                self.wrapped_lines = wrap_text(&self.font, &self.text, self.font_size, available.width);
                self.wrap_at_width    = available.width;
                self.layout_text      = self.text.clone();
                self.layout_font_size = self.font_size;
                self.cache_pixels     = None; // invalidate backbuffer
            }
            let total_h = self.wrapped_lines.len() as f64 * line_h;
            Size::new(available.width, total_h)
        } else {
            // Single-line path: tight bounds matching rendered text width.
            // Text measurement (rustybuzz::shape) is cached: only re-runs when the
            // text string or font_size changes, not on every frame.
            if self.layout_text != self.text
                || (self.layout_font_size - self.font_size).abs() > 0.01
            {
                let metrics =
                    crate::text::measure_text_metrics(&self.font, &self.text, self.font_size);
                self.layout_width     = metrics.width;
                self.layout_text      = self.text.clone();
                self.layout_font_size = self.font_size;
            }
            Size::new(self.layout_width.min(available.width), line_h)
        }
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        // If no explicit colour was set, follow the active theme.
        let color = self.color.unwrap_or_else(|| ctx.visuals().text_color);

        // ── Wrapped multi-line path (always direct; no backbuffer for wrapped text) ─
        if self.wrap && !self.wrapped_lines.is_empty() {
            ctx.set_fill_color(color);
            let line_h = self.font_size * 1.5;
            let total_h = self.wrapped_lines.len() as f64 * line_h;
            for (i, line) in self.wrapped_lines.iter().enumerate() {
                if line.is_empty() { continue; }
                if let Some(m) = ctx.measure_text(line) {
                    // Y-up: line 0 is topmost → y_center = total_h - 0.5*line_h
                    let line_center_y = total_h - (i as f64 + 0.5) * line_h;
                    let ty = line_center_y - (m.ascent - m.descent) * 0.5;
                    let tx = match self.align {
                        LabelAlign::Left   => 0.0,
                        LabelAlign::Center => (w - m.width) * 0.5,
                        LabelAlign::Right  => w - m.width,
                    };
                    ctx.fill_text(line, tx, ty);
                }
            }
            return;
        }

        // ── Backbuffer path ───────────────────────────────────────────────────
        if self.buffered && ctx.has_image_blit() && w >= 1.0 && h >= 1.0 {
            let bw = w.ceil() as u32;
            let bh = h.ceil() as u32;

            // Rebuild cache if anything changed.
            let cache_valid = self.cache_pixels.is_some()
                && self.cache_w == bw
                && self.cache_h == bh
                && self.cache_text == self.text
                && (self.cache_font_size - self.font_size).abs() < 0.01
                && colors_equal(self.cache_color, color);

            if !cache_valid {
                let mut fb = Framebuffer::new(bw, bh);
                {
                    let mut gfx = GfxCtx::new(&mut fb);
                    gfx.set_font(Arc::clone(&self.font));
                    gfx.set_font_size(self.font_size);
                    gfx.set_fill_color(color);
                    if let Some(m) = gfx.measure_text(&self.text) {
                        let ty = h * 0.5 - (m.ascent - m.descent) * 0.5;
                        let tx = match self.align {
                            LabelAlign::Left   => 0.0,
                            LabelAlign::Center => (w - m.width) * 0.5,
                            LabelAlign::Right  => w - m.width,
                        };
                        gfx.fill_text(&self.text, tx, ty);
                    }
                }
                self.cache_pixels   = Some(fb.pixels_flipped());
                self.cache_w        = bw;
                self.cache_h        = bh;
                self.cache_text     = self.text.clone();
                self.cache_font_size = self.font_size;
                self.cache_color    = color;
            }

            if let Some(pixels) = &self.cache_pixels {
                ctx.draw_image_rgba(pixels, self.cache_w, self.cache_h, 0.0, 0.0, w, h);
            }
            return;
        }

        // ── Direct path (GL or non-buffered) ──────────────────────────────────
        ctx.set_fill_color(color);
        if let Some(m) = ctx.measure_text(&self.text) {
            let ty = h * 0.5 - (m.ascent - m.descent) * 0.5;
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
