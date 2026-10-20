//! Layout pass for `MarkdownView`.
//!
//! Converts parsed markdown flow items into positioned rows in the framework's
//! Y-up coordinate system, and records the widest text row for
//! [`MarkdownView::max_line_width`].

use crate::geometry::Size;
use crate::text::measure_text_metrics;

use super::alignment::{self, MarkdownAlign};
use super::cell_images;
use super::matching::{self, MATCH_PAD};
use super::table_style::{header_bold, CELL_INSET};
use super::{
    clamp_block_offset, emphasis, ImageState, InlineItem, LayoutItem, LineRun, LineStyle,
    MarkdownRunStyle, MarkdownView, BLOCK_SCROLLBAR_GAP, BLOCK_SCROLLBAR_H,
};

/// The style of a text run before it is positioned.
struct TextRun {
    text: String,
    link: Option<String>,
    code: bool,
    style: MarkdownRunStyle,
    highlight: bool,
    lead: f64,
    trail: f64,
}

impl MarkdownView {
    /// The widest text row of the last layout, from the row's left text edge
    /// (its indent included, the view's padding not) to the end of its last
    /// word or image: agg-sharp's `ParagraphX.MaxLineWidth`, the largest over
    /// the document. Paragraphs, headings and list items count; tables and
    /// code blocks, which scroll inside their own box, do not (C# measures
    /// only `ParagraphX`). A host shrinks a view to its text with it, as
    /// MatterCAD's tooltip lays out at 350 and re-lays at this width + 15.
    pub fn max_line_width(&self) -> f64 {
        self.max_line_w
    }

    /// Whether the gap every paragraph, heading and list leaves below itself is
    /// also kept after the document's last block (default `true`). A view
    /// boxed by its own padding, such as a tooltip, passes `false`, so the
    /// space under its last line equals the space over its first.
    pub fn with_end_spacing(mut self, keep: bool) -> Self {
        self.end_spacing = keep;
        self
    }

    /// The gap every paragraph, heading and list leaves below itself, in lines
    /// of the body font (default [`super::DEFAULT_PARAGRAPH_SPACING`]). A
    /// tooltip passes about a quarter, so its help line sits right under the
    /// name it explains.
    pub fn with_paragraph_spacing(mut self, lines: f64) -> Self {
        self.paragraph_spacing = lines;
        self
    }

