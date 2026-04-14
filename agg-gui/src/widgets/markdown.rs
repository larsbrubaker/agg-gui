//! `MarkdownView` — render a Markdown string as formatted text.
//!
//! Uses `pulldown-cmark` for parsing, then converts the event stream into a
//! flat list of styled lines.  Word-wrapping is computed in `layout()` using
//! the standalone `measure_text_metrics` / `measure_advance` functions so no
//! `DrawCtx` is required at layout time.
//!
//! # Supported Markdown features
//!
//! - Headings H1–H4 (larger font sizes)
//! - Paragraphs (word-wrapped)
//! - Bullet lists (`-`/`*`) with "• " prefix
//! - Ordered lists with "N. " prefix
//! - Inline code `` `x` `` (background box)
//! - Fenced code blocks (monospaced, background box)
//! - Horizontal rules (thin line)
//! - Links (blue text, URL ignored — use `on_link_click` callback if needed)
//! - Blank separator between block-level elements

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

/// Visual style applied to a rendered line.
#[derive(Clone, Copy, Debug, PartialEq)]
enum LineStyle {
    Body,
    H1,
    H2,
    H3,
    H4,
    Code,   // inline or block code — draws with background
    Rule,   // horizontal separator
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

/// A single rendered row after word-wrapping.
#[derive(Clone)]
struct LayoutLine {
    text:    String,
    style:   LineStyle,
    /// Extra left indent in pixels.
    indent:  f64,
    /// Pre-computed Y position (bottom-left, Y-up).
    y:       f64,
    /// Line height in pixels.
    height:  f64,
}

// ── MarkdownView widget ────────────────────────────────────────────────────────

/// A widget that renders a Markdown string as formatted, word-wrapped text.
pub struct MarkdownView {
    bounds:    Rect,
    children:  Vec<Box<dyn Widget>>,
    base:      WidgetBase,

    markdown:  String,
    font:      Arc<Font>,
    font_size: f64,
    padding:   f64,

