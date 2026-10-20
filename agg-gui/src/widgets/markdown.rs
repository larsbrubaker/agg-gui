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
//! **Remote images (`http(s)://`) load only through a host-supplied
//! [`RemoteImageLoader`]**: the view's own
//! ([`MarkdownView::with_remote_image_loader`]), the painting thread's
//! ([`set_remote_image_loader_on_this_thread`]) or the process default
//! ([`set_default_remote_image_loader`]). With none installed the view makes
//! no network request and shows the image's placeholder. Install
//! [`HttpRemoteImageLoader`] to have agg-gui fetch them itself
//! (`image_loader.rs`). A `file://` picture goes to an installed loader the
//! same way, and [`MarkdownView::with_image_url_resolver`] turns an image's
//! source url into the url that loads (agg-sharp's `ResolveImageSource`).
//!
//! # Supported Markdown features
//!
//! - Headings H1–H4 (larger font sizes)
//! - Paragraphs (word-wrapped)
//! - Bullet lists (`-`/`*`) with "• " prefix
//! - Ordered lists with "N. " prefix
//! - Inline code `` `x` `` (highlight)
//! - Emphasis: italic, bold, both, strikethrough, and paired `<b>`/`<i>`/`<s>`
//!   style tags ([`MarkdownView::with_bold_font`], `emphasis.rs`)
//! - Fenced code blocks (background box) in a monospace face
//!   ([`MarkdownView::with_code_font`]; the body face when none is given)
//! - Horizontal rules (thin separator line)
//! - Images via `image_provider` callback; compact inline placeholder when unavailable
//! - Animated GIFs (remote, through a [`RemoteImageLoader`], or from [`MarkdownView::with_image_sequence_provider`])
//!   play at their frame delays once loaded and painted (`animated_image.rs`)
//! - Links (coloured text, URL is not opened — add `on_link_click` if needed),
//!   in paragraphs and in table cells (`cell_links.rs`)
//! - Matching-text highlight boxes ([`MarkdownView::set_matching_text`])
//! - Pipe and grid tables, whose cells hold inline images (`cell_images.rs`)
//! - Markdown as inline-styled HTML for the clipboard ([`to_styled_html`],
//!   `styled_html.rs`), which the rich editor's copy writes
//! - Copy (MarkdigAgg `GetSelectionClipboardData`): the selected words as
//!   plain text plus their styled HTML (`rich_html.rs`, styled with
//!   [`MarkdownView::with_html_theme`])
//!
//! # Colours
//!
//! A view draws in the app's visuals (`DrawCtx::visuals`).  An owner themed
//! apart from the app hands it a [`MarkdownColors`]
//! ([`MarkdownView::with_colors`], `colors.rs`): the rich editor does so for
//! its Raw block hosts, so a dark editor's raw blocks follow its theme.
//!
//! # Updating in place
//!
//! [`MarkdownView::set_markdown`] replaces the text of a live view, e.g. while
//! a reply streams in. Every layout re-parses the markdown, so the view keeps
//! its selection (clamped to the new text), its code-block and table scroll
//! offsets and its decoded images; a host that rebuilt the view instead would
//! lose them.

use std::sync::{Arc, Mutex};

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

mod alignment;
#[cfg(test)]
mod alignment_tests;
mod animated_image;
#[cfg(test)]
mod animated_image_tests;
mod block_scroll;
mod builders;
mod cell_images;
mod cell_links;
#[cfg(test)]
mod clipboard_selection_tests;
mod colors;
mod emphasis;
#[cfg(test)]
mod end_spacing_tests;
mod event;
#[cfg(test)]
mod extension_tests;
mod extensions;
#[cfg(test)]
mod feature_rendering_tests;
#[cfg(test)]
mod grid_table_tests;
mod grid_tables;
mod html_form;
#[cfg(test)]
mod html_form_tests;
mod html_rewriter;
mod image_context;
pub(crate) mod image_loader;
mod introspect;
mod layout;
#[cfg(test)]
mod line_width_tests;
mod matching;
mod paint;
mod parse;
mod placeholder_icon;
#[cfg(test)]
mod placeholder_icon_tests;
#[cfg(test)]
mod plain_text_tests;
#[cfg(test)]
mod remote_image_tests;
mod rich_html;
mod selection;
#[cfg(test)]
mod selection_tests;
mod styled_html;
#[cfg(test)]
mod styled_html_tests;
mod table_grid;
#[cfg(test)]
mod table_grid_tests;
#[cfg(test)]
mod table_image_tests;
mod table_style;
#[cfg(test)]
mod table_tests;
#[cfg(test)]
mod update_tests;
#[cfg(test)]
mod wheel_tests;