    pub(super) fn layout_markdown(&mut self, available: Size) -> Size {
        let pad = self.padding;
        let viewport_w = crate::widget::current_viewport().width;
        let wrap_w = if available.width.is_finite() && available.width < viewport_w * 2.0 {
            available.width
        } else {
            viewport_w
        };
        let max_w = (wrap_w - pad * 2.0).max(1.0);

        let mut paragraphs = self.parse_paragraphs();
        if !self.end_spacing {
            // The gap a block leaves below itself separates it from the next
            // block; after the last one the view's padding is the only inset.
            while matches!(paragraphs.last(), Some(super::ParagraphItem::Spacer)) {
                paragraphs.pop();
            }
        }
        let mut laid_out = Vec::new();
        let mut block_idx = 0usize;
        // Widest text row (indent + its content), C#'s `ParagraphX.MaxLineWidth`.
        let mut max_line_w = 0.0_f64;

        for item in &paragraphs {
            match item {
                super::ParagraphItem::Rule => laid_out.push(LayoutItem::Line {
                    runs: Vec::new(),
                    style: LineStyle::Rule,
                    indent: 0.0,
                    quote: false,
                    continues: false,
                    align: MarkdownAlign::Left,
                    level: 0,
                    y: 0.0,
                    height: 8.0,
                }),
                super::ParagraphItem::Spacer => {
                    let metrics = measure_text_metrics(&self.active_font(), "", self.font_size);
                    laid_out.push(LayoutItem::Line {
                        runs: Vec::new(),
                        style: LineStyle::Body,
                        indent: 0.0,
                        quote: false,
                        continues: false,
                        align: MarkdownAlign::Left,
                        level: 0,
                        y: 0.0,
                        height: metrics.line_height * self.paragraph_spacing,
                    });
                }
                super::ParagraphItem::Table {
                    rows,
                    cell_styles,
                    cell_images,
                    header,
                    aligns,
                } => {
                    let cell_styles = &header_bold(rows, cell_styles, *header);
                    let cell_images = self.layout_cell_images(cell_images, max_w);
                    let (col_widths, row_heights) =
                        self.layout_table(rows, cell_styles, &cell_images);
                    let line = super::table_grid::line_for_scale(crate::device_scale());
                    let grid = super::table_grid::TableGrid {
                        col_widths: &col_widths,
                        row_heights: &row_heights,
                        line,
                    };
                    let mut height = grid.height();
                    let content_width = grid.width();
                    let viewport_width = max_w;
                    if content_width > viewport_width {
                        height += BLOCK_SCROLLBAR_H + BLOCK_SCROLLBAR_GAP;
                    }
                    let offset = clamp_block_offset(
                        self.block_scroll_offset(block_idx),
                        viewport_width,
                        content_width,
                    );
                    self.block_scroll_mut(block_idx).offset = offset;
                    laid_out.push(LayoutItem::Table {
                        block_idx,
                        rows: rows.clone(),
                        cell_styles: cell_styles.clone(),
                        cell_images,
                        header: *header,
                        aligns: aligns.clone(),
                        y: 0.0,
                        height,
                        row_heights,
                        col_widths,
                        line,
                        viewport_width,
                        content_width,
                    });
                    block_idx += 1;
                }
                super::ParagraphItem::CodeBlock(lines) => {
                    let (line_h, mut height, content_width) = self.layout_code_block(lines, max_w);
                    let viewport_width = max_w;
                    if content_width > viewport_width {
                        height += BLOCK_SCROLLBAR_H + BLOCK_SCROLLBAR_GAP;
                    }
                    let offset = clamp_block_offset(
                        self.block_scroll_offset(block_idx),
                        viewport_width,
                        content_width,
                    );
                    self.block_scroll_mut(block_idx).offset = offset;
                    laid_out.push(LayoutItem::CodeBlock {
                        block_idx,
                        lines: lines.clone(),
                        y: 0.0,
                        height,
                        line_h,
                        viewport_width,
                        content_width,
                    });
                    block_idx += 1;
                }
                super::ParagraphItem::Flow {
                    items,
                    style,
                    indent,
                    quote,
                    align,
                    level,
                } => {
                    let flow_start = laid_out.len();
                    let font_size = style.font_size(self.font_size);
                    let metrics = measure_text_metrics(&self.active_font(), "", font_size);
                    let line_h = metrics.line_height * 1.3;
                    let avail = (max_w - indent).max(1.0);
                    let mut runs = Vec::new();
                    let mut used = 0.0;
                    // Where the row's content ends: `used` minus an image's trailing gap.
                    let mut line_end = 0.0_f64;
                    let mut row_h = line_h;

                    for inline in items {
                        match inline {
                            InlineItem::Text {
                                text,
                                link,
                                code,
                                style: run_style,
                                glue,
                            } => {
                                // Markdig never inline-parses code, so it is never matched.
                                let ranges = match self.active_matching_text() {
                                    Some(needle) if !*code => matching::match_ranges(text, needle),
                                    _ => Vec::new(),
                                };
                                for (wi, (offset, word)) in
                                    matching::words_with_offsets(text).into_iter().enumerate()
                                {
                                    // A glued item's first word follows the
                                    // previous item with no space.
                                    let spaced = used > 0.0 && !(*glue && wi == 0);
                                    let mut segs =
                                        Self::word_segments(word, offset, &ranges, spaced);
                                    let w = self.segments_width(&segs, *style, *code, *run_style);
                                    if used > 0.0 && used + w > avail {
                                        max_line_w = max_line_w.max(indent + line_end);
                                        line_end = 0.0;
                                        Self::push_line(
                                            &mut laid_out,
                                            &mut runs,
                                            *style,
                                            *indent,
                                            *quote,
                                            row_h,
                                        );
                                        used = 0.0;
                                        row_h = line_h;
                                        segs = Self::word_segments(word, offset, &ranges, false);
                                    }
                                    for seg in segs {
                                        let sw =
                                            self.segment_width(&seg, *style, *code, *run_style);
                                        Self::push_text_run(
                                            &mut runs,
                                            TextRun {
                                                text: seg.text,
                                                link: link.clone(),
                                                code: *code,
                                                style: *run_style,
                                                highlight: seg.highlight,
                                                lead: if seg.starts_match {
                                                    MATCH_PAD
                                                } else {
                                                    0.0
                                                },
                                                trail: if seg.ends_match { MATCH_PAD } else { 0.0 },
                                            },
                                            used,
                                            sw,
                                        );
                                        used += sw;
                                        line_end = used;
                                    }
                                }
                            }
                            InlineItem::Image { url, alt, link } => {
                                let cache_idx = self.get_or_load_image(url);
                                let (iw, ih) = self.inline_image_size(cache_idx, alt, avail);
                                if used > 0.0 && used + iw > avail {
                                    max_line_w = max_line_w.max(indent + line_end);
                                    Self::push_line(
                                        &mut laid_out,
                                        &mut runs,
                                        *style,
                                        *indent,
                                        *quote,
                                        row_h,
                                    );
                                    used = 0.0;
                                    row_h = line_h;
                                }
                                runs.push(LineRun::Image {
                                    url: url.clone(),
                                    alt: alt.clone(),
                                    link: link.clone(),
                                    cache_idx,
                                    x: used,
                                    y_offset: (row_h - ih).max(0.0) * 0.5,
                                    width: iw,
                                    height: ih,
                                });
                                line_end = used + iw;
                                used += iw + 4.0;
                                row_h = row_h.max(ih);
                            }
                        }
                    }
                    if !runs.is_empty() {
                        max_line_w = max_line_w.max(indent + line_end);
                        Self::push_line(&mut laid_out, &mut runs, *style, *indent, *quote, row_h);
                    }
                    // Every row after a paragraph's first is a soft wrap of it.
                    for item in laid_out.iter_mut().skip(flow_start + 1) {
                        if let LayoutItem::Line { continues, .. } = item {
                            *continues = true;
                        }
                    }
                    for item in laid_out.iter_mut().skip(flow_start) {
                        if let LayoutItem::Line { level: l, .. } = item {
                            *l = *level;
                        }
                    }
                    alignment::align_rows(&mut laid_out[flow_start..], avail, *align);
                }
            }
        }

        let total_h: f64 = laid_out
            .iter()
            .map(|item| match item {
                LayoutItem::Line { height, .. } => *height,
                LayoutItem::Table { height, .. } => *height,
                LayoutItem::CodeBlock { height, .. } => *height,
            })
            .sum::<f64>()
            + pad * 2.0;
        let mut y = total_h - pad;

        self.items.clear();
        for mut item in laid_out {
            let item_h = match &item {
                LayoutItem::Line { height, .. } => *height,
                LayoutItem::Table { height, .. } => *height,
                LayoutItem::CodeBlock { height, .. } => *height,
            };
            y -= item_h;
            match &mut item {
                LayoutItem::Line { y: item_y, .. } => *item_y = y,
                LayoutItem::Table { y: item_y, .. } => *item_y = y,
                LayoutItem::CodeBlock { y: item_y, .. } => *item_y = y,
            }
            self.items.push(item);
        }

        self.content_h = total_h;
        self.max_line_w = max_line_w;
        self.bounds = crate::geometry::Rect::new(0.0, 0.0, wrap_w, total_h);
        self.rebuild_selection_model();
        Size::new(wrap_w, total_h)
    }

