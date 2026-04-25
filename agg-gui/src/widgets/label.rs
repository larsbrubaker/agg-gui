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
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
    /// When `true` (the default), this Label owns a CPU backbuffer
    /// that's re-rasterised on dirty and blitted every frame.  Set to
    /// `false` only for text that changes every frame (e.g. live
    /// counters) where caching adds overhead with no benefit — those
    /// go through `ctx.fill_text` direct every paint.
    pub buffered: bool,
    /// Per-widget CPU bitmap cache.  Populated by `paint_subtree` when
    /// `buffered = true`; invalidated by Label's setters (text, color,
    /// align, etc.) so the next paint re-rasterises.
    cache: crate::widget::BackbufferCache,
    /// When `true`, long lines are broken at word boundaries to fit
    /// `available.width`.  The label height expands to fit all lines.
    /// Disabled by default; enable with `.with_wrap(true)`.
    wrap: bool,
    /// When `true`, this Label ignores the system-wide font override
    /// (`font_settings::current_system_font`) and always renders with
    /// the specific `self.font` passed to `Label::new`.  Used by font
    /// preview widgets (ComboBox item labels in the System window's
    /// font selector) where each entry must render in its OWN face
    /// regardless of the current global font choice.
    ignore_system_font: bool,
    /// Per-instance LCD preference: `Some(true)` always LCD, `Some(false)`
    /// always grayscale, `None` defers to the global
    /// `font_settings::lcd_enabled()`.  Exposed on every widget via
    /// `Widget::lcd_preference`; Label is the only widget that reads it
    /// today.
    lcd_pref: Option<bool>,

    // ── Layout measurement cache ──────────────────────────────────────────────
    /// Cached text advance width from last `measure_advance()` call.
    /// Avoids calling `rustybuzz::shape()` every frame — only re-measures
    /// when `text` or `font_size` changes.
    layout_text: String,
    layout_font_size: f64,
    layout_width: f64,
    /// Pointer identity of the [`Font`] used for the last measurement.  If
    /// the system-wide font override (see
    /// [`font_settings::current_system_font`](crate::font_settings::current_system_font))
    /// is swapped, pointer identity changes and we re-measure to pick up
    /// the new font's glyph metrics.
    layout_font_ptr: *const Font,
    /// Width used for the last word-wrap computation.
    wrap_at_width: f64,
    /// Lines produced by the last word-wrap computation.
    wrapped_lines: Vec<String>,
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
            // Default: backbuffer only when grayscale.  Rationale:
            //   - Grayscale on GL direct-to-surface goes through
            //     tessellated glyph outlines, which are visibly thinner
            //     than AGG's subpixel-accurate scanline coverage.
            //     Routing grayscale through a software backbuffer gives
            //     AGG-quality rasterisation blitted as a texture.
            //   - LCD on GL direct-to-surface uses dual-source blend on
            //     the cached LCD mask — identical quality to AGG.
            //     Adding a backbuffer here would force the sub-ctx into
            //     `Rgba` mode (Label has no opaque bg for `LcdCoverage`)
            //     and lose the subpixel result.
            // `buffered` stores the user's opt-out; the actual decision
            // happens in `backbuffer_cache_mut` based on the global
            // LCD flag.
            buffered: true,
            cache: crate::widget::BackbufferCache::new(),
            wrap: false,
            ignore_system_font: false,
            lcd_pref: None,
            layout_text: String::new(),
            layout_font_size: 0.0,
            layout_width: 0.0,
            layout_font_ptr: std::ptr::null(),
            wrap_at_width: -1.0,
            wrapped_lines: Vec::new(),
        }
    }

    // ── builder methods ───────────────────────────────────────────────────────

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }
    /// Override the label colour.  Pass an explicit `Color` to always use that
    /// colour regardless of the active theme.  Omit this call to follow the
    /// theme's `text_color` automatically.
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
    pub fn with_align(mut self, align: LabelAlign) -> Self {
        self.align = align;
        self
    }
    pub fn with_has_backbuffer(mut self, v: bool) -> Self {
        self.buffered = v;
        self
    }
    /// Enable or disable word-wrapping.  When `true`, long lines are broken at
    /// word boundaries to fit the available width; the label height expands to
    /// accommodate all lines.  Newlines in the text are always honoured.
    pub fn with_wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Opt OUT of the system-wide font override for this Label.  The
    /// Label will render with `self.font` (passed to `Label::new`)
    /// regardless of what `font_settings::set_system_font` is pointing
    /// at.  Useful for font-preview UI — each entry in a font picker
    /// dropdown needs its OWN face, not the currently selected one.
    /// Pin this label's LCD setting: `Some(true)` always LCD, `Some(false)`
    /// always grayscale, `None` (default) defers to the global toggle.
    pub fn with_lcd(mut self, pref: Option<bool>) -> Self {
        self.lcd_pref = pref;
        self
    }

    pub fn with_ignore_system_font(mut self, ignore: bool) -> Self {
        self.ignore_system_font = ignore;
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

    // ── getter methods ────────────────────────────────────────────────────────

    /// Return the current label text as a `&str`.
    pub fn text_str(&self) -> &str {
        &self.text
    }

    /// Resolve the font used for THIS layout/paint.  Prefers the system-wide
    /// font override (set by the System window / `font_settings::set_system_font`)
    /// so swapping the system font live flows through every widget; falls
    /// back to the per-instance font otherwise.  Scrollbar-style pattern.
    fn active_font(&self) -> Arc<Font> {
        if self.ignore_system_font {
            Arc::clone(&self.font)
        } else {
            crate::font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.font))
        }
    }

    /// Per-instance font size multiplied by the system-wide
    /// [`font_settings::current_font_size_scale`].  Label's font-preview
    /// UI (combo-box items flagged `ignore_system_font`) ALSO ignores
    /// the scale — a font picker must show every entry at the same
    /// reference size or comparing faces becomes useless.
    fn active_font_size(&self) -> f64 {
        if self.ignore_system_font {
            self.font_size
        } else {
            self.font_size * crate::font_settings::current_font_size_scale()
        }
    }

    // ── setter methods (for post-construction mutation) ───────────────────────

    pub fn set_font_size(&mut self, size: f64) {
        if (self.font_size - size).abs() > 1e-9 {
            self.font_size = size;
            self.cache.invalidate();
        }
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text != self.text {
            self.text = text;
            self.cache.invalidate();
        }
    }
    pub fn set_color(&mut self, color: Color) {
        if self.color != Some(color) {
            self.color = Some(color);
            self.cache.invalidate();
        }
    }
    pub fn clear_color(&mut self) {
        if self.color.is_some() {
            self.color = None;
            self.cache.invalidate();
        }
    }
    pub fn set_align(&mut self, align: LabelAlign) {
        if self.align != align {
            self.align = align;
            self.cache.invalidate();
        }
    }
}

