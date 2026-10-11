//! Selection, hit-testing, highlighting, and copy for `MarkdownView`.
//!
//! The renderer lays out Markdown into visual fragments. This module builds a
//! parallel selectable model from those fragments so hit-testing, highlighting,
//! and clipboard output all agree with the painted geometry.  A table cell
//! with images becomes text fragments between image fragments
//! (`cell_images.rs`).
//!
//! Copy follows MarkdigAgg's `MarkdownWidget.GetSelectionClipboardData`: the
//! plain text is the selected words as laid out, and the HTML is the
//! selection's markup (`rich_html.rs`) styled by `StyleHtmlFragment`
//! (`styled_html.rs`), or the whole document's `ToStyledHtml` when
//! everything is selected.

use std::ops::Range;
use std::sync::Arc;

use crate::clipboard;
use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect};
use crate::text::{measure_advance, Font};

use super::cell_images::{images_of, LaidCellImage};
use super::styled_html::{style_html_fragment, to_styled_html};
use super::{
    emphasis, LayoutItem, LineRun, LineStyle, MarkdownRunStyle, MarkdownView, BLOCK_SCROLLBAR_GAP,
    BLOCK_SCROLLBAR_H,
};

/// Where a table cell's content is laid out, for its fragments.
struct TableCellPlace<'a> {
    text: &'a str,
    spans: &'a [emphasis::CellSpan],
    images: &'a [LaidCellImage],
    /// The cell's content start (inside its inset).
    x: f64,
    y: f64,
    height: f64,
    clip: Rect,
}

#[derive(Clone)]
pub(super) struct SelectableFragment {
    pub range: Range<usize>,
    pub(super) text: String,
    pub(super) kind: SelectableKind,
    pub(super) text_x: f64,
    pub(super) y: f64,
    pub(super) height: f64,
    pub(super) width: f64,
    pub(super) font_size: f64,
    /// The emphasis a text run draws with, so selection measures in its face.
    pub(super) run_style: MarkdownRunStyle,
    pub(super) clip: Option<Rect>,
    /// Where the fragment sits in the document's blocks.
    pub(super) place: FragmentPlace,
}

/// The block a fragment belongs to, for building its copied HTML.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct FragmentPlace {
    /// Counts the document's blocks (paragraph, heading, list item, code
    /// block, table) in order: fragments of one block share it.
    pub(super) block: usize,
    /// The block is quoted.
    pub(super) quote: bool,
    /// For a table cell's fragment: the table's index in the layout items.
    pub(super) table: Option<usize>,
}

#[derive(Clone)]
pub(super) enum SelectableKind {
    Text {
        style: LineStyle,
        link: Option<String>,
        code: bool,
    },
    CodeBlock,
    TableCell,
    /// A horizontal rule: no text, it only marks where the rule sits so a
    /// selection across it copies `<hr />`.
    Rule,
    Image {
        url: String,
        alt: String,
        cache_idx: usize,
    },
}

