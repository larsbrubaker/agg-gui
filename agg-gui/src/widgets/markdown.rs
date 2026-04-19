//! `MarkdownView` — render a Markdown string as formatted text with images.
//!
//! Uses `pulldown-cmark` for parsing, then converts the event stream into a
//! flat list of styled lines and image placeholders.  Word-wrapping is
//! computed in `layout()` using the standalone `measure_text_metrics` function
//! so no `DrawCtx` is needed at layout time.
//!
//! # Image support
//!
//! Pass an `image_provider` closure via [`MarkdownView::with_image_provider`].
//! It receives the image URL/path string and should return
//! `Some((rgba_bytes, width, height))` or `None` for unknown images.  The data
//! must be tightly-packed RGBA8 in row-major order, **top-row first**.
//!
//! Images are decoded and cached on the first `layout()` call and then drawn
//! via `DrawCtx::draw_image_rgba()` on every `paint()`.
//!
//! # Supported Markdown features
//!
//! - Headings H1–H4 (larger font sizes)
//! - Paragraphs (word-wrapped)
//! - Bullet lists (`-`/`*`) with "• " prefix
//! - Ordered lists with "N. " prefix
//! - Inline code `` `x` `` (highlight)
//! - Fenced code blocks (background box)
//! - Horizontal rules (thin separator line)
//! - Images via `image_provider` callback; placeholder box when unavailable
//! - Links (coloured text, URL is not opened — add `on_link_click` if needed)

use std::sync::Arc;

use pulldown_cmark::{Event as MdEvent, Options, Parser, Tag, TagEnd};

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::{measure_text_metrics, Font};
use crate::widget::Widget;

// ── Styled line representation ─────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum LineStyle {
    Body,
    H1,
    H2,
    H3,
    H4,
    Code,
    Rule,
}

impl LineStyle {
    fn font_size(self, base: f64) -> f64 {
        match self {
            LineStyle::H1   => base * 1.8,
            LineStyle::H2   => base * 1.5,
            LineStyle::H3   => base * 1.25,
            LineStyle::H4   => base * 1.1,
            LineStyle::Body => base,
            LineStyle::Code => base * 0.9,
            LineStyle::Rule => base,
        }
    }
}

// ── Layout item ────────────────────────────────────────────────────────────────

/// A single item in the laid-out view.
#[derive(Clone)]
enum LayoutItem {
    /// A text row (including blank spacing rows and horizontal rules).
    Line {
        text:   String,
        style:  LineStyle,
        indent: f64,
        y:      f64,
        height: f64,
    },
    /// An image row — draws cached pixel data or a placeholder box.
    Image {
        /// URL/path originally specified in the Markdown.
        #[allow(dead_code)]
        url:    String,
        alt:    String,
        /// Index into `MarkdownView::image_cache`.
        cache_idx: usize,
        /// Displayed rect in local Y-up coordinates.
        x:      f64,
        y:      f64,
        width:  f64,
        height: f64,
    },
}

// ── Intermediate paragraph item (before layout) ────────────────────────────────

enum ParagraphItem {
    Text(String, LineStyle, f64),
    Image { url: String, alt: String },
}

// ── Image cache entry ──────────────────────────────────────────────────────────

struct ImageEntry {
    url:    String,
    /// `None` = provider returned nothing, `Some(...)` = decoded image.
    data:   Option<(Vec<u8>, u32, u32)>,
}

// ── MarkdownView widget ────────────────────────────────────────────────────────

/// A widget that renders a Markdown string as formatted, word-wrapped text
/// with optional image support.
pub struct MarkdownView {
    bounds:    Rect,
    children:  Vec<Box<dyn Widget>>,
    base:      WidgetBase,

    markdown:  String,
    font:      Arc<Font>,
    font_size: f64,
    padding:   f64,

    /// Optional image decoder.  Receives a URL/path, returns RGBA8 pixel data
    /// (top-row first) + (width, height), or `None` if unavailable.
    image_provider: Option<Box<dyn Fn(&str) -> Option<(Vec<u8>, u32, u32)>>>,

    /// Cached image data, indexed by `LayoutItem::Image::cache_idx`.
    image_cache: Vec<ImageEntry>,

    /// Laid-out items (populated by `layout()`).
    items:     Vec<LayoutItem>,
    /// Total content height from the last layout pass.
    content_h: f64,
}