impl Widget for Label {
    fn type_name(&self) -> &'static str {
        "Label"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        // Only invalidate on SIZE change — position doesn't affect
        // cached bitmap (painted at local origin, blitted at parent's
        // choice of translation).  Framework also invalidates via
        // `cache.width != w || cache.height != h` in
        // `paint_subtree_backbuffered`, so this is defence in depth.
        if self.bounds.width != b.width || self.bounds.height != b.height {
            self.cache.invalidate();
        }
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn lcd_preference(&self) -> Option<bool> {
        self.lcd_pref
    }

    fn backbuffer_cache_mut(&mut self) -> Option<&mut crate::widget::BackbufferCache> {
        // Cache always when `buffered`.  Mode is chosen by
        // `backbuffer_mode` below — LCD on → per-channel LcdCoverage
        // buffer, LCD off → Rgba buffer.  Per-channel alpha means
        // unpainted pixels stay `alpha = 0` and blit leaves parent
        // unchanged there, so no scroll-stale cache problem (that
        // was a dead end from the seed-from-parent approach we ripped
        // out).
        if self.buffered {
            Some(&mut self.cache)
        } else {
            None
        }
    }

    fn backbuffer_mode(&self) -> crate::widget::BackbufferMode {
        // Dispatching on the global LCD flag means toggling the
        // setting automatically rebuilds every cached label in the
        // right format — `paint_subtree_backbuffered` detects the
        // mode flip via `cache.lcd_alpha.is_some()` vs the requested
        // mode and forces a re-raster.
        if crate::font_settings::lcd_enabled() {
            crate::widget::BackbufferMode::LcdCoverage
        } else {
            crate::widget::BackbufferMode::Rgba
        }
    }

    /// Labels are never independently hittable.  This lets their interactive
    /// parent (e.g., Button) retain full hit-test and focus ownership even
    /// when the label fills the parent's entire bounds.
    fn hit_test(&self, _: Point) -> bool {
        false
    }

