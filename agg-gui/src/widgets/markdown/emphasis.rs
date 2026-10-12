//! Emphasis styling for `MarkdownView` — agg-sharp MarkdigAgg's
//! `AggEmphasisInlineRenderer`/`EmphasisInlineX`, `HtmlStyleTags` and
//! `AggHtmlInlineRenderer`.
//!
//! A single `*`/`_` slants its words (italic), a double one makes them bold,
//! `***x***` does both, and `~~x~~` strikes them through; the styles nest, so
//! a word takes every style of the spans around it (C# styles every
//! `TextWidget` below a span, whatever the nesting order). Bare inline HTML
//! style tags — `<strong>`/`<b>`, `<em>`/`<i>`, `<del>`/`<s>`/`<strike>`, any
//! letter case, no attributes — style the words between an opening tag and
//! its close the same way, when the tags pair up in order among the direct
//! children of one inline container; an unpaired, attributed or crossed tag
//! is dropped like any other inline HTML.
//!
//! Drawing follows agg-sharp's `TextWidget`: italic is a faux shear
//! (`Font::variant_with_faux_style`, `StyledTypeFace.FauxItalic`) that keeps
//! every advance, bold draws with the host's bold face
//! ([`MarkdownView::with_bold_font`], C# `AggContext.DefaultFontBold`) — or the
//! regular face thickened when the host has none — and so measures with it,
//! and strikethrough is a 1 px line through the middle of the text.
//!
//! `parse.rs` keeps a [`StyleStack`] while walking pulldown-cmark's events,
//! `layout.rs` measures each run with [`MarkdownView::run_font`], and
//! `paint.rs` draws with it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use pulldown_cmark::Event as MdEvent;

use crate::text::Font;

use super::MarkdownView;

/// The emphasis of one laid-out run of text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MarkdownRunStyle {
    /// Drawn with the bold face (`**x**`, `__x__`, `<b>`, `<strong>`).
    pub bold: bool,
    /// Slanted (`*x*`, `_x_`, `<i>`, `<em>`).
    pub italic: bool,
    /// Struck through (`~~x~~`, `<del>`, `<s>`, `<strike>`).
    pub strikethrough: bool,
}

/// The style a paired HTML tag applies (C# `HtmlStyleTags.Kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HtmlStyle {
    Bold,
    Italic,
    Strike,
}

/// How many open spans of each style surround the current text.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct StyleStack {
    bold: u32,
    italic: u32,
    strike: u32,
}

impl StyleStack {
    pub(super) fn style(&self) -> MarkdownRunStyle {
        MarkdownRunStyle {
            bold: self.bold > 0,
            italic: self.italic > 0,
            strikethrough: self.strike > 0,
        }
    }

    /// Open (`open == true`) or close one span of `kind`.
    pub(super) fn apply(&mut self, kind: HtmlStyle, open: bool) {
        let counter = match kind {
            HtmlStyle::Bold => &mut self.bold,
            HtmlStyle::Italic => &mut self.italic,
            HtmlStyle::Strike => &mut self.strike,
        };
        if open {
            *counter += 1;
        } else {
            *counter = counter.saturating_sub(1);
        }
    }
}

/// The lower-case element name of a bare style tag (`<b>`, `</EM>`), or
/// `None` for any other HTML, including a style tag with attributes.
fn style_tag_name(tag: &str) -> Option<(String, bool)> {
    let inner = tag.strip_prefix('<')?.strip_suffix('>')?;
    let (closing, name) = match inner.strip_prefix('/') {
        Some(name) => (true, name),
        None => (false, inner),
    };
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "strong" | "b" | "em" | "i" | "del" | "s" | "strike"
    )
    .then_some((name, closing))
}

fn style_of(name: &str) -> HtmlStyle {
    match name {
        "strong" | "b" => HtmlStyle::Bold,
        "em" | "i" => HtmlStyle::Italic,
        _ => HtmlStyle::Strike,
    }
}