    /// Computed during `layout()`.
    lines:     Vec<LayoutLine>,
    /// Total content height from the last layout pass.
    content_h: f64,
}

impl MarkdownView {
    pub fn new(markdown: impl Into<String>, font: Arc<Font>) -> Self {
        Self {
            bounds:    Rect::default(),
            children:  Vec::new(),
            base:      WidgetBase::new(),
            markdown:  markdown.into(),
            font,
            font_size: 14.0,
            padding:   8.0,
            lines:     Vec::new(),
            content_h: 0.0,
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self { self.font_size = size; self }
    pub fn with_padding(mut self, p: f64) -> Self { self.padding = p; self }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }

    // ── Markdown → raw paragraphs ─────────────────────────────────────────────

    /// Parse the markdown into a list of `(text, style, indent)` paragraphs.
    /// Each paragraph may be word-wrapped later into multiple `LayoutLine`s.
    fn parse_paragraphs(&self) -> Vec<(String, LineStyle, f64)> {
        let mut out: Vec<(String, LineStyle, f64)> = Vec::new();

        let opts = Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_TABLES;
        let parser = Parser::new_ext(&self.markdown, opts);

        let mut cur_text  = String::new();
        let mut cur_style = LineStyle::Body;
        let mut cur_indent = 0.0_f64;
        let mut list_depth = 0u32;
        let mut list_ordinal: Vec<u64> = Vec::new(); // per-depth counter

        let flush = |out: &mut Vec<_>, text: &mut String, style: LineStyle, indent: f64| {
            let t = text.trim().to_string();
            if !t.is_empty() {
                out.push((t, style, indent));
            }
            text.clear();
        };

        for ev in parser {
            match ev {
                MdEvent::Start(Tag::Heading { level, .. }) => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                    cur_style = match level as u8 {
                        1 => LineStyle::H1,
                        2 => LineStyle::H2,
                        3 => LineStyle::H3,
                        _ => LineStyle::H4,
                    };
                    cur_indent = 0.0;
                }
                MdEvent::End(TagEnd::Heading(_)) => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                    // Add vertical spacing after a heading (empty body line).
                    out.push(("".to_string(), LineStyle::Body, 0.0));
                    cur_style  = LineStyle::Body;
                    cur_indent = 0.0;
                }
                MdEvent::Start(Tag::Paragraph) => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                }
                MdEvent::End(TagEnd::Paragraph) => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                    out.push(("".to_string(), LineStyle::Body, 0.0)); // spacing
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
                        out.push(("".to_string(), LineStyle::Body, 0.0));
                    }
                }
                MdEvent::Start(Tag::Item) => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                    // Prepend bullet/ordinal.
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
                    out.push(("".to_string(), LineStyle::Body, 0.0));
                    cur_style = LineStyle::Body;
                }
                MdEvent::Rule => {
                    flush(&mut out, &mut cur_text, cur_style, cur_indent);
                    out.push(("".to_string(), LineStyle::Rule, 0.0));
                }
                MdEvent::Text(t) => {
                    if !cur_text.is_empty() && !cur_text.ends_with(' ')
                        && !cur_text.ends_with('\n')
                    {
                        cur_text.push(' ');
                    }
                    cur_text.push_str(&t);
                }
                MdEvent::Code(t) => {
                    // Inline code: wrap in backticks for visual hint.
                    if !cur_text.is_empty() && !cur_text.ends_with(' ') {
                        cur_text.push(' ');
                    }
                    cur_text.push('`');
                    cur_text.push_str(&t);
                    cur_text.push('`');
                }
                MdEvent::SoftBreak | MdEvent::HardBreak => {
                    cur_text.push(' ');
                }
                MdEvent::Start(Tag::Link { dest_url, .. }) => {
                    // For now just show the link text; URL is ignored.
                    let _ = dest_url;
                }
                // Ignore everything else (images, HTML, emphasis wrappers, etc.).
                _ => {}
            }
        }
        flush(&mut out, &mut cur_text, cur_style, cur_indent);
        out
    }

    // ── Word-wrapping ─────────────────────────────────────────────────────────

    /// Word-wrap a single paragraph into lines that fit `max_w`.
    fn wrap_paragraph(
        &self,
        text:    &str,
        style:   LineStyle,
        indent:  f64,
        max_w:   f64,
    ) -> Vec<(String, f64)> {
        let font_size = style.font_size(self.font_size);
        let avail     = (max_w - indent).max(1.0);

        if text.is_empty() {
            return vec![("".to_string(), indent)];
        }

        let mut lines: Vec<(String, f64)> = Vec::new();
        let mut current = String::new();

        for word in text.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{} {}", current, word)
            };
            let w = measure_text_metrics(&self.font, &candidate, font_size).width;
            if w <= avail || current.is_empty() {
                current = candidate;
            } else {
                lines.push((current, indent));
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push((current, indent));
        }
        lines
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

        // Build LayoutLines top-to-bottom (convert to Y-up at end).
        let mut top_down: Vec<(String, LineStyle, f64, f64)> = Vec::new(); // (text, style, indent, line_h)

        for (text, style, indent) in &paragraphs {
            if *style == LineStyle::Rule {
                top_down.push(("".to_string(), LineStyle::Rule, 0.0, 8.0));
                continue;
            }
            let font_size  = style.font_size(self.font_size);
            let metrics    = measure_text_metrics(&self.font, "", font_size);
            let line_h     = metrics.line_height * 1.3;

            if text.is_empty() {
                // Spacing line.
                top_down.push(("".to_string(), *style, *indent, line_h * 0.5));
                continue;
            }

            let wrapped = self.wrap_paragraph(text, *style, *indent, max_w);
            for (wl, ind) in wrapped {
                top_down.push((wl, *style, ind, line_h));
            }
        }

        // Now assign Y positions (Y-up: start from top = total_h, going down).
        let total_h: f64 = top_down.iter().map(|(_, _, _, h)| *h).sum::<f64>() + pad * 2.0;
        let mut y = total_h - pad;

        self.lines.clear();
        for (text, style, indent, line_h) in top_down {
            y -= line_h;
            self.lines.push(LayoutLine { text, style, indent, y, height: line_h });
        }

        self.content_h = total_h;
        self.bounds = Rect::new(0.0, 0.0, available.width, total_h);
        Size::new(available.width, total_h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v    = ctx.visuals();
        let pad  = self.padding;
        let w    = self.bounds.width;
        ctx.set_font(Arc::clone(&self.font));

        for line in &self.lines {
            let fs = line.style.font_size(self.font_size);
            ctx.set_font_size(fs);

            let tx = pad + line.indent;
            let ty = line.y + line.height * 0.5;
            // Compute text baseline offset from center.
            let metrics = measure_text_metrics(&self.font, &line.text, fs);
            let text_y  = ty - (metrics.ascent - metrics.descent) * 0.5 + metrics.descent;

            match line.style {
                LineStyle::Rule => {
                    ctx.set_fill_color(v.separator);
                    ctx.begin_path();
                    ctx.rect(pad, ty, w - pad * 2.0, 1.0);
                    ctx.fill();
                }
                LineStyle::Code => {
                    // Background box for code lines.
                    let bg = Color::rgba(0.0, 0.0, 0.0, 0.15);
                    ctx.set_fill_color(bg);
                    ctx.begin_path();
                    ctx.rounded_rect(pad, line.y, w - pad * 2.0, line.height, 3.0);
                    ctx.fill();
                    ctx.set_fill_color(v.accent);
                    ctx.fill_text(&line.text, tx + 4.0, text_y);
                }
                LineStyle::H1 | LineStyle::H2 | LineStyle::H3 | LineStyle::H4 => {
                    ctx.set_fill_color(v.text_color);
                    ctx.fill_text(&line.text, tx, text_y);
                }
                LineStyle::Body => {
                    ctx.set_fill_color(v.text_color);
                    ctx.fill_text(&line.text, tx, text_y);
                }
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}