    fn text_width(&self, text: &str, style: LineStyle) -> f64 {
        let font_size = style.font_size(self.font_size);
        measure_text_metrics(&self.active_font(), text, font_size).width
    }

    /// Width of `text` in the face of `run_style` (a bold face measures
    /// wider; the italic shear keeps every advance).
    pub(super) fn styled_text_width(
        &self,
        text: &str,
        style: LineStyle,
        run_style: MarkdownRunStyle,
    ) -> f64 {
        let font_size = style.font_size(self.font_size);
        measure_text_metrics(&self.run_font(run_style), text, font_size).width
    }

    fn run_width(
        &self,
        text: &str,
        style: LineStyle,
        code: bool,
        run_style: MarkdownRunStyle,
    ) -> f64 {
        let font_size = style.font_size(self.font_size);
        let width = measure_text_metrics(&self.run_face(code, run_style), text, font_size).width;
        if code {
            width + self.font_size * 0.75
        } else {
            width
        }
    }

    pub(super) fn inline_image_size(&self, cache_idx: usize, alt: &str, max_w: f64) -> (f64, f64) {
        if let Some((iw, ih)) = self.image_cache.get(cache_idx).and_then(|entry| {
            entry.state.lock().ok().and_then(|state| match &*state {
                ImageState::Ready { image, .. } => Some((image.width, image.height)),
                _ => None,
            })
        }) {
            let scale = (max_w / iw as f64).min(1.0);
            (iw as f64 * scale, ih as f64 * scale)
        } else {
            let label = if alt.is_empty() { "image" } else { alt };
            let w = self.text_width(label, LineStyle::Body) + 16.0 + self.placeholder_icon_room();
            (w.min(max_w), self.font_size * 1.45)
        }
    }

    /// Width of one word segment: its text plus code or match-box padding.
    fn segment_width(
        &self,
        seg: &matching::Segment,
        style: LineStyle,
        code: bool,
        run_style: MarkdownRunStyle,
    ) -> f64 {
        let mut w = self.run_width(&seg.text, style, code, run_style);
        if seg.starts_match {
            w += MATCH_PAD;
        }
        if seg.ends_match {
            w += MATCH_PAD;
        }
        w
    }

    fn segments_width(
        &self,
        segs: &[matching::Segment],
        style: LineStyle,
        code: bool,
        run_style: MarkdownRunStyle,
    ) -> f64 {
        segs.iter()
            .map(|seg| self.segment_width(seg, style, code, run_style))
            .sum()
    }