pub use alignment::MarkdownAlign;

/// The gap a paragraph, heading or list leaves below itself, in lines of the
/// body font, unless the view is given another
/// ([`MarkdownView::with_paragraph_spacing`]).
pub const DEFAULT_PARAGRAPH_SPACING: f64 = 0.65;
use block_scroll::{
    clamp_block_offset, scrollbar_rect, scrollbar_thumb, BLOCK_SCROLLBAR_GAP, BLOCK_SCROLLBAR_H,
};
pub use colors::MarkdownColors;
pub use emphasis::MarkdownRunStyle;
pub use html_form::{markdown_html, markdown_html_collapsed};
pub use image_loader::{
    clear_remote_image_loader_on_this_thread, default_remote_image_loader,
    remote_image_loader_on_this_thread, remote_images_in_flight,
    remote_images_in_flight_on_this_thread, set_default_remote_image_loader,
    set_remote_image_loader_on_this_thread, HttpRemoteImageLoader, RemoteImageDone,
    RemoteImageLoader,
};
pub use introspect::{MarkdownBlockKind, MarkdownTableCell, MarkdownTextBlock, MarkdownWord};
pub use styled_html::{css_color, style_html_fragment, to_styled_html, HtmlStyleTheme};

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
        /// The row continues the previous row's paragraph (a soft wrap), so
        /// the plain text joins the two with a space rather than a newline.
        continues: bool,
        /// The row's paragraph or heading alignment (`alignment.rs`).
        align: MarkdownAlign,
        /// The heading level of the row's block, 0 for a paragraph.
        level: u8,
        y: f64,
        height: f64,
    },
    Table {
        block_idx: usize,
        rows: Vec<Vec<String>>,
        /// Each cell's emphasis spans (see `emphasis.rs`).
        cell_styles: emphasis::CellStyles,
        /// Each cell's inline images, sized (see `cell_images.rs`).
        cell_images: cell_images::LaidCellImages,
        /// Whether the first row is a header row, drawn bold.
        header: bool,
        /// Each column's alignment, from its `:---:` marker.
        aligns: Vec<MarkdownAlign>,
        y: f64,
        height: f64,
        /// Each row's height, top row first: a text row's height, or more
        /// for a row whose images are taller.
        row_heights: Vec<f64>,
        /// Each column's cell width, between its grid lines.
        col_widths: Vec<f64>,
        /// One grid line in logical units, whole device pixels at the
        /// device scale the table was laid out at (`table_grid.rs`).
        line: f64,
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
        /// Bold / italic / strikethrough (see `emphasis.rs`).
        style: MarkdownRunStyle,
        /// Part of a matching-text box (see `matching.rs`).
        highlight: bool,
        /// Box padding before the text (the run begins a match).
        lead: f64,
        /// Box padding after the text (the run ends a match).
        trail: f64,
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
        style: MarkdownRunStyle,
        /// The text follows the previous item with no whitespace between
        /// them (`**bold**,`), so its first word gets no leading space.
        glue: bool,
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
        /// Set by the `<div align>` around it or its own aligned element (`alignment.rs`).
        align: MarkdownAlign,
        /// The heading level (1-6), 0 for a paragraph or list item.
        level: u8,
    },
    Table {
        rows: Vec<Vec<String>>,
        cell_styles: emphasis::CellStyles,
        /// Each cell's inline images, where they sit in its text.
        cell_images: cell_images::CellImages,
        /// Whether the first row is a header row (a grid table may have none).
        header: bool,
        /// Each column's alignment, from its `:---:` marker.
        aligns: Vec<MarkdownAlign>,
    },
    CodeBlock(Vec<String>),
    Spacer,
    Rule,
}

// ── Image cache entry ──────────────────────────────────────────────────────────

