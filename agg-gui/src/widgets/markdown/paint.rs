//! Painting for `MarkdownView`.
//!
//! All drawing in this module uses the already laid-out Y-up row positions
//! produced by `layout.rs`. Each text run draws in the face of its emphasis
//! (`MarkdownView::run_font`, `emphasis.rs`), struck through when it is.
//! Images, in a row or a table cell, draw through `cell_images.rs`.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::text::measure_text_metrics;

use super::cell_images::{self, LaidCellImage};
use super::table_style;
use super::table_style::cell_content_x;
use super::{
    emphasis, matching, scrollbar_rect, scrollbar_thumb, LayoutItem, LineRun, LineStyle,
    MarkdownView, BLOCK_SCROLLBAR_GAP, BLOCK_SCROLLBAR_H,
};

/// A table cell's text, emphasis spans and images.
type CellContent<'a> = (&'a str, &'a [emphasis::CellSpan], &'a [LaidCellImage]);

impl MarkdownView {
    pub(super) fn paint_markdown(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let colors = self.resolved_colors(&v);
        let pad = self.padding;
        let w = self.bounds.width;
        let font = self.active_font();
        ctx.set_font(Arc::clone(&font));
        if let Some(background) = colors.background {
            ctx.set_fill_color(background);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, w, self.bounds.height);
            ctx.fill();
        }
        self.paint_selection(ctx);