    /// Append a run, merging it into the previous one when they look the same.
    /// A run that starts a match box never merges into the box before it, and
    /// nothing merges after a box's end, so each box keeps its own padding.
    fn push_text_run(runs: &mut Vec<LineRun>, run: TextRun, x: f64, width: f64) {
        if let Some(LineRun::Text {
            text: last,
            width: last_w,
            link: last_link,
            code: last_code,
            style: last_style,
            highlight: last_highlight,
            trail: last_trail,
            ..
        }) = runs.last_mut()
        {
            let box_edge = run.highlight && (run.lead > 0.0 || *last_trail > 0.0);
            if *last_link == run.link
                && *last_code == run.code
                && *last_style == run.style
                && *last_highlight == run.highlight
                && !box_edge
            {
                last.push_str(&run.text);
                *last_w += width;
                *last_trail = run.trail;
                return;
            }
        }
        runs.push(LineRun::Text {
            text: run.text,
            link: run.link,
            code: run.code,
            style: run.style,
            highlight: run.highlight,
            lead: run.lead,
            trail: run.trail,
            x,
            width,
        });
    }

    /// The height of a table row of text.
    pub(super) fn table_text_row_height(&self) -> f64 {
        measure_text_metrics(&self.active_font(), "", self.font_size).line_height * 1.6
    }

    /// Each column's width (its widest cell's text and images, plus the
    /// cell inset) and each row's height (a row of text, or its tallest
    /// image with padding above and below).
    fn layout_table(
        &self,
        rows: &[Vec<String>],
        cell_styles: &emphasis::CellStyles,
        cell_images: &cell_images::LaidCellImages,
    ) -> (Vec<f64>, Vec<f64>) {
        let text_row_h = self.table_text_row_height();
        let scale = crate::device_scale();
        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut widths = vec![0.0_f64; cols];
        let mut heights = Vec::with_capacity(rows.len());
        for (ri, row) in rows.iter().enumerate() {
            let mut row_h = text_row_h;
            for (col, text) in row.iter().enumerate() {
                let spans = emphasis::cell_spans(cell_styles, ri, col);
                let images = cell_images::images_of(cell_images, ri, col);
                let content_w = self.cell_content_width(text, spans, images);
                widths[col] = widths[col].max(content_w + CELL_INSET * 2.0);
                for image in images {
                    row_h = row_h.max(image.height + cell_images::CELL_IMAGE_PAD_Y * 2.0);
                }
            }
            // Whole device pixels, as C# `Math.Ceiling` sized rows and
            // columns, so the snapped grid keeps every cell its width.
            heights.push(super::table_grid::ceil_to_device_pixels(row_h, scale));
        }
        for width in &mut widths {
            *width = super::table_grid::ceil_to_device_pixels(*width, scale);
        }
        (widths, heights)
    }

    /// Width of a table cell's text up to byte `end`, each styled piece in
    /// its own face.
    pub(super) fn cell_prefix_width(
        &self,
        text: &str,
        spans: &[emphasis::CellSpan],
        end: usize,
    ) -> f64 {
        emphasis::cell_pieces(text, spans)
            .into_iter()
            .filter(|(range, _)| range.start < end)
            .map(|(range, style)| {
                let piece = &text[range.start..range.end.min(end)];
                self.styled_text_width(piece, LineStyle::Body, style)
            })
            .sum()
    }

    fn layout_code_block(&self, lines: &[String], max_w: f64) -> (f64, f64, f64) {
        let font_size = LineStyle::Code.font_size(self.font_size);
        let code_font = self.code_font();
        let metrics = measure_text_metrics(&code_font, "", font_size);
        let line_h = metrics.line_height * 1.35;
        let max_line_w = lines
            .iter()
            .map(|line| measure_text_metrics(&code_font, line, font_size).width)
            .fold(0.0_f64, f64::max);
        let pad_x = self.font_size;
        let pad_y = self.font_size * 0.75;
        let width = (max_line_w + pad_x * 2.0).max(max_w);
        let height = line_h * lines.len().max(1) as f64 + pad_y * 2.0;
        (line_h, height, width)
    }

    fn push_line(
        items: &mut Vec<LayoutItem>,
        runs: &mut Vec<LineRun>,
        style: LineStyle,
        indent: f64,
        quote: bool,
        height: f64,
    ) {
        for run in runs.iter_mut() {
            if let LineRun::Image {
                url: _,
                y_offset,
                height: image_h,
                ..
            } = run
            {
                *y_offset = (height - *image_h).max(0.0) * 0.5;
            }
        }
        items.push(LayoutItem::Line {
            runs: std::mem::take(runs),
            style,
            indent,
            quote,
            continues: false,
            align: MarkdownAlign::Left,
            level: 0,
            y: 0.0,
            height,
        });
    }
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