impl MarkdownView {
    pub(super) fn rebuild_selection_model(&mut self) {
        self.selectable_text.clear();
        self.selectable_fragments.clear();

        let pad = self.padding;
        let font = self.active_font();
        let items = self.items.clone();
        let mut place = FragmentPlace::default();
        for (item_idx, item) in items.iter().enumerate() {
            match item {
                LayoutItem::Line {
                    runs,
                    style,
                    indent,
                    quote,
                    continues,
                    y,
                    height,
                    ..
                } => {
                    if matches!(style, LineStyle::Rule) {
                        self.push_block_gap();
                        place = FragmentPlace {
                            block: place.block + 1,
                            quote: *quote,
                            table: None,
                        };
                        let at = self.selectable_text.len();
                        self.selectable_fragments.push(SelectableFragment {
                            range: at..at,
                            text: String::new(),
                            kind: SelectableKind::Rule,
                            text_x: pad + indent,
                            y: *y,
                            height: 0.0,
                            width: 0.0,
                            font_size: self.font_size,
                            run_style: MarkdownRunStyle::default(),
                            clip: None,
                            place,
                        });
                        continue;
                    }
                    if runs.is_empty() {
                        self.push_selection_gap("\n");
                        continue;
                    }
                    if !*continues {
                        place = FragmentPlace {
                            block: place.block + 1,
                            quote: *quote,
                            table: None,
                        };
                    }
                    // A soft-wrapped row joins its paragraph's previous row
                    // with the space the wrap replaced; a new paragraph (a
                    // list item, a heading right after text) starts a line.
                    if *continues {
                        self.push_word_gap();
                    } else {
                        self.push_block_gap();
                    }

                    let fs = style.font_size(self.font_size);
                    let metrics = crate::text::measure_text_metrics(&font, "", fs);
                    let ty = y + height * 0.5;
                    let text_y = ty - (metrics.ascent - metrics.descent) * 0.5;
                    let tx = pad + indent;

                    for run in runs {
                        match run {
                            LineRun::Text {
                                text,
                                link,
                                code,
                                lead,
                                x,
                                width,
                                style: run_style,
                                ..
                            } => {
                                let text_x = if matches!(style, LineStyle::Code) {
                                    tx + x + 4.0
                                } else if *code {
                                    tx + x + self.font_size * 0.35
                                } else {
                                    tx + x + lead
                                };
                                self.push_fragment(SelectableFragment {
                                    range: 0..0,
                                    text: text.clone(),
                                    kind: SelectableKind::Text {
                                        style: *style,
                                        link: link.clone(),
                                        code: *code,
                                    },
                                    text_x,
                                    y: *y,
                                    height: *height,
                                    width: *width,
                                    font_size: fs,
                                    run_style: *run_style,
                                    clip: None,
                                    place,
                                });
                                let _ = text_y;
                            }
                            LineRun::Image {
                                url,
                                alt,
                                cache_idx,
                                x,
                                y_offset,
                                width,
                                height: image_h,
                                ..
                            } => {
                                let label = if alt.is_empty() { "image" } else { alt };
                                self.push_fragment(SelectableFragment {
                                    range: 0..0,
                                    text: label.to_string(),
                                    kind: SelectableKind::Image {
                                        url: url.clone(),
                                        alt: alt.clone(),
                                        cache_idx: *cache_idx,
                                    },
                                    text_x: tx + x,
                                    y: y + y_offset,
                                    height: *image_h,
                                    width: *width,
                                    font_size: self.font_size,
                                    run_style: MarkdownRunStyle::default(),
                                    clip: None,
                                    place,
                                });
                            }
                        }
                    }
                }
                LayoutItem::CodeBlock {
                    block_idx,
                    lines,
                    y,
                    height,
                    line_h,
                    viewport_width,
                    content_width,
                } => {
                    self.push_block_gap();
                    place = FragmentPlace {
                        block: place.block + 1,
                        quote: false,
                        table: None,
                    };
                    let scroll_x = self.block_scroll_offset(*block_idx);
                    let scrollbar_h = if content_width > viewport_width {
                        BLOCK_SCROLLBAR_H + BLOCK_SCROLLBAR_GAP
                    } else {
                        0.0
                    };
                    let code_pad_x = self.font_size;
                    let code_pad_y = self.font_size * 0.75;
                    let content_y = y + scrollbar_h;
                    let content_h = height - scrollbar_h;
                    let mut line_top = content_y + content_h - code_pad_y - line_h;
                    for (idx, line) in lines.iter().enumerate() {
                        if idx > 0 {
                            self.push_selection_gap("\n");
                        }
                        self.push_fragment(SelectableFragment {
                            range: 0..0,
                            text: line.clone(),
                            kind: SelectableKind::CodeBlock,
                            text_x: pad + code_pad_x - scroll_x,
                            y: line_top,
                            height: *line_h,
                            width: measure_advance(
                                &font,
                                line,
                                LineStyle::Code.font_size(self.font_size),
                            ),
                            font_size: LineStyle::Code.font_size(self.font_size),
                            run_style: MarkdownRunStyle::default(),
                            clip: Some(Rect::new(pad, content_y, *viewport_width, content_h)),
                            place,
                        });
                        line_top -= line_h;
                    }
                }
                LayoutItem::Table {
                    block_idx,
                    rows,
                    cell_styles,
                    cell_images,
                    y,
                    row_heights,
                    col_widths,
                    viewport_width,
                    content_width,
                    ..
                } => {
                    self.push_block_gap();
                    place = FragmentPlace {
                        block: place.block + 1,
                        quote: false,
                        table: Some(item_idx),
                    };
                    let scroll_x = self.block_scroll_offset(*block_idx);
                    let scrollbar_h = if content_width > viewport_width {
                        BLOCK_SCROLLBAR_H + BLOCK_SCROLLBAR_GAP
                    } else {
                        0.0
                    };
                    let content_y = y + scrollbar_h;
                    let table_h: f64 = row_heights.iter().sum();
                    let clip = Rect::new(pad, content_y, *viewport_width, table_h);
                    let mut cy = content_y + table_h;
                    for (ri, row) in rows.iter().enumerate() {
                        if ri > 0 {
                            self.push_selection_gap("\n");
                        }
                        let row_h = row_heights.get(ri).copied().unwrap_or_default();
                        cy -= row_h;
                        let mut cx = pad - scroll_x;
                        for (ci, width) in col_widths.iter().enumerate() {
                            if ci > 0 {
                                self.push_selection_gap("\t");
                            }
                            if let Some(text) = row.get(ci) {
                                let cell = TableCellPlace {
                                    text,
                                    spans: emphasis::cell_spans(cell_styles, ri, ci),
                                    images: images_of(cell_images, ri, ci),
                                    x: cx + 8.0,
                                    y: cy,
                                    height: row_h,
                                    clip,
                                };
                                self.push_table_cell(&cell, place);
                            }
                            cx += width;
                        }
                    }
                }
            }
        }

        self.clamp_selection_to_text();
    }