        for item in &self.items {
            match item {
                LayoutItem::Line {
                    runs,
                    style,
                    indent,
                    quote,
                    y,
                    height,
                    ..
                } => {
                    let fs = style.font_size(self.font_size);
                    ctx.set_font_size(fs);

                    let tx = pad + indent;
                    let ty = y + height * 0.5;
                    let metrics = measure_text_metrics(&font, "", fs);
                    let text_y = ty - (metrics.ascent - metrics.descent) * 0.5;
                    if *quote {
                        ctx.set_fill_color(colors.rule);
                        ctx.begin_path();
                        ctx.rect(pad + indent - 12.0, *y, 3.0, *height);
                        ctx.fill();
                    }

                    match style {
                        LineStyle::Rule => {
                            ctx.set_fill_color(colors.rule);
                            ctx.begin_path();
                            ctx.rect(pad, ty, w - pad * 2.0, 1.0);
                            ctx.fill();
                        }
                        LineStyle::Code => {
                            ctx.set_fill_color(
                                colors
                                    .code_background
                                    .unwrap_or(Color::rgba(0.0, 0.0, 0.0, 0.15)),
                            );
                            ctx.begin_path();
                            ctx.rounded_rect(pad, *y, w - pad * 2.0, *height, 3.0);
                            ctx.fill();
                            // The visuals' look draws these rows in the accent; a view
                            // with its own colours draws them in its text colour.
                            ctx.set_fill_color(if colors.code_background.is_some() {
                                colors.text
                            } else {
                                v.accent
                            });
                            for run in runs {
                                if let LineRun::Text { text, x, .. } = run {
                                    ctx.fill_text(text, tx + x + 4.0, text_y);
                                }
                            }
                        }
                        _ => {
                            ctx.set_fill_color(colors.text);
                            for run in runs {
                                match run {
                                    LineRun::Text {
                                        text,
                                        link,
                                        code,
                                        style: run_style,
                                        highlight,
                                        lead,
                                        trail,
                                        x,
                                        width,
                                    } => {
                                        if *highlight {
                                            ctx.set_fill_color(matching::match_fill(v.accent));
                                            ctx.begin_path();
                                            ctx.rect(tx + x, *y, *width, *height);
                                            ctx.fill();
                                        }
                                        if *code {
                                            ctx.set_fill_color(
                                                colors
                                                    .code_background
                                                    .unwrap_or(Color::rgba(0.5, 0.5, 0.5, 0.14)),
                                            );
                                            ctx.begin_path();
                                            ctx.rounded_rect(
                                                tx + x,
                                                *y + height * 0.16,
                                                *width,
                                                height * 0.68,
                                                3.0,
                                            );
                                            ctx.fill();
                                        }
                                        ctx.set_fill_color(if link.is_some() {
                                            colors.link
                                        } else {
                                            colors.text
                                        });
                                        let text_x = if *code {
                                            tx + x + self.font_size * 0.35
                                        } else {
                                            tx + x + lead
                                        };
                                        let face = self.run_face(*code, *run_style);
                                        ctx.set_font(Arc::clone(&face));
                                        ctx.fill_text(text, text_x, text_y);
                                        if run_style.strikethrough {
                                            // Past the run's lead space, as C# strikes
                                            // each word's own text.
                                            let space = text.len() - text.trim_start().len();
                                            let skip =
                                                measure_text_metrics(&face, &text[..space], fs)
                                                    .width;
                                            let pad = if *code { 0.0 } else { *lead + *trail };
                                            self.paint_strike(
                                                ctx,
                                                fs,
                                                text_x + skip,
                                                text_y,
                                                (*width - pad - skip).max(0.0),
                                            );
                                        }
                                        ctx.set_font(Arc::clone(&font));
                                        if link.is_some() {
                                            ctx.begin_path();
                                            ctx.rect(tx + x, text_y - 2.0, *width, 1.0);
                                            ctx.fill();
                                        }
                                    }
                                    LineRun::Image {
                                        url: _,
                                        alt,
                                        link: _,
                                        cache_idx,
                                        x,
                                        y_offset,
                                        width,
                                        height,
                                    } => {
                                        self.paint_inline_image(
                                            ctx,
                                            *cache_idx,
                                            alt,
                                            tx + x,
                                            y + y_offset,
                                            *width,
                                            *height,
                                            fs,
                                        );
                                    }
                                }
                            }
                        }
                    }

                    if matches!(style, LineStyle::H1 | LineStyle::H2) && !runs.is_empty() {
                        ctx.set_fill_color(colors.rule);
                        ctx.begin_path();
                        ctx.rect(pad, *y, w - pad * 2.0, 1.0);
                        ctx.fill();
                    }
                }
                LayoutItem::Table {
                    block_idx,
                    rows,
                    cell_styles,
                    cell_images,
                    header,
                    aligns,
                    y,
                    height: _,
                    row_heights,
                    col_widths,
                    line,
                    viewport_width,
                    content_width,
                } => {
                    let grid = super::table_grid::TableGrid {
                        col_widths,
                        row_heights,
                        line: *line,
                    };
                    let table_h = grid.height();
                    let text_row_h = self.table_text_row_height();
                    let scroll_x = self.block_scroll_offset(*block_idx);
                    let scrollbar_h = if content_width > viewport_width {
                        BLOCK_SCROLLBAR_H + BLOCK_SCROLLBAR_GAP
                    } else {
                        0.0
                    };
                    let content_y = y + scrollbar_h;
                    ctx.save();
                    ctx.clip_rect(pad, content_y, *viewport_width, table_h);
                    ctx.translate(-scroll_x, 0.0);
                    let snapped = super::table_grid::SnappedGrid::new(
                        &grid,
                        pad,
                        content_y,
                        &ctx.transform(),
                        crate::device_scale(),
                    );
                    if let Some(snapped) = &snapped {
                        snapped.paint_stripes(ctx, |ri| {
                            table_style::stripe_color(colors.text, ri, *header)
                        });
                    }
                    ctx.set_font_size(self.font_size);
                    for (ri, row) in rows.iter().enumerate() {
                        let row_h = row_heights.get(ri).copied().unwrap_or(text_row_h);
                        let cy = content_y + grid.row_bottom(ri);
                        // The text row sits in the middle of a taller row.
                        let text_cy = cy + (row_h - text_row_h) * 0.5;
                        for (ci, width) in col_widths.iter().enumerate() {
                            let cx = pad + grid.cell_left(ci);
                            if let Some(text) = row.get(ci) {
                                let spans = emphasis::cell_spans(cell_styles, ri, ci);
                                let images = cell_images::images_of(cell_images, ri, ci);
                                let align = aligns.get(ci).copied().unwrap_or_default();
                                let content_w = self.cell_content_width(text, spans, images);
                                let tx = cx + cell_content_x(*width, content_w, align);
                                self.paint_cell_matches(
                                    ctx,
                                    (text, spans, images),
                                    tx,
                                    cy,
                                    row_h,
                                    v.accent,
                                );
                                ctx.set_fill_color(colors.text);
                                self.paint_cell_text(
                                    ctx,
                                    (text, spans, images),
                                    tx,
                                    text_cy + text_row_h * 0.36,
                                );
                                ctx.set_font(Arc::clone(&font));
                                for (k, image) in images.iter().enumerate() {
                                    let ix = tx + self.cell_image_x(text, spans, images, k);
                                    let iy = cy + (row_h - image.height) * 0.5;
                                    self.paint_inline_image(
                                        ctx,
                                        image.cache_idx,
                                        &image.image.alt,
                                        ix,
                                        iy,
                                        image.width,
                                        image.height,
                                        self.font_size,
                                    );
                                }
                            }
                        }
                    }
                    if let Some(snapped) = &snapped {
                        snapped.paint_lines(ctx, table_style::grid_color(colors.text));
                    }
                    ctx.restore();

                    if *content_width > *viewport_width {
                        self.paint_block_scrollbar(
                            ctx,
                            *y,
                            *viewport_width,
                            *content_width,
                            scroll_x,
                        );
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
                    let code_pad_x = self.font_size;
                    let code_pad_y = self.font_size * 0.75;
                    let fs = LineStyle::Code.font_size(self.font_size);
                    let scroll_x = self.block_scroll_offset(*block_idx);
                    let scrollbar_h = if content_width > viewport_width {
                        BLOCK_SCROLLBAR_H + BLOCK_SCROLLBAR_GAP
                    } else {
                        0.0
                    };
                    let content_y = y + scrollbar_h;
                    let content_h = height - scrollbar_h;
                    ctx.set_font_size(fs);
                    ctx.set_fill_color(
                        colors
                            .code_background
                            .unwrap_or(Color::rgba(0.5, 0.5, 0.5, 0.12)),
                    );
                    ctx.begin_path();
                    ctx.rounded_rect(pad, *y, *viewport_width, *height, 4.0);
                    ctx.fill();
                    ctx.set_fill_color(colors.text);

                    ctx.save();
                    ctx.clip_rect(pad, content_y, *viewport_width, content_h);
                    ctx.translate(-scroll_x, 0.0);
                    let code_font = self.code_font();
                    ctx.set_font(Arc::clone(&code_font));
                    let metrics = measure_text_metrics(&code_font, "", fs);
                    let mut line_top = content_y + content_h - code_pad_y - line_h;
                    for line in lines {
                        let ty = line_top + line_h * 0.5;
                        let text_y = ty - (metrics.ascent - metrics.descent) * 0.5;
                        ctx.fill_text(line, pad + code_pad_x, text_y);
                        line_top -= line_h;
                    }
                    ctx.set_font(Arc::clone(&font));
                    ctx.restore();

                    if *content_width > *viewport_width {
                        self.paint_block_scrollbar(
                            ctx,
                            *y,
                            *viewport_width,
                            *content_width,
                            scroll_x,
                        );
                    }
                }
            }
        }
    }