/// The inline HTML style tags among `events` that pair up in order within
/// their container (C# `HtmlStyleTags.Pair`), by event index: the style and
/// `true` for the opening tag, `false` for the closing one. Every `Start`
/// event opens a new container and its `End` closes it, so a tag pairs only
/// with tags that are siblings of it. Unpaired and crossed tags are left out.
pub(super) fn pair_style_tags(events: &[MdEvent<'_>]) -> HashMap<usize, (HtmlStyle, bool)> {
    let mut paired = HashMap::new();
    // One list of still-open tags per container on the path from the root.
    let mut containers: Vec<Vec<(usize, String)>> = vec![Vec::new()];
    for (idx, event) in events.iter().enumerate() {
        match event {
            MdEvent::Start(_) => containers.push(Vec::new()),
            MdEvent::End(_) => {
                if containers.len() > 1 {
                    containers.pop();
                }
            }
            MdEvent::InlineHtml(tag) => {
                let Some((name, closing)) = style_tag_name(tag) else {
                    continue;
                };
                let Some(open) = containers.last_mut() else {
                    continue;
                };
                if !closing {
                    open.push((idx, name));
                } else if open.last().is_some_and(|(_, last)| *last == name) {
                    if let Some((open_idx, _)) = open.pop() {
                        let style = style_of(&name);
                        paired.insert(open_idx, (style, true));
                        paired.insert(idx, (style, false));
                    }
                } else {
                    // Crossed tags: the open ones can no longer close in
                    // order, so they stay unknown HTML.
                    open.clear();
                }
            }
            _ => {}
        }
    }
    paired
}

/// The four faces a view draws runs with, built once per base font so the
/// text caches (keyed on the `Arc`) keep hitting from frame to frame.
pub(super) struct StyledFonts {
    regular: Arc<Font>,
    bold_face: Option<Arc<Font>>,
    /// Indexed by `bold << 1 | italic`.
    fonts: [Arc<Font>; 4],
}

impl StyledFonts {
    fn new(regular: Arc<Font>, bold_face: Option<Arc<Font>>) -> Self {
        let italic = Arc::new(regular.variant_with_faux_style(true, false));
        let (bold, bold_italic) = match &bold_face {
            Some(face) => (
                Arc::clone(face),
                Arc::new(face.variant_with_faux_style(true, false)),
            ),
            None => (
                Arc::new(regular.variant_with_faux_style(false, true)),
                Arc::new(regular.variant_with_faux_style(true, true)),
            ),
        };
        Self {
            fonts: [Arc::clone(&regular), italic, bold, bold_italic],
            regular,
            bold_face,
        }
    }

    fn matches(&self, regular: &Arc<Font>, bold_face: &Option<Arc<Font>>) -> bool {
        Arc::ptr_eq(&self.regular, regular)
            && match (&self.bold_face, bold_face) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

/// The lazily built [`StyledFonts`] of a view.
pub(super) type StyledFontCache = RefCell<Option<StyledFonts>>;

impl MarkdownView {
    /// Draw bold runs with `font` (C# `AggContext.DefaultFontBold`). Without
    /// one, bold runs draw with the regular face thickened.
    pub fn with_bold_font(mut self, font: Arc<Font>) -> Self {
        self.bold_font = Some(font);
        self
    }

    /// The face a run of `style` measures and draws with.
    /// The face a run of text draws in: an inline code span in the code
    /// font when the view has one (C#'s copied HTML sets `<code>` in the
    /// monospace family; `with_code_font`), everything else in its
    /// emphasis face ([`Self::run_font`]).
    pub(super) fn run_face(&self, code: bool, style: MarkdownRunStyle) -> Arc<Font> {
        match (&self.code_font, code) {
            (Some(font), true) => Arc::clone(font),
            _ => self.run_font(style),
        }
    }

    pub(super) fn run_font(&self, style: MarkdownRunStyle) -> Arc<Font> {
        let regular = self.active_font();
        let mut cache = self.styled_fonts.borrow_mut();
        if !cache
            .as_ref()
            .is_some_and(|fonts| fonts.matches(&regular, &self.bold_font))
        {
            *cache = Some(StyledFonts::new(regular, self.bold_font.clone()));
        }
        let idx = (usize::from(style.bold) << 1) | usize::from(style.italic);
        match cache.as_ref() {
            Some(fonts) => Arc::clone(&fonts.fonts[idx]),
            None => self.active_font(),
        }
    }

    /// Every laid-out word with the emphasis it is drawn with, in reading
    /// order: paragraph, heading and list rows, then table cells where they
    /// appear (code blocks are never styled and are left out). The C# tests
    /// read the same from each word's `TextWidget` (`Bold`, `Italic`,
    /// `StrikeThrough`).
    pub fn laid_out_words(&self) -> Vec<(String, MarkdownRunStyle)> {
        let mut out = Vec::new();
        for item in &self.items {
            match item {
                super::LayoutItem::Line { runs, .. } => {
                    for run in runs {
                        if let super::LineRun::Text { text, style, .. } = run {
                            out.extend(text.split_whitespace().map(|w| (w.to_string(), *style)));
                        }
                    }
                }
                super::LayoutItem::Table {
                    rows, cell_styles, ..
                } => {
                    for (ri, row) in rows.iter().enumerate() {
                        for (ci, text) in row.iter().enumerate() {
                            let spans = cell_spans(cell_styles, ri, ci);
                            for (offset, word) in super::matching::words_with_offsets(text) {
                                out.push((word.to_string(), style_at(spans, offset)));
                            }
                        }
                    }
                }
                super::LayoutItem::CodeBlock { .. } => {}
            }
        }
        out
    }
}

/// A styled or linked byte range of a table cell's text.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CellSpan {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) style: MarkdownRunStyle,
    /// The url the range links to (C#'s `TextLinkX` inside an `AggTableCell`).
    pub(super) link: Option<String>,
}

/// Per table row, per cell, the cell's styled spans.
pub(super) type CellStyles = Vec<Vec<Vec<CellSpan>>>;

pub(super) fn cell_spans(styles: &CellStyles, row: usize, col: usize) -> &[CellSpan] {
    styles
        .get(row)
        .and_then(|r| r.get(col))
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// The style of the cell text at byte `offset`.
pub(super) fn style_at(spans: &[CellSpan], offset: usize) -> MarkdownRunStyle {
    spans
        .iter()
        .find(|span| span.start <= offset && offset < span.end)
        .map(|span| span.style)
        .unwrap_or_default()
}

/// The url the cell text at byte `offset` links to.
pub(super) fn link_at(spans: &[CellSpan], offset: usize) -> Option<&str> {
    spans
        .iter()
        .find(|span| span.start <= offset && offset < span.end)
        .and_then(|span| span.link.as_deref())
}

/// A cell's text cut into consecutive `(byte range, style)` pieces that
/// cover it from start to end.
pub(super) fn cell_pieces(
    text: &str,
    spans: &[CellSpan],
) -> Vec<(std::ops::Range<usize>, MarkdownRunStyle)> {
    let mut out = Vec::new();
    let mut pos = 0;
    for span in spans {
        let (start, end) = (span.start.min(text.len()), span.end.min(text.len()));
        if start > pos {
            out.push((pos..start, MarkdownRunStyle::default()));
        }
        if end > start.max(pos) {
            out.push((start.max(pos)..end, span.style));
            pos = end;
        }
    }
    if pos < text.len() {
        out.push((pos..text.len(), MarkdownRunStyle::default()));
    }
    out
}