    /// A table cell's selectable fragments: its text between its images
    /// and the images, separated by a space where the text has none.
    fn push_table_cell(&mut self, cell: &TableCellPlace<'_>, place: FragmentPlace) {
        let mut pieces: Vec<(usize, Option<usize>)> = Vec::new();
        for range in MarkdownView::cell_text_pieces(cell.text, cell.images) {
            pieces.push((range.start, None));
        }
        for (k, image) in cell.images.iter().enumerate() {
            pieces.push((image.image.at, Some(k)));
        }
        // An image sits before the text at its offset.
        pieces.sort_by_key(|(at, image)| (*at, image.is_none()));
        let text_pieces = MarkdownView::cell_text_pieces(cell.text, cell.images);
        let mut first = true;
        for (at, image) in pieces {
            if !first {
                self.push_word_gap();
            }
            first = false;
            match image {
                Some(k) => {
                    let image = &cell.images[k];
                    let alt = &image.image.alt;
                    let label = if alt.is_empty() { "image" } else { alt };
                    self.push_fragment(SelectableFragment {
                        range: 0..0,
                        text: label.to_string(),
                        kind: SelectableKind::Image {
                            url: image.image.url.clone(),
                            alt: alt.clone(),
                            cache_idx: image.cache_idx,
                        },
                        text_x: cell.x + self.cell_image_x(cell.text, cell.spans, cell.images, k),
                        y: cell.y + (cell.height - image.height) * 0.5,
                        height: image.height,
                        width: image.width,
                        font_size: self.font_size,
                        run_style: MarkdownRunStyle::default(),
                        clip: Some(cell.clip),
                        place,
                    });
                }
                None => {
                    let Some(range) = text_pieces.iter().find(|r| r.start == at) else {
                        continue;
                    };
                    let text = &cell.text[range.clone()];
                    self.push_fragment(SelectableFragment {
                        range: 0..0,
                        text: text.to_string(),
                        kind: SelectableKind::TableCell,
                        text_x: cell.x
                            + self.cell_text_x(cell.text, cell.spans, cell.images, range.start),
                        y: cell.y,
                        height: cell.height,
                        width: measure_advance(&self.active_font(), text, self.font_size),
                        font_size: self.font_size,
                        run_style: MarkdownRunStyle::default(),
                        clip: Some(cell.clip),
                        place,
                    });
                }
            }
        }
    }

    /// Separate a soft-wrapped row from the row before it with one space,
    /// unless the text already ends in whitespace.
    fn push_word_gap(&mut self) {
        if !self.selectable_text.is_empty() && !self.selectable_text.ends_with(char::is_whitespace)
        {
            self.selectable_text.push(' ');
        }
    }