    /// Fill a match box behind each matching-text occurrence in a table cell
    /// whose text starts at `text_x`. Cells keep their measured widths, so the
    /// box's padding extends into the cell's own 8 px inset rather than
    /// shifting the text.
    fn paint_cell_matches(
        &self,
        ctx: &mut dyn DrawCtx,
        (text, spans, images): CellContent<'_>,
        text_x: f64,
        cy: f64,
        row_h: f64,
        accent: Color,
    ) {
        let Some(needle) = self.active_matching_text() else {
            return;
        };
        for range in matching::match_ranges(text, needle) {
            let x0 = self.cell_text_x(text, spans, images, range.start);
            let w = self.cell_prefix_width(text, spans, range.end)
                - self.cell_prefix_width(text, spans, range.start);
            ctx.set_fill_color(matching::match_fill(accent));
            ctx.begin_path();
            ctx.rect(
                text_x + x0 - matching::MATCH_PAD,
                cy,
                w + matching::MATCH_PAD * 2.0,
                row_h,
            );
            ctx.fill();
        }
    }

    /// Draw a table cell's text piece by piece, each in its emphasis face,
    /// from baseline-left `(x, y)`, stepping over the cell's images. A
    /// linked piece draws in the accent colour, underlined, as a paragraph's
    /// link does. The caller restores the font.
    fn paint_cell_text(
        &self,
        ctx: &mut dyn DrawCtx,
        (text, spans, images): CellContent<'_>,
        x: f64,
        y: f64,
    ) {
        let colors = self.resolved_colors(&ctx.visuals());
        for between in MarkdownView::cell_text_pieces(text, images) {
            for (range, style) in emphasis::cell_pieces(text, spans) {
                let start = range.start.max(between.start);
                let end = range.end.min(between.end);
                if start >= end {
                    continue;
                }
                let piece = &text[start..end];
                let pen = x + self.cell_text_x(text, spans, images, start);
                let linked = emphasis::link_at(spans, start).is_some();
                ctx.set_fill_color(if linked { colors.link } else { colors.text });
                ctx.set_font(self.run_font(style));
                ctx.fill_text(piece, pen, y);
                if linked {
                    let space = piece.len() - piece.trim_start().len();
                    let skip = self.styled_text_width(&piece[..space], LineStyle::Body, style);
                    let ink_w = self.styled_text_width(piece.trim_end(), LineStyle::Body, style);
                    ctx.begin_path();
                    ctx.rect(pen + skip, y - 2.0, ink_w - skip, 1.0);
                    ctx.fill();
                }
                if style.strikethrough {
                    let space = piece.len() - piece.trim_start().len();
                    let skip = self.styled_text_width(&piece[..space], LineStyle::Body, style);
                    let ink = piece.trim_end();
                    let ink_w = self.styled_text_width(ink, LineStyle::Body, style);
                    self.paint_strike(ctx, self.font_size, pen + skip, y, ink_w - skip);
                }
            }
        }
    }

