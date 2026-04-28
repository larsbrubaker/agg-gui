//! `MarkdownView` — render a Markdown string as formatted text with images.
//!
//! Uses `pulldown-cmark` for parsing, then converts the event stream into a
//! flat list of styled lines, inline image runs, and image placeholders. Word-wrapping is
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
//! - Images via `image_provider` callback; compact inline placeholder when unavailable
//! - Links (coloured text, URL is not opened — add `on_link_click` if needed)

use std::sync::{Arc, Mutex};

use crate::cursor::{set_cursor_icon, CursorIcon};
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

mod image_loader;
mod layout;
mod paint;
mod parse;

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
            LineStyle::H1 => base * 1.8,
            LineStyle::H2 => base * 1.5,
            LineStyle::H3 => base * 1.25,
            LineStyle::H4 => base * 1.1,
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
        runs: Vec<LineRun>,
        style: LineStyle,
        indent: f64,
        quote: bool,
        y: f64,
        height: f64,
    },
    Table {
        rows: Vec<Vec<String>>,
        y: f64,
        height: f64,
        row_h: f64,
        col_widths: Vec<f64>,
    },
    CodeBlock {
        lines: Vec<String>,
        y: f64,
        height: f64,
        line_h: f64,
        width: f64,
    },
}

#[derive(Clone)]
enum LineRun {
    Text {
        text: String,
        link: Option<String>,
        code: bool,
        x: f64,
        width: f64,
    },
    Image {
        alt: String,
        link: Option<String>,
        cache_idx: usize,
        x: f64,
        y_offset: f64,
        width: f64,
        height: f64,
    },
}

// ── Intermediate paragraph item (before layout) ────────────────────────────────

#[derive(Clone)]
enum InlineItem {
    Text {
        text: String,
        link: Option<String>,
        code: bool,
    },
    Image {
        url: String,
        alt: String,
        link: Option<String>,
    },
}

enum ParagraphItem {
    Flow {
        items: Vec<InlineItem>,
        style: LineStyle,
        indent: f64,
        quote: bool,
    },
    Table(Vec<Vec<String>>),
    CodeBlock(Vec<String>),
    Spacer,
    Rule,
}

// ── Image cache entry ──────────────────────────────────────────────────────────

struct ImageEntry {
    url: String,
    state: Arc<Mutex<ImageState>>,
}

#[derive(Clone)]
struct ImagePixels {
    data: Arc<Vec<u8>>,
    width: u32,
    height: u32,
}

enum ImageState {
    RemotePending,
    Loading,
    Ready { image: ImagePixels, seen: bool },
    Failed,
}

// ── MarkdownView widget ────────────────────────────────────────────────────────

/// A widget that renders a Markdown string as formatted, word-wrapped text
/// with optional image support.
pub struct MarkdownView {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,

    markdown: String,
    font: Arc<Font>,
    font_size: f64,
    padding: f64,

    /// Optional image decoder.  Receives a URL/path, returns RGBA8 pixel data
    /// (top-row first) + (width, height), or `None` if unavailable.
    image_provider: Option<Box<dyn Fn(&str) -> Option<(Vec<u8>, u32, u32)>>>,

    /// Cached image data, indexed by `LineRun::Image::cache_idx`.
    image_cache: Vec<ImageEntry>,

    /// Laid-out items (populated by `layout()`).
    items: Vec<LayoutItem>,
    /// Total content height from the last layout pass.
    content_h: f64,
    on_link_click: Option<Box<dyn FnMut(&str)>>,
}

impl MarkdownView {
    pub fn new(markdown: impl Into<String>, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            markdown: markdown.into(),
            font,
            font_size: 14.0,
            padding: 8.0,
            image_provider: None,
            image_cache: Vec::new(),
            items: Vec::new(),
            content_h: 0.0,
            on_link_click: None,
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }
    pub fn with_padding(mut self, p: f64) -> Self {
        self.padding = p;
        self
    }

