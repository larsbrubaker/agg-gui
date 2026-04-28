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

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

mod image_loader;
mod event;
mod image_context;
mod layout;
mod paint;
mod parse;
mod selection;

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
        block_idx: usize,
        rows: Vec<Vec<String>>,
        y: f64,
        height: f64,
        row_h: f64,
        col_widths: Vec<f64>,
        viewport_width: f64,
        content_width: f64,
    },
    CodeBlock {
        block_idx: usize,
        lines: Vec<String>,
        y: f64,
        height: f64,
        line_h: f64,
        viewport_width: f64,
        content_width: f64,
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
        url: String,
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

#[derive(Clone, Copy, Debug, Default)]
struct BlockScroll {
    offset: f64,
    dragging: bool,
    drag_thumb_offset: f64,
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
    on_image_open: Option<Box<dyn FnMut(&str)>>,
    block_scrolls: Vec<BlockScroll>,
    focused: bool,
    selecting_drag: bool,
    selection_anchor: Option<usize>,
    selection_cursor: Option<usize>,
    selection_drag_start: Option<Point>,
    selection_dragged: bool,
    selectable_text: String,
    selectable_fragments: Vec<selection::SelectableFragment>,
    context_menu: Option<image_context::MarkdownContextMenuState>,
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
            on_image_open: None,
            block_scrolls: Vec::new(),
            focused: false,
            selecting_drag: false,
            selection_anchor: None,
            selection_cursor: None,
            selection_drag_start: None,
            selection_dragged: false,
            selectable_text: String::new(),
            selectable_fragments: Vec::new(),
            context_menu: None,
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

    pub fn on_image_open(mut self, cb: impl FnMut(&str) + 'static) -> Self {
        self.on_image_open = Some(Box::new(cb));
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
                            url: _,
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

    fn block_scroll_mut(&mut self, block_idx: usize) -> &mut BlockScroll {
        if block_idx >= self.block_scrolls.len() {
            self.block_scrolls
                .resize(block_idx + 1, BlockScroll::default());
        }
        &mut self.block_scrolls[block_idx]
    }

    fn block_scroll_offset(&self, block_idx: usize) -> f64 {
        self.block_scrolls
            .get(block_idx)
            .map(|s| s.offset)
            .unwrap_or(0.0)
    }

    fn hit_scrollbar(&self, pos: Point) -> Option<BlockHit> {
        for item in &self.items {
            match item {
                LayoutItem::Table {
                    block_idx,
                    y,
                    height,
                    viewport_width,
                    content_width,
                    ..
                }
                | LayoutItem::CodeBlock {
                    block_idx,
                    y,
                    height,
                    viewport_width,
                    content_width,
                    ..
                } if *content_width > *viewport_width => {
                    let bar = scrollbar_rect(*y, *viewport_width);
                    if point_in_rect(pos, self.padding + bar.x, bar.y, bar.width, bar.height) {
                        let offset = self.block_scroll_offset(*block_idx);
                        let thumb = scrollbar_thumb(bar, *viewport_width, *content_width, offset);
                        let thumb_hit = point_in_rect(
                            pos,
                            self.padding + thumb.x,
                            thumb.y,
                            thumb.width,
                            thumb.height,
                        );
                        return Some(BlockHit {
                            block_idx: *block_idx,
                            viewport_width: *viewport_width,
                            content_width: *content_width,
                            bar,
                            thumb,
                            on_thumb: thumb_hit,
                        });
                    }
                    let _ = height;
                }
                _ => {}
            }
        }
        None
    }

    fn point_over_scrollable_block(&self, pos: Point) -> Option<(usize, f64, f64)> {
        for item in &self.items {
            match item {
                LayoutItem::Table {
                    block_idx,
                    y,
                    height,
                    viewport_width,
                    content_width,
                    ..
                }
                | LayoutItem::CodeBlock {
                    block_idx,
                    y,
                    height,
                    viewport_width,
                    content_width,
                    ..
                } if *content_width > *viewport_width
                    && point_in_rect(pos, self.padding, *y, *viewport_width, *height) =>
                {
                    return Some((*block_idx, *viewport_width, *content_width));
                }
                _ => {}
            }
        }
        None
    }

    fn block_metrics(&self, block_idx: usize) -> Option<(Rect, f64, f64)> {
        self.items.iter().find_map(|item| match item {
            LayoutItem::Table {
                block_idx: idx,
                y,
                viewport_width,
                content_width,
                ..
            }
            | LayoutItem::CodeBlock {
                block_idx: idx,
                y,
                viewport_width,
                content_width,
                ..
            } if *idx == block_idx && *content_width > *viewport_width => Some((
                scrollbar_rect(*y, *viewport_width),
                *viewport_width,
                *content_width,
            )),
            _ => None,
        })
    }

    fn dragging_block(&self) -> Option<usize> {
        self.block_scrolls
            .iter()
            .enumerate()
            .find_map(|(idx, scroll)| scroll.dragging.then_some(idx))
    }

    fn scroll_block_to(
        &mut self,
        block_idx: usize,
        offset: f64,
        viewport: f64,
        content: f64,
    ) -> bool {
        let scroll = self.block_scroll_mut(block_idx);
        let next = clamp_block_offset(offset, viewport, content);
        let changed = (next - scroll.offset).abs() > 1e-6;
        scroll.offset = next;
        changed
    }

    fn drag_block_scrollbar(&mut self, block_idx: usize, pos: Point) -> bool {
        let Some((bar, viewport, content)) = self.block_metrics(block_idx) else {
            return false;
        };
        let offset = self.block_scroll_offset(block_idx);
        let thumb = scrollbar_thumb(bar, viewport, content, offset);
        let drag_thumb_offset = self
            .block_scrolls
            .get(block_idx)
            .map(|scroll| scroll.drag_thumb_offset)
            .unwrap_or(0.0);
        let travel = (bar.width - thumb.width).max(1.0);
        let raw_start = pos.x - self.padding - drag_thumb_offset;
        let frac = ((raw_start - bar.x) / travel).clamp(0.0, 1.0);
        self.scroll_block_to(
            block_idx,
            frac * (content - viewport).max(0.0),
            viewport,
            content,
        )
    }
}

fn point_in_rect(pos: Point, x: f64, y: f64, w: f64, h: f64) -> bool {
    pos.x >= x && pos.x <= x + w && pos.y >= y && pos.y <= y + h
}

#[derive(Clone, Copy)]
struct BlockHit {
    block_idx: usize,
    viewport_width: f64,
    content_width: f64,
    bar: Rect,
    thumb: Rect,
    on_thumb: bool,
}

pub(super) const BLOCK_SCROLLBAR_H: f64 = 10.0;
pub(super) const BLOCK_SCROLLBAR_GAP: f64 = 4.0;
const BLOCK_SCROLLBAR_MIN_THUMB: f64 = 24.0;

fn scrollbar_rect(block_y: f64, viewport_width: f64) -> Rect {
    Rect::new(0.0, block_y + 1.0, viewport_width, BLOCK_SCROLLBAR_H)
}

fn scrollbar_thumb(bar: Rect, viewport_width: f64, content_width: f64, offset: f64) -> Rect {
    let ratio = (viewport_width / content_width).clamp(0.0, 1.0);
    let thumb_w = (bar.width * ratio)
        .max(BLOCK_SCROLLBAR_MIN_THUMB)
        .min(bar.width);
    let travel = (bar.width - thumb_w).max(0.0);
    let max_scroll = (content_width - viewport_width).max(0.0);
    let x = if max_scroll > 0.0 {
        bar.x + travel * (offset / max_scroll).clamp(0.0, 1.0)
    } else {
        bar.x
    };
    Rect::new(x, bar.y, thumb_w, bar.height)
}

fn clamp_block_offset(offset: f64, viewport_width: f64, content_width: f64) -> f64 {
    offset
        .clamp(0.0, (content_width - viewport_width).max(0.0))
        .round()
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
        self.handle_markdown_event(event)
    }

    fn is_focusable(&self) -> bool {
        true
    }
}