struct ImageEntry {
    /// The url the markdown names (layout and context menus look entries up by it).
    url: String,
    /// The url the pictures load from: `url` after the view's resolver
    /// ([`MarkdownView::with_image_url_resolver`]).
    load_url: String,
    state: Arc<Mutex<ImageState>>,
    /// Playback of an animated image (`animated_image.rs`).
    animation: animated_image::AnimationClock,
}

#[derive(Clone, Copy, Debug, Default)]
struct BlockScroll {
    offset: f64,
    dragging: bool,
    drag_thumb_offset: f64,
}

#[derive(Clone)]
pub(crate) struct ImagePixels {
    pub(crate) data: Arc<Vec<u8>>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// The frames of an animated image; `data` is its first frame.
    pub(crate) sequence: Option<Arc<crate::image_sequence::ImageSequence>>,
}

/// An image's load state, shared with the rich editor's image cache
/// (`markdown_editing::image_cache`), which loads through the same fetcher.
pub(crate) enum ImageState {
    RemotePending,
    Loading,
    Ready { image: ImagePixels, seen: bool },
    Failed,
}

// ── MarkdownView widget ────────────────────────────────────────────────────────

/// Resolves an image URL to decoded RGBA `(pixels, width, height)`.
type ImageProvider = Box<dyn Fn(&str) -> Option<(Vec<u8>, u32, u32)>>;

/// Resolves an image URL to an animated [`crate::image_sequence::ImageSequence`].
type SequenceProvider = Box<dyn Fn(&str) -> Option<crate::image_sequence::ImageSequence>>;

/// Turns an image's source url into the url to load.
type ImageUrlResolver = Box<dyn Fn(&str) -> String>;

/// A callback handed a link or image URL.
type UrlCallback = Box<dyn FnMut(&str)>;

/// A widget that renders a Markdown string as formatted, word-wrapped text
/// with optional image support.
pub struct MarkdownView {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,

    markdown: String,
    font: Arc<Font>,
    /// The face bold runs draw with ([`MarkdownView::with_bold_font`]).
    bold_font: Option<Arc<Font>>,
    /// The monospace face code blocks draw with ([`MarkdownView::with_code_font`]).
    code_font: Option<Arc<Font>>,
    /// The regular, italic, bold and bold-italic faces, built on first use.
    styled_fonts: emphasis::StyledFontCache,
    font_size: f64,
    padding: f64,

    /// Optional image decoder.  Receives a URL/path, returns RGBA8 pixel data
    /// (top-row first) + (width, height), or `None` if unavailable.
    image_provider: Option<ImageProvider>,
    /// Optional animated-image source, asked before `image_provider`.
    sequence_provider: Option<SequenceProvider>,
    /// Fetches remote images ([`MarkdownView::with_remote_image_loader`]);
    /// `None` falls back to the process default (`image_loader.rs`).
    remote_image_loader: Option<Arc<dyn RemoteImageLoader>>,
    /// Turns an image's source url into the url that loads
    /// ([`MarkdownView::with_image_url_resolver`]).
    image_url_resolver: Option<ImageUrlResolver>,

    /// Cached image data, indexed by `LineRun::Image::cache_idx`.
    image_cache: Vec<ImageEntry>,

    /// Laid-out items (populated by `layout()`).
    items: Vec<LayoutItem>,
    /// Total content height from the last layout pass.
    content_h: f64,
    /// Widest text row from the last layout pass (see [`MarkdownView::max_line_width`]).
    max_line_w: f64,
    on_link_click: Option<UrlCallback>,
    on_image_open: Option<UrlCallback>,
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
    suppress_next_left_mouse_up: bool,
    /// Text whose occurrences are highlighted (`MarkdownWidget.MatchingText`).
    matching_text: Option<String>,
    /// The look copied HTML carries (C#'s `MarkdownWidget.Theme`).
    html_theme: HtmlStyleTheme,
    /// The view's own theme colours; `None` draws in the app's visuals.
    colors: Option<MarkdownColors>,
    /// The colour emphasised (italic) text draws in
    /// ([`MarkdownView::with_emphasis_color`]); `None` draws it as body text.
    emphasis_color: Option<crate::color::Color>,
    /// Whether the gap after a block is kept after the document's last block
    /// ([`MarkdownView::with_end_spacing`]).
    end_spacing: bool,
    /// The gap after a block, in lines of its font
    /// ([`MarkdownView::with_paragraph_spacing`]).
    paragraph_spacing: f64,
    /// The icon image placeholders show (`placeholder_icon.rs`).
    link_placeholder_icon: Option<crate::icon_image::IconImage>,
}