    /// Currently-active font — honours the thread-local system-font override
    /// (`font_settings::current_system_font`) so system-font changes propagate
    /// live without rebuilding the markdown view.
    fn active_font(&self) -> Arc<Font> {
        crate::font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.font))
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

    pub fn on_link_click(mut self, cb: impl FnMut(&str) + 'static) -> Self {
        self.on_link_click = Some(Box::new(cb));
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

    // ── Image cache management ────────────────────────────────────────────────

    /// Return the cache index for `url`, loading it via the provider if not yet cached.
    fn get_or_load_image(&mut self, url: &str) -> usize {
        // Check if already cached.
        if let Some(idx) = self.image_cache.iter().position(|e| e.url == url) {
            return idx;
        }

        let state = Arc::new(Mutex::new(
            if let Some((data, width, height)) = self.image_provider.as_ref().and_then(|p| p(url)) {
                ImageState::Ready {
                    image: ImagePixels {
                        data: Arc::new(data),
                        width,
                        height,
                    },
                    seen: false,
                }
            } else if is_remote_url(url) {
                ImageState::RemotePending
            } else {
                ImageState::Failed
            },
        ));

        let idx = self.image_cache.len();
        self.image_cache.push(ImageEntry {
            url: url.to_string(),
            state,
        });
        idx
    }

    fn link_at(&self, pos: Point) -> Option<&str> {
        let pad = self.padding;
        for item in &self.items {
            if let LayoutItem::Line {
                runs,
                indent,
                y,
                height,
                ..
            } = item
            {
                let tx = pad + indent;
                for run in runs {
                    match run {
                        LineRun::Text {
                            link: Some(url),
                            x,
                            width,
                            ..
                        } => {
                            if point_in_rect(pos, tx + x, *y, *width, *height) {
                                return Some(url);
                            }
                        }
                        LineRun::Image {
                            link: Some(url),
                            x,
                            y_offset,
                            width,
                            height,
                            ..
                        } => {
                            if point_in_rect(pos, tx + x, y + y_offset, *width, *height) {
                                return Some(url);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        None
    }
}

fn point_in_rect(pos: Point, x: f64, y: f64, w: f64, h: f64) -> bool {
    pos.x >= x && pos.x <= x + w && pos.y >= y && pos.y <= y + h
}

fn is_rect_visible_in_root(ctx: &dyn DrawCtx, x: f64, y: f64, w: f64, h: f64) -> bool {
    let mut points = [(x, y), (x + w, y), (x, y + h), (x + w, y + h)];
    let transform = ctx.root_transform();
    for (px, py) in &mut points {
        transform.transform(px, py);
    }
    let min_x = points.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
    let max_x = points
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = points.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
    let max_y = points
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::NEG_INFINITY, f64::max);
    let viewport = crate::widget::current_viewport();
    let root_visible =
        max_x >= 0.0 && min_x <= viewport.width && max_y >= 0.0 && min_y <= viewport.height;
    if !root_visible {
        return false;
    }

    if let Some(clip) = crate::widget::current_paint_clip() {
        max_x >= clip.x
            && min_x <= clip.x + clip.width
            && max_y >= clip.y
            && min_y <= clip.y + clip.height
    } else {
        true
    }
}

fn is_remote_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

impl Widget for MarkdownView {
    fn type_name(&self) -> &'static str {
        "MarkdownView"
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

    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }

    fn layout(&mut self, available: Size) -> Size {
        self.layout_markdown(available)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.paint_markdown(ctx);
    }

    fn needs_draw(&self) -> bool {
        if !self.is_visible() {
            return false;
        }
        self.image_cache.iter().any(|entry| {
            entry
                .state
                .lock()
                .map(|state| {
                    matches!(
                        *state,
                        ImageState::Loading | ImageState::Ready { seen: false, .. }
                    )
                })
                .unwrap_or(false)
        }) || self.children().iter().any(|c| c.needs_draw())
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                if self.link_at(*pos).is_some() {
                    set_cursor_icon(CursorIcon::PointingHand);
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } if self.link_at(*pos).is_some() => EventResult::Consumed,
            Event::MouseUp {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                let url = self.link_at(*pos).map(str::to_string);
                if let Some(url) = url {
                    if let Some(cb) = self.on_link_click.as_mut() {
                        cb(&url);
                    }
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }
}