    /// Start a new block (paragraph, table, code block) on its own line.
    fn push_block_gap(&mut self) {
        if !self.selectable_text.is_empty() {
            self.push_selection_gap("\n");
        }
    }

    fn push_selection_gap(&mut self, gap: &str) {
        if !self.selectable_text.ends_with(gap) {
            self.selectable_text.push_str(gap);
        }
    }

    fn push_fragment(&mut self, mut fragment: SelectableFragment) {
        if fragment.text.is_empty() {
            return;
        }
        let start = self.selectable_text.len();
        self.selectable_text.push_str(&fragment.text);
        fragment.range = start..self.selectable_text.len();
        self.selectable_fragments.push(fragment);
    }

    /// Keep the selection inside the rebuilt text, on character boundaries:
    /// after [`set_markdown`](MarkdownView::set_markdown) the old byte offsets
    /// may sit past the end or inside a character that changed, and slicing
    /// there (copy, highlight) would panic.
    pub(super) fn clamp_selection_to_text(&mut self) {
        let text = &self.selectable_text;
        let clamp = |pos: usize| {
            let mut pos = pos.min(text.len());
            while !text.is_char_boundary(pos) {
                pos -= 1;
            }
            pos
        };
        self.selection_anchor = self.selection_anchor.map(clamp);
        self.selection_cursor = self.selection_cursor.map(clamp);
    }

    /// The laid-out document as plain text (what select-all copies): the
    /// rendered words without their markdown syntax.
    pub fn laid_out_text(&self) -> &str {
        &self.selectable_text
    }

    /// Whether any text is selected (a drag across the words, a double-click
    /// or Select All).
    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    pub(super) fn text_pos_at(&self, pos: Point) -> Option<usize> {
        let mut best: Option<(f64, usize)> = None;
        for fragment in &self.selectable_fragments {
            if matches!(fragment.kind, SelectableKind::Rule) {
                continue;
            }
            if pos.y < fragment.y || pos.y > fragment.y + fragment.height {
                continue;
            }
            if let Some(clip) = fragment.clip {
                if pos.x < clip.x
                    || pos.x > clip.x + clip.width
                    || pos.y < clip.y
                    || pos.y > clip.y + clip.height
                {
                    continue;
                }
            }

            let x0 = fragment.text_x;
            let x1 = fragment.text_x + fragment.width;
            let (distance, offset) = if pos.x <= x0 {
                (x0 - pos.x, fragment.range.start)
            } else if pos.x >= x1 {
                (pos.x - x1, fragment.range.end)
            } else if matches!(fragment.kind, SelectableKind::Image { .. }) {
                let mid = x0 + fragment.width * 0.5;
                if pos.x < mid {
                    (0.0, fragment.range.start)
                } else {
                    (0.0, fragment.range.end)
                }
            } else {
                (
                    0.0,
                    fragment.range.start
                        + byte_at_x(
                            &self.run_font(fragment.run_style),
                            &fragment.text,
                            fragment.font_size,
                            pos.x - x0,
                        ),
                )
            };

            if best.map_or(true, |(best_distance, _)| distance < best_distance) {
                best = Some((distance, offset));
            }
        }
        best.map(|(_, pos)| pos)
    }

    pub(super) fn selection_range(&self) -> Option<Range<usize>> {
        let anchor = self.selection_anchor?;
        let cursor = self.selection_cursor?;
        if anchor == cursor {
            None
        } else {
            Some(anchor.min(cursor)..anchor.max(cursor))
        }
    }

    pub(super) fn clear_selection(&mut self) {
        self.selection_anchor = None;
        self.selection_cursor = None;
    }

    pub(super) fn select_all_text(&mut self) {
        self.selection_anchor = Some(0);
        self.selection_cursor = Some(self.selectable_text.len());
    }

    pub(super) fn copy_selection(&self) {
        self.try_copy_selection_to_clipboard();
    }