    fn layout(&mut self, available: Size) -> Size {
        // Resolve the effective font + size ONCE per layout so this call
        // and the paint that follows agree on glyph metrics even if the
        // system scale is mid-transition.
        let font = self.active_font();
        let size = self.active_font_size();
        let line_h = size * 1.5;

        // Drop the pre-rasterized bitmap the moment we notice a font or size
        // swap — unconditionally, before any other branching.  Without this
        // a buffered Label (the default) keeps blitting glyphs drawn with
        // the previous typeface / point size until a bounds change or a
        // text edit happens to invalidate the cache.  DragValue hits this
        // hardest: its `value_label` often measures the same width for two
        // different fonts ("14.0" in Arial vs the default is identical
        // within a pixel), so the size-based invalidation in `set_bounds`
        // never fires and the stale bitmap lingers until the user hovers
        // (which triggers some other layout-affecting update).
        let font_changed = Arc::as_ptr(&font) != self.layout_font_ptr;
        let size_changed = (self.layout_font_size - size).abs() > 0.01;
        if font_changed || size_changed {
            self.cache.invalidate();
        }

        if self.wrap && available.width > 0.0 {
            let text_changed = self.layout_text != self.text || size_changed;
            let width_changed = (self.wrap_at_width - available.width).abs() > 1.0;
            if text_changed || width_changed || font_changed {
                self.wrapped_lines = wrap_text(&font, &self.text, size, available.width);
                self.wrap_at_width = available.width;
                self.layout_text = self.text.clone();
                self.layout_font_size = size;
                self.layout_font_ptr = Arc::as_ptr(&font);
                // Text changes also need a bitmap rebuild.
                if text_changed {
                    self.cache.invalidate();
                }
            }
            let total_h = self.wrapped_lines.len() as f64 * line_h;
            Size::new(available.width, total_h)
        } else {
            // Single-line path: tight bounds matching rendered text width.
            if self.layout_text != self.text || size_changed || font_changed {
                let metrics = crate::text::measure_text_metrics(&font, &self.text, size);
                self.layout_width = metrics.width;
                self.layout_text = self.text.clone();
                self.layout_font_size = size;
                self.layout_font_ptr = Arc::as_ptr(&font);
            }
            Size::new(self.layout_width.min(available.width), line_h)
        }
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Resolve the font to use THIS PAINT: prefer the system-wide override
        // (set by the System window) so font changes propagate live; fall
        // back to the per-instance font otherwise.  The same resolution runs
        // in `layout()` so the two stages agree on metrics.
        let font = self.active_font();
        let size = self.active_font_size();

        ctx.set_font(Arc::clone(&font));
        ctx.set_font_size(size);
        // If no explicit colour was set, follow the active theme.
        let color = self.color.unwrap_or_else(|| ctx.visuals().text_color);

        let is_wrapped = self.wrap && !self.wrapped_lines.is_empty();

        // Clip text rendering to the label's bounds.  `Label::layout`
        // clamps its returned width to `available.width`, so a long
        // label inside a narrow parent gets bounds narrower than the
        // text's natural width.  The backbuffered path (grayscale cache)
        // implicitly clips at the bitmap's edges; the direct-paint path
        // (LCD mode) would otherwise draw glyphs past the bounds.  An
        // explicit clip makes both modes behave identically — text
        // never escapes the label's rect.
        ctx.save();
        ctx.clip_rect(0.0, 0.0, w, h);

        // Labels always paint through `ctx.fill_text` — the backend
        // decides LCD vs grayscale AA internally based on
        // `font_settings::lcd_enabled()` and whether it can composite
        // per-channel coverage.  No backbuffer, no LCD-specific logic
        // lives here.  Label is just a widget that draws text.
        ctx.set_fill_color(color);
        if is_wrapped {
            let line_h = size * 1.5;
            let total_h = self.wrapped_lines.len() as f64 * line_h;
            for (i, line) in self.wrapped_lines.iter().enumerate() {
                if line.is_empty() {
                    continue;
                }
                if let Some(m) = ctx.measure_text(line) {
                    let line_center_y = total_h - (i as f64 + 0.5) * line_h;
                    let ty = line_center_y - (m.ascent - m.descent) * 0.5;
                    let tx = match self.align {
                        LabelAlign::Left => 0.0,
                        LabelAlign::Center => (w - m.width) * 0.5,
                        LabelAlign::Right => w - m.width,
                    };
                    ctx.fill_text(line, tx, ty);
                }
            }
        } else if let Some(m) = ctx.measure_text(&self.text) {
            let ty = h * 0.5 - (m.ascent - m.descent) * 0.5;
            let tx = match self.align {
                LabelAlign::Left => 0.0,
                LabelAlign::Center => (w - m.width) * 0.5,
                LabelAlign::Right => w - m.width,
            };
            ctx.fill_text(&self.text, tx, ty);
        }

        ctx.restore();
    }

    fn margin(&self) -> Insets {
        self.base.margin
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

    fn measure_min_height(&self, available_w: f64) -> f64 {
        // Wrapped: count lines at the supplied width.  Non-wrapped:
        // a single line tall.  Used by ancestor `Window::tight_content_fit`
        // to compute a content-bound for height.
        let font = self.active_font();
        let size = self.active_font_size();
        let line_h = size * 1.5;
        if self.wrap && available_w > 0.0 {
            let lines = wrap_text(&font, &self.text, size, available_w);
            (lines.len().max(1) as f64) * line_h
        } else {
            line_h
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }

    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("text", self.text.clone()),
            ("font_size", format!("{:.1}", self.font_size)),
            ("align", format!("{:?}", self.align)),
            (
                "has_backbuffer",
                if self.buffered { "true" } else { "false" }.to_string(),
            ),
        ]
    }
}