impl MarkdownView {
    pub fn new(markdown: impl Into<String>, font: Arc<Font>) -> Self {
        Self {
            bounds:         Rect::default(),
            children:       Vec::new(),
            base:           WidgetBase::new(),
            markdown:       markdown.into(),
            font,
            font_size:      14.0,
            padding:        8.0,
            image_provider: None,
            image_cache:    Vec::new(),
            items:          Vec::new(),
            content_h:      0.0,
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self { self.font_size = size; self }
    pub fn with_padding(mut self, p: f64) -> Self { self.padding = p; self }

    /// Currently-active font — honours the thread-local system-font override
    /// (`font_settings::current_system_font`) so system-font changes propagate
    /// live without rebuilding the markdown view.
    fn active_font(&self) -> Arc<Font> {
        crate::font_settings::current_system_font()
            .unwrap_or_else(|| Arc::clone(&self.font))
    }

    /// Supply an image provider closure.
    ///
    /// The closure receives a URL/path string from the Markdown source and must
    /// return `Some((rgba_bytes, width, height))` or `None`.
    pub fn with_image_provider(
        mut self,
        provider: impl Fn(&str) -> Option<(Vec<u8>, u32, u32)> + 'static,
    ) -> Self {
        self.image_provider = Some(Box::new(provider));
        self
    }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }

    // ── Markdown → paragraph items ────────────────────────────────────────────

    fn parse_paragraphs(&self) -> Vec<ParagraphItem> {
        let mut out: Vec<ParagraphItem> = Vec::new();

        let opts = Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_TABLES;
        let parser = Parser::new_ext(&self.markdown, opts);

        let mut cur_text   = String::new();
        let mut cur_style  = LineStyle::Body;
        let mut cur_indent = 0.0_f64;
        let mut list_depth = 0u32;
        let mut list_ordinal: Vec<u64> = Vec::new();
        // When inside an image tag, collect the alt text and suppress normal text.
        let mut in_image: Option<String> = None; // Some(url) while parsing image

        let flush = |out: &mut Vec<ParagraphItem>, text: &mut String, style: LineStyle, indent: f64| {
            let t = text.trim().to_string();
            if !t.is_empty() {
                out.push(ParagraphItem::Text(t, style, indent));
            }
            text.clear();
        };

        for ev in parser {
            match ev {
                MdEvent::Start(Tag::Image { dest_url, .. }) => {
                    // Flush any pending text, then start collecting alt text.
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                    in_image = Some(dest_url.to_string());
                }
                MdEvent::End(TagEnd::Image) => {
                    if let Some(url) = in_image.take() {
                        let alt = cur_text.trim().to_string();
                        cur_text.clear();
                        out.push(ParagraphItem::Image { url, alt });
                        out.push(ParagraphItem::Text("".to_string(), LineStyle::Body, 0.0)); // spacing
                    }
                }
                // While parsing an image, Text events are alt text — collect separately.
                MdEvent::Text(t) if in_image.is_some() => {
                    cur_text.push_str(&t);
                }
                MdEvent::Start(Tag::Heading { level, .. }) => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                    cur_style  = match level as u8 { 1 => LineStyle::H1, 2 => LineStyle::H2, 3 => LineStyle::H3, _ => LineStyle::H4 };
                    cur_indent = 0.0;
                }
                MdEvent::End(TagEnd::Heading(_)) => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                    out.push(ParagraphItem::Text("".to_string(), LineStyle::Body, 0.0));
                    cur_style  = LineStyle::Body;
                    cur_indent = 0.0;
                }
                MdEvent::Start(Tag::Paragraph) => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                }
                MdEvent::End(TagEnd::Paragraph) => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                    out.push(ParagraphItem::Text("".to_string(), LineStyle::Body, 0.0));
                }
                MdEvent::Start(Tag::List(first)) => {
                    list_depth += 1;
                    list_ordinal.push(first.unwrap_or(1));
                    cur_indent = list_depth as f64 * 16.0;
                }
                MdEvent::End(TagEnd::List(_)) => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                    list_depth = list_depth.saturating_sub(1);
                    list_ordinal.pop();
                    cur_indent = list_depth as f64 * 16.0;
                    if list_depth == 0 {
                        out.push(ParagraphItem::Text("".to_string(), LineStyle::Body, 0.0));
                    }
                }
                MdEvent::Start(Tag::Item) => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                    if let Some(n) = list_ordinal.last_mut() {
                        cur_text = format!("{}. ", n);
                        *n += 1;
                    } else {
                        cur_text = "• ".to_string();
                    }
                }
                MdEvent::End(TagEnd::Item) => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                }
                MdEvent::Start(Tag::CodeBlock(_)) => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                    cur_style = LineStyle::Code;
                }
                MdEvent::End(TagEnd::CodeBlock) => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                    out.push(ParagraphItem::Text("".to_string(), LineStyle::Body, 0.0));
                    cur_style = LineStyle::Body;
                }
                MdEvent::Rule => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                    out.push(ParagraphItem::Text("".to_string(), LineStyle::Rule, 0.0));
                }
                MdEvent::Text(t) => {
                    if !cur_text.is_empty() && !cur_text.ends_with(' ') && !cur_text.ends_with('\n') {
                        cur_text.push(' ');
                    }
                    cur_text.push_str(&t);
                }
                MdEvent::Code(t) => {
                    if !cur_text.is_empty() && !cur_text.ends_with(' ') { cur_text.push(' '); }
                    cur_text.push('`');
                    cur_text.push_str(&t);
                    cur_text.push('`');
                }
                MdEvent::SoftBreak | MdEvent::HardBreak => { cur_text.push(' '); }
                MdEvent::Start(Tag::Link { .. }) | MdEvent::End(TagEnd::Link) => {}
                _ => {}
            }
        }
        flush(&mut out, &mut cur_text, cur_style, cur_indent);
        out
    }

    // ── Word-wrapping ─────────────────────────────────────────────────────────

    fn wrap_paragraph(&self, text: &str, style: LineStyle, indent: f64, max_w: f64) -> Vec<(String, f64)> {
        let font_size = style.font_size(self.font_size);
        let avail     = (max_w - indent).max(1.0);
        if text.is_empty() { return vec![("".to_string(), indent)]; }

        let font = self.active_font();
        let mut lines: Vec<(String, f64)> = Vec::new();
        let mut current = String::new();

        for word in text.split_whitespace() {
            let candidate = if current.is_empty() { word.to_string() } else { format!("{} {}", current, word) };
            let w = measure_text_metrics(&font, &candidate, font_size).width;
            if w <= avail || current.is_empty() {
                current = candidate;
            } else {
                lines.push((current, indent));
                current = word.to_string();
            }
        }
        if !current.is_empty() { lines.push((current, indent)); }
        lines
    }

    // ── Image cache management ────────────────────────────────────────────────

    /// Return the cache index for `url`, loading it via the provider if not yet cached.
    fn get_or_load_image(&mut self, url: &str) -> usize {
        // Check if already cached.
        if let Some(idx) = self.image_cache.iter().position(|e| e.url == url) {
            return idx;
        }
        // Load via provider.
        let data = self.image_provider.as_ref().and_then(|p| p(url));
        let idx = self.image_cache.len();
        self.image_cache.push(ImageEntry { url: url.to_string(), data });
        idx
    }
}