impl MarkdownView {
    pub fn new(markdown: impl Into<String>, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            markdown: markdown.into(),
            font,
            bold_font: None,
            code_font: None,
            styled_fonts: Default::default(),
            font_size: 14.0,
            padding: 8.0,
            image_provider: None,
            sequence_provider: None,
            remote_image_loader: None,
            image_url_resolver: None,
            image_cache: Vec::new(),
            items: Vec::new(),
            content_h: 0.0,
            max_line_w: 0.0,
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
            suppress_next_left_mouse_up: false,
            matching_text: None,
            html_theme: HtmlStyleTheme::default(),
            colors: None,
            emphasis_color: None,
            end_spacing: true,
            paragraph_spacing: DEFAULT_PARAGRAPH_SPACING,
            link_placeholder_icon: None,
        }
    }

    /// Currently-active font — honours the thread-local system-font override
    /// (`font_settings::current_system_font`) so system-font changes propagate
    /// live without rebuilding the markdown view.
    fn active_font(&self) -> Arc<Font> {
        crate::font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.font))
    }

    /// The url of the link drawn under `pos` (this view's coordinates), as a
    /// click would follow it; the rich editor reads links in its Raw blocks'
    /// rendered hosts through it (agg-sharp's `RichEditorMouse.RawLinkAt`).
    pub fn link_at_point(&self, pos: Point) -> Option<&str> {
        self.link_at(pos)
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
                        } if point_in_rect(pos, tx + x, y + y_offset, *width, *height) => {
                            return Some(url);
                        }
                        _ => {}
                    }
                }
            }
        }
        self.table_link_at(pos)
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
    // `root_transform` yields root-target DEVICE pixels, while
    // `current_viewport()` is LOGICAL (App::layout divides by the effective
    // scale), so scale the viewport up before comparing.  The paint clip
    // below is already in root device pixels.
    let viewport = crate::widget::current_viewport();
    let scale = crate::ux_scale::effective_scale();
    let (viewport_w, viewport_h) = (viewport.width * scale, viewport.height * scale);
    let root_visible = max_x >= 0.0 && min_x <= viewport_w && max_y >= 0.0 && min_y <= viewport_h;
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

pub(crate) fn is_fetchable_url(url: &str) -> bool {
    !url.is_empty()
        && !url.starts_with('#')
        && !url.starts_with("file://")
        && !url.starts_with("data:")
}

impl Widget for MarkdownView {
    crate::widgets::widget_as_any!();
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

    fn layout(&mut self, available: Size) -> Size {
        self.layout_markdown(available)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.paint_markdown(ctx);
    }

    fn hit_test_global_overlay(&self, local_pos: Point) -> bool {
        self.context_menu_contains(local_pos)
    }

    fn has_active_modal(&self) -> bool {
        self.context_menu.is_some()
    }

    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.paint_context_menu(ctx);
    }

    fn find_named_targets(&self, name: &str, out: &mut Vec<crate::NamedTarget>) {
        self.context_menu_named_targets(name, out);
    }

    fn needs_draw(&self) -> bool {
        if !self.is_visible() {
            return false;
        }
        self.images_need_draw()
            || self.animation_frame_due()
            || self.children().iter().any(|c| c.needs_draw())
    }

    fn next_draw_deadline(&self) -> Option<web_time::Instant> {
        if !self.is_visible() {
            return None;
        }
        self.next_animation_deadline()
            .into_iter()
            .chain(self.children.iter().filter_map(|c| c.next_draw_deadline()))
            .min()
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        self.handle_markdown_event(event)
    }

    fn is_focusable(&self) -> bool {
        true
    }

    /// A press on the view opens and chooses from its right-click menu
    /// (`markdown/image_context.rs`), so when an app turns off its click
    /// focus ([`WidgetBase::focus_on_click`]) the press leaves keyboard
    /// focus where it was rather than clearing it.
    fn keeps_focus_on_press(&self) -> bool {
        true
    }
}