    /// agg-sharp `TextWidget`'s strikethrough: a 1 px line in the text colour
    /// at the vertical centre of the text's bounds (ascent above the baseline
    /// down to descent below it).
    fn paint_strike(&self, ctx: &mut dyn DrawCtx, font_size: f64, x: f64, baseline: f64, w: f64) {
        let metrics = measure_text_metrics(&self.active_font(), "", font_size);
        let center = baseline + (metrics.ascent - metrics.descent) * 0.5;
        ctx.begin_path();
        ctx.rect(x, center - 0.5, w, 1.0);
        ctx.fill();
    }

    fn paint_block_scrollbar(
        &self,
        ctx: &mut dyn DrawCtx,
        block_y: f64,
        viewport_width: f64,
        content_width: f64,
        offset: f64,
    ) {
        let bar = scrollbar_rect(block_y, viewport_width);
        let thumb = scrollbar_thumb(bar, viewport_width, content_width, offset);
        ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.5, 0.18));
        ctx.begin_path();
        ctx.rounded_rect(self.padding + bar.x, bar.y, bar.width, bar.height, 4.0);
        ctx.fill();
        ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.5, 0.55));
        ctx.begin_path();
        ctx.rounded_rect(
            self.padding + thumb.x,
            thumb.y,
            thumb.width,
            thumb.height,
            4.0,
        );
        ctx.fill();
    }
}