impl Widget for MarkdownView {
    fn type_name(&self) -> &'static str { "MarkdownView" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }

    fn layout(&mut self, available: Size) -> Size {
        let pad   = self.padding;
        let max_w = (available.width - pad * 2.0).max(1.0);

        let paragraphs = self.parse_paragraphs();

        // Build intermediate list: (text/image, style, indent, line_h), top-to-bottom.
        struct RawItem {
            text:    String,
            style:   LineStyle,
            indent:  f64,
            height:  f64,
            // Image-specific fields.
            is_image:  bool,
            image_url: String,
            image_alt: String,
            cache_idx: usize,
            img_disp_w: f64,
        }

        let mut raw: Vec<RawItem> = Vec::new();

        for item in &paragraphs {
            match item {
                ParagraphItem::Text(text, style, indent) => {
                    if *style == LineStyle::Rule {
                        raw.push(RawItem { text: String::new(), style: LineStyle::Rule, indent: 0.0,
                            height: 8.0, is_image: false, image_url: String::new(), image_alt: String::new(),
                            cache_idx: 0, img_disp_w: 0.0 });
                        continue;
                    }
                    let font_size = style.font_size(self.font_size);
                    let metrics   = measure_text_metrics(&self.active_font(), "", font_size);
                    let line_h    = metrics.line_height * 1.3;

                    if text.is_empty() {
                        raw.push(RawItem { text: String::new(), style: *style, indent: *indent,
                            height: line_h * 0.5, is_image: false, image_url: String::new(),
                            image_alt: String::new(), cache_idx: 0, img_disp_w: 0.0 });
                        continue;
                    }
                    let wrapped = self.wrap_paragraph(text, *style, *indent, max_w);
                    for (wl, ind) in wrapped {
                        raw.push(RawItem { text: wl, style: *style, indent: ind,
                            height: line_h, is_image: false, image_url: String::new(),
                            image_alt: String::new(), cache_idx: 0, img_disp_w: 0.0 });
                    }
                }
                ParagraphItem::Image { url, alt } => {
                    let cache_idx = self.get_or_load_image(url);
                    let (disp_w, disp_h) = if let Some((_, iw, ih)) = self.image_cache[cache_idx].data.as_ref() {
                        // Scale to fit available width, preserve aspect.
                        let scale = (max_w / *iw as f64).min(1.0); // never upscale beyond natural size
                        (*iw as f64 * scale, *ih as f64 * scale)
                    } else {
                        // Placeholder: full-width × 60px box.
                        (max_w, 60.0)
                    };
                    raw.push(RawItem { text: alt.clone(), style: LineStyle::Body, indent: 0.0,
                        height: disp_h, is_image: true, image_url: url.clone(),
                        image_alt: alt.clone(), cache_idx, img_disp_w: disp_w });
                }
            }
        }

        // Assign Y positions (Y-up: cursor starts at top and decrements).
        let total_h: f64 = raw.iter().map(|r| r.height).sum::<f64>() + pad * 2.0;
        let mut y = total_h - pad;

        self.items.clear();
        for r in raw {
            y -= r.height;
            if r.is_image {
                self.items.push(LayoutItem::Image {
                    url:       r.image_url,
                    alt:       r.image_alt,
                    cache_idx: r.cache_idx,
                    x:         pad,
                    y,
                    width:     r.img_disp_w,
                    height:    r.height,
                });
            } else {
                self.items.push(LayoutItem::Line {
                    text:   r.text,
                    style:  r.style,
                    indent: r.indent,
                    y,
                    height: r.height,
                });
            }
        }

        self.content_h = total_h;
        self.bounds = Rect::new(0.0, 0.0, available.width, total_h);
        Size::new(available.width, total_h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v   = ctx.visuals();
        let pad = self.padding;
        let w   = self.bounds.width;
        let font = self.active_font();
        ctx.set_font(Arc::clone(&font));

        for item in &self.items {
            match item {
                LayoutItem::Line { text, style, indent, y, height } => {
                    let fs = style.font_size(self.font_size);
                    ctx.set_font_size(fs);

                    let tx = pad + indent;
                    let ty = y + height * 0.5;
                    let metrics = measure_text_metrics(&font, text.as_str(), fs);
                    let text_y  = ty - (metrics.ascent - metrics.descent) * 0.5;

                    match style {
                        LineStyle::Rule => {
                            ctx.set_fill_color(v.separator);
                            ctx.begin_path();
                            ctx.rect(pad, ty, w - pad * 2.0, 1.0);
                            ctx.fill();
                        }
                        LineStyle::Code => {
                            ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.15));
                            ctx.begin_path();
                            ctx.rounded_rect(pad, *y, w - pad * 2.0, *height, 3.0);
                            ctx.fill();
                            ctx.set_fill_color(v.accent);
                            ctx.fill_text(text, tx + 4.0, text_y);
                        }
                        _ => {
                            ctx.set_fill_color(v.text_color);
                            if !text.is_empty() {
                                ctx.fill_text(text, tx, text_y);
                            }
                        }
                    }
                }
                LayoutItem::Image { url: _, alt, cache_idx, x, y, width, height } => {
                    if let Some(entry) = self.image_cache.get(*cache_idx) {
                        if let Some((data, iw, ih)) = &entry.data {
                            ctx.draw_image_rgba(data.as_slice(), *iw, *ih, *x, *y, *width, *height);
                        } else {
                            // Placeholder box when image unavailable.
                            ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.5, 0.15));
                            ctx.begin_path();
                            ctx.rounded_rect(*x, *y, *width, *height, 4.0);
                            ctx.fill();
                            ctx.set_fill_color(v.text_dim);
                            ctx.set_font_size(self.font_size * 0.85);
                            let label = format!("[image: {}]", alt);
                            ctx.fill_text(&label, x + 8.0, y + height * 0.5);
                        }
                    }
                }
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}
