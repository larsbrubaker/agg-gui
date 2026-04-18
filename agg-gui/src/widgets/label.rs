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
use crate::framebuffer::{Framebuffer, unpremultiply_rgba_inplace};
use crate::gfx_ctx::GfxCtx;
use crate::image_cache::{get_or_raster, LabelPixelKey};
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
    /// When `true`, this Label ignores the system-wide font override
    /// (`font_settings::current_system_font`) and always renders with
    /// the specific `self.font` passed to `Label::new`.  Used by font
    /// preview widgets (ComboBox item labels in the System window's
    /// font selector) where each entry must render in its OWN face
    /// regardless of the current global font choice.
    ignore_system_font: bool,

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

    // ── Backbuffer cache ──────────────────────────────────────────────────────
    //
    // Labels don't own their cached pixels any more — they come from the
    // shared crate-level `image_cache`, keyed on (text, font, size, color,
    // bounds, align).  We retain the most-recent `Arc` + its dimensions on
    // the widget only so `cache_for_test` can inspect the last paint and so
    // the GL backend sees a stable `Arc::as_ptr` key between frames.  The
    // `Arc` drops naturally when the label drops; no manual invalidation
    // is needed on set_text/set_color because a new key misses the global
    // cache and calls the rasterizer.
    cache_pixels: Option<Arc<Vec<u8>>>,
    cache_w: u32,
    cache_h: u32,
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
            ignore_system_font: false,
            layout_text: String::new(),
            layout_font_size: 0.0,
            layout_width: 0.0,
            layout_font_ptr: std::ptr::null(),
            wrap_at_width: -1.0,
            wrapped_lines: Vec::new(),
            cache_pixels: None,
            cache_w: 0,
            cache_h: 0,
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

    /// Opt OUT of the system-wide font override for this Label.  The
    /// Label will render with `self.font` (passed to `Label::new`)
    /// regardless of what `font_settings::set_system_font` is pointing
    /// at.  Useful for font-preview UI — each entry in a font picker
    /// dropdown needs its OWN face, not the currently selected one.
    pub fn with_ignore_system_font(mut self, ignore: bool) -> Self {
        self.ignore_system_font = ignore;
        self
    }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

    // ── getter methods ────────────────────────────────────────────────────────

    /// Return the current label text as a `&str`.
    pub fn text_str(&self) -> &str { &self.text }

    /// Resolve the font used for THIS layout/paint.  Prefers the system-wide
    /// font override (set by the System window / `font_settings::set_system_font`)
    /// so swapping the system font live flows through every widget; falls
    /// back to the per-instance font otherwise.  Scrollbar-style pattern.
    fn active_font(&self) -> Arc<Font> {
        if self.ignore_system_font {
            Arc::clone(&self.font)
        } else {
            crate::font_settings::current_system_font()
                .unwrap_or_else(|| Arc::clone(&self.font))
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

    /// Test-only accessor for the backbuffer cache. Returns the cached
    /// `(pixels, width, height)` triple, where pixels are **straight-alpha**
    /// RGBA8 in top-row-first order.
    #[cfg(test)]
    pub(crate) fn cache_for_test(&self) -> Option<(&[u8], u32, u32)> {
        self.cache_pixels
            .as_ref()
            .map(|arc| (arc.as_slice(), self.cache_w, self.cache_h))
    }

    // ── setter methods (for post-construction mutation) ───────────────────────

    pub fn set_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text != self.text {
            self.text = text;
            // layout_text mismatch will trigger remeasure on next layout().
            // No manual pixel-cache invalidation: the new text produces a
            // new key in image_cache, which is either a hit (reuse) or a
            // miss (rasterize).
        }
    }
    pub fn set_color(&mut self, color: Color) {
        self.color = Some(color);
    }
    pub fn clear_color(&mut self) {
        self.color = None;
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
        // Resolve the effective font + size ONCE per layout so this call
        // and the paint that follows agree on glyph metrics even if the
        // system scale is mid-transition.
        let font   = self.active_font();
        let size   = self.active_font_size();
        let line_h = size * 1.5;

        if self.wrap && available.width > 0.0 {
            let text_changed  = self.layout_text != self.text
                || (self.layout_font_size - size).abs() > 0.01;
            let width_changed = (self.wrap_at_width - available.width).abs() > 1.0;
            // ALSO rebuild when the system font has been swapped since the
            // last layout — measurement depends on glyph metrics from a
            // particular font.
            let font_changed  = Arc::as_ptr(&font) != self.layout_font_ptr;
            if text_changed || width_changed || font_changed {
                self.wrapped_lines = wrap_text(&font, &self.text, size, available.width);
                self.wrap_at_width    = available.width;
                self.layout_text      = self.text.clone();
                self.layout_font_size = size;
                self.layout_font_ptr  = Arc::as_ptr(&font);
                self.cache_pixels     = None; // invalidate backbuffer
            }
            let total_h = self.wrapped_lines.len() as f64 * line_h;
            Size::new(available.width, total_h)
        } else {
            // Single-line path: tight bounds matching rendered text width.
            let font_changed = Arc::as_ptr(&font) != self.layout_font_ptr;
            if self.layout_text != self.text
                || (self.layout_font_size - size).abs() > 0.01
                || font_changed
            {
                let metrics =
                    crate::text::measure_text_metrics(&font, &self.text, size);
                self.layout_width     = metrics.width;
                self.layout_text      = self.text.clone();
                self.layout_font_size = size;
                self.layout_font_ptr  = Arc::as_ptr(&font);
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

        // ── Backbuffer path ───────────────────────────────────────────────────
        // Handles BOTH single-line and wrapped multi-line content, in
        // BOTH rendering modes (grayscale AA and LCD subpixel).  The LCD
        // path activates when:
        //   - `font_settings::lcd_enabled()` is `true`, AND
        //   - a parent widget has pushed a surface bg via
        //     `Widget::surface_bg_for_children` (needed for per-channel
        //     blending to be correct).
        // Otherwise the grayscale AA path (the default) is used —
        // un-premultiplied output that composites correctly over any dst.
        if self.buffered && ctx.has_image_blit() && w >= 1.0 && h >= 1.0 {
            let bw = w.ceil() as u32;
            let bh = h.ceil() as u32;

            let align_byte: u8 = match self.align {
                LabelAlign::Left   => 0,
                LabelAlign::Center => 1,
                LabelAlign::Right  => 2,
            };

            // Decide LCD vs grayscale for THIS paint.
            //
            // **Single source of truth: read the actual pixel painted
            // beneath us.**  `ctx.sample_bg_pixel` returns `Some(color)`
            // when the backend can cheap-read its framebuffer (GfxCtx =
            // memory read; GlGfxCtx = `glReadPixels` once per cache miss)
            // AND the sampled pixel is opaque.  If it returns `None` — no
            // known bg — we fall back to grayscale AA.  We do NOT guess
            // from widget-declared hints: either we know the destination
            // colour or we don't.
            let lcd_bg: Option<Color> =
                if crate::font_settings::lcd_enabled() {
                    ctx.sample_bg_pixel(0.0, 0.0)
                } else {
                    None
                };

            let mut key = LabelPixelKey::new(
                &self.text,
                Arc::as_ptr(&font) as *const () as usize,
                size,
                color,
                bw, bh, align_byte,
            );
            if let Some(bg) = lcd_bg {
                key = key.with_lcd_bg(bg);
            }

            let text  = self.text.clone();
            let font  = Arc::clone(&font);
            let align = self.align;
            let wrapped_lines = if is_wrapped {
                Some(self.wrapped_lines.clone())
            } else {
                None
            };

            let arc = get_or_raster(key, move || {
                let mut fb = Framebuffer::new(bw, bh);

                if let Some(bg) = lcd_bg {
                    // ── LCD subpixel branch ──────────────────────────
                    // Pre-fill bg so `PixfmtRgba32Lcd`'s per-channel blend
                    // sees the destination colour; each line's text is
                    // then mixed in via the 5-tap distribution kernel.
                    let bg_r = (bg.r * 255.0).clamp(0.0, 255.0) as u8;
                    let bg_g = (bg.g * 255.0).clamp(0.0, 255.0) as u8;
                    let bg_b = (bg.b * 255.0).clamp(0.0, 255.0) as u8;
                    for px in fb.pixels_mut().chunks_exact_mut(4) {
                        px[0] = bg_r; px[1] = bg_g; px[2] = bg_b; px[3] = 255;
                    }

                    // Text positions for the LCD path — identical to the
                    // grayscale path below, but we measure via the text
                    // module directly (we don't have a GfxCtx here).
                    let metrics = |s: &str| {
                        crate::text::measure_text_metrics(&font, s, size)
                    };
                    let xform = agg_rust::trans_affine::TransAffine::new();

                    if let Some(lines) = &wrapped_lines {
                        let line_h  = size * 1.5;
                        let total_h = lines.len() as f64 * line_h;
                        for (i, line) in lines.iter().enumerate() {
                            if line.is_empty() { continue; }
                            let m = metrics(line);
                            let line_center_y =
                                total_h - (i as f64 + 0.5) * line_h;
                            let ty = line_center_y - (m.ascent - m.descent) * 0.5;
                            let tx = match align {
                                LabelAlign::Left   => 0.0,
                                LabelAlign::Center => (w - m.width) * 0.5,
                                LabelAlign::Right  => w - m.width,
                            };
                            crate::text_lcd::blend_text_lcd(
                                &mut fb, &font, line, size, tx, ty, color, &xform,
                            );
                        }
                    } else {
                        let m = metrics(&text);
                        let ty = h * 0.5 - (m.ascent - m.descent) * 0.5;
                        let tx = match align {
                            LabelAlign::Left   => 0.0,
                            LabelAlign::Center => (w - m.width) * 0.5,
                            LabelAlign::Right  => w - m.width,
                        };
                        crate::text_lcd::blend_text_lcd(
                            &mut fb, &font, &text, size, tx, ty, color, &xform,
                        );
                    }

                    // Already opaque (alpha=255 everywhere from both the
                    // pre-fill and the LCD pixfmt).  Flip to top-row-first
                    // to match `draw_image_rgba` convention.
                    fb.pixels_flipped()
                } else {
                    // ── Grayscale AA branch (default) ────────────────
                    {
                        let mut gfx = GfxCtx::new(&mut fb);
                        gfx.set_font(Arc::clone(&font));
                        gfx.set_font_size(size);
                        gfx.set_fill_color(color);

                        if let Some(lines) = &wrapped_lines {
                            let line_h  = size * 1.5;
                            let total_h = lines.len() as f64 * line_h;
                            for (i, line) in lines.iter().enumerate() {
                                if line.is_empty() { continue; }
                                if let Some(m) = gfx.measure_text(line) {
                                    let line_center_y =
                                        total_h - (i as f64 + 0.5) * line_h;
                                    let ty = line_center_y
                                        - (m.ascent - m.descent) * 0.5;
                                    let tx = match align {
                                        LabelAlign::Left   => 0.0,
                                        LabelAlign::Center => (w - m.width) * 0.5,
                                        LabelAlign::Right  => w - m.width,
                                    };
                                    gfx.fill_text(line, tx, ty);
                                }
                            }
                        } else if let Some(m) = gfx.measure_text(&text) {
                            let ty = h * 0.5 - (m.ascent - m.descent) * 0.5;
                            let tx = match align {
                                LabelAlign::Left   => 0.0,
                                LabelAlign::Center => (w - m.width) * 0.5,
                                LabelAlign::Right  => w - m.width,
                            };
                            gfx.fill_text(&text, tx, ty);
                        }
                    }
                    let mut pixels = fb.pixels_flipped();
                    unpremultiply_rgba_inplace(&mut pixels);
                    pixels
                }
            });
            self.cache_pixels = Some(Arc::clone(&arc));
            self.cache_w      = bw;
            self.cache_h      = bh;

            ctx.draw_image_rgba_arc(&arc, bw, bh, 0.0, 0.0, bw as f64, bh as f64);
            return;
        }

        // ── Direct path — no backbuffer (GL tess glyphs, or `buffered = false`) ─
        ctx.set_fill_color(color);
        if is_wrapped {
            let line_h  = size * 1.5;
            let total_h = self.wrapped_lines.len() as f64 * line_h;
            for (i, line) in self.wrapped_lines.iter().enumerate() {
                if line.is_empty() { continue; }
                if let Some(m) = ctx.measure_text(line) {
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
        } else if let Some(m) = ctx.measure_text(&self.text) {
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