    /// Copy the selection (MarkdigAgg's `TryCopySelectionToClipboard`): its
    /// plain text, with its styled HTML when there is any.  `false` when
    /// nothing is selected.
    pub fn try_copy_selection_to_clipboard(&self) -> bool {
        let Some(range) = self.selection_range() else {
            return false;
        };
        let (plain, html) = self.copy_payloads(range);
        if plain.is_empty() {
            return false;
        }
        if html.trim().is_empty() {
            clipboard::set_text(&plain);
        } else {
            clipboard::set_rich_text(&plain, &html);
        }
        true
    }

    /// The plain text and styled HTML a copy of `range` writes (MarkdigAgg's
    /// `GetSelectionClipboardData`): the selected words, and their markup
    /// styled with the view's HTML theme, or the whole document as styled
    /// HTML when the selection covers all of it.
    pub(super) fn copy_payloads(&self, range: Range<usize>) -> (String, String) {
        let plain = self
            .selectable_text
            .get(range.clone())
            .unwrap_or_default()
            .to_string();
        let html = if range.start == 0
            && range.end == self.selectable_text.len()
            && !self.markdown.trim().is_empty()
        {
            to_styled_html(&self.markdown, &self.html_theme)
        } else {
            style_html_fragment(
                &super::rich_html::selection_html(self, range),
                &self.html_theme,
            )
        };
        (plain, html)
    }

    pub(super) fn paint_selection(&self, ctx: &mut dyn DrawCtx) {
        let Some(range) = self.selection_range() else {
            return;
        };
        let color = if self.focused {
            ctx.visuals().selection_bg
        } else {
            ctx.visuals().selection_bg_unfocused
        };
        ctx.set_fill_color(color);
        ctx.set_font(Arc::clone(&self.active_font()));

        for fragment in &self.selectable_fragments {
            if fragment.range.end <= range.start || fragment.range.start >= range.end {
                continue;
            }

            let start = range.start.max(fragment.range.start) - fragment.range.start;
            let end = range.end.min(fragment.range.end) - fragment.range.start;
            let (x, w) = if matches!(fragment.kind, SelectableKind::Image { .. }) {
                (fragment.text_x, fragment.width)
            } else {
                ctx.set_font_size(fragment.font_size);
                let before = &fragment.text[..start];
                let selected = &fragment.text[start..end];
                let face = self.run_font(fragment.run_style);
                (
                    fragment.text_x + measure_advance(&face, before, fragment.font_size),
                    measure_advance(&face, selected, fragment.font_size),
                )
            };

            if w <= 0.0 {
                continue;
            }

            if let Some(clip) = fragment.clip {
                ctx.save();
                ctx.clip_rect(clip.x, clip.y, clip.width, clip.height);
                fill_highlight(ctx, x, fragment.y, w, fragment.height);
                ctx.restore();
            } else {
                fill_highlight(ctx, x, fragment.y, w, fragment.height);
            }
        }
    }

    pub(super) fn hit_image(&self, pos: Point) -> Option<(String, String, usize)> {
        for fragment in &self.selectable_fragments {
            let SelectableKind::Image {
                url,
                alt,
                cache_idx,
            } = &fragment.kind
            else {
                continue;
            };
            if fragment.clip.is_some_and(|clip| !clip.contains(pos)) {
                continue;
            }
            if pos.x >= fragment.text_x
                && pos.x <= fragment.text_x + fragment.width
                && pos.y >= fragment.y
                && pos.y <= fragment.y + fragment.height
            {
                return Some((url.clone(), alt.clone(), *cache_idx));
            }
        }
        None
    }
}

fn byte_at_x(font: &Arc<Font>, text: &str, font_size: f64, x: f64) -> usize {
    if x <= 0.0 {
        return 0;
    }
    let mut last = 0;
    for (idx, ch) in text.char_indices() {
        let next = idx + ch.len_utf8();
        let left = measure_advance(font, &text[..idx], font_size);
        let right = measure_advance(font, &text[..next], font_size);
        if x < (left + right) * 0.5 {
            return idx;
        }
        last = next;
    }
    last
}

fn fill_highlight(ctx: &mut dyn DrawCtx, x: f64, y: f64, w: f64, h: f64) {
    ctx.begin_path();
    ctx.rect(x, y + h * 0.12, w, h * 0.76);
    ctx.fill();
}

pub(super) fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(super) fn escape_attr(text: &str) -> String {
    escape_html(text).replace('"', "&quot;")
}
