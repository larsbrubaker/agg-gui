//! Painting for `MarkdownView`.
//!
//! All drawing in this module uses the already laid-out Y-up row positions
//! produced by `layout.rs`. Each text run draws in the face of its emphasis
//! (`MarkdownView::run_font`, `emphasis.rs`), struck through when it is.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::text::measure_text_metrics;

use super::{
    emphasis, image_loader, is_rect_visible_in_root, matching, scrollbar_rect, scrollbar_thumb,
    ImageState, LayoutItem, LineRun, LineStyle, MarkdownView, BLOCK_SCROLLBAR_GAP,
    BLOCK_SCROLLBAR_H,
};

impl MarkdownView {
    pub(super) fn paint_markdown(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let pad = self.padding;
        let w = self.bounds.width;
        let font = self.active_font();
        ctx.set_font(Arc::clone(&font));
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
                        ctx.set_fill_color(v.separator);
                        ctx.begin_path();
                        ctx.rect(pad + indent - 12.0, *y, 3.0, *height);
                        ctx.fill();
                    }

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
                            for run in runs {
                                if let LineRun::Text { text, x, .. } = run {
                                    ctx.fill_text(text, tx + x + 4.0, text_y);
                                }
                            }
                        }
                        _ => {
                            ctx.set_fill_color(v.text_color);
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
                                            ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.5, 0.14));
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
                                            v.accent
                                        } else {
                                            v.text_color
                                        });
                                        let text_x = if *code {
                                            tx + x + self.font_size * 0.35
                                        } else {
                                            tx + x + lead
                                        };
                                        ctx.set_font(self.run_font(*run_style));
                                        ctx.fill_text(text, text_x, text_y);
                                        if run_style.strikethrough {
                                            // Past the run's lead space, as C# strikes
                                            // each word's own text.
                                            let space = text.len() - text.trim_start().len();
                                            let skip = self.styled_text_width(
                                                &text[..space],
                                                *style,
                                                *run_style,
                                            );
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
                                        let rx = tx + x;
                                        let ry = y + y_offset;
                                        if let Some(entry) = self.image_cache.get(*cache_idx) {
                                            if is_rect_visible_in_root(ctx, rx, ry, *width, *height)
                                            {
                                                let should_load = if let Ok(mut state) =
                                                    entry.state.lock()
                                                {
                                                    if matches!(*state, ImageState::RemotePending) {
                                                        *state = ImageState::Loading;
                                                        true
                                                    } else {
                                                        false
                                                    }
                                                } else {
                                                    false
                                                };
                                                if should_load {
                                                    image_loader::load_remote_image(
                                                        entry.url.clone(),
                                                        std::sync::Arc::clone(&entry.state),
                                                    );
                                                }
                                            }
                                            let ready =
                                                entry.state.lock().ok().and_then(|mut state| {
                                                    match &mut *state {
                                                        ImageState::Ready { image, seen } => {
                                                            *seen = true;
                                                            Some(image.clone())
                                                        }
                                                        _ => None,
                                                    }
                                                });
                                            if let Some(image) = ready {
                                                ctx.draw_image_rgba(
                                                    image.data.as_slice(),
                                                    image.width,
                                                    image.height,
                                                    rx,
                                                    ry,
                                                    *width,
                                                    *height,
                                                );
                                            } else {
                                                ctx.set_fill_color(Color::rgba(
                                                    0.5, 0.5, 0.5, 0.15,
                                                ));
                                                ctx.begin_path();
                                                ctx.rounded_rect(rx, ry, *width, *height, 3.0);
                                                ctx.fill();
                                                ctx.set_fill_color(v.text_dim);
                                                ctx.set_font_size(self.font_size * 0.85);
                                                let label = if alt.is_empty() {
                                                    "image".to_string()
                                                } else {
                                                    alt.clone()
                                                };
                                                ctx.fill_text(&label, rx + 8.0, ry + height * 0.5);
                                                ctx.set_font_size(fs);
                                                ctx.set_fill_color(v.text_color);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if matches!(style, LineStyle::H1 | LineStyle::H2) && !runs.is_empty() {
                        ctx.set_fill_color(v.separator);
                        ctx.begin_path();
                        ctx.rect(pad, *y, w - pad * 2.0, 1.0);
                        ctx.fill();
                    }
                }
                LayoutItem::Table {
                    block_idx,
                    rows,
                    cell_styles,
                    header,
                    y,
                    height: _,
                    row_h,
                    col_widths,
                    viewport_width,
                    content_width,
                } => {
                    let table_w: f64 = col_widths.iter().sum();
                    let scroll_x = self.block_scroll_offset(*block_idx);
                    let scrollbar_h = if content_width > viewport_width {
                        BLOCK_SCROLLBAR_H + BLOCK_SCROLLBAR_GAP
                    } else {
                        0.0
                    };
                    let content_y = y + scrollbar_h;
                    ctx.save();
                    ctx.clip_rect(pad, content_y, *viewport_width, row_h * rows.len() as f64);
                    ctx.translate(-scroll_x, 0.0);
                    let mut cy = content_y + row_h * rows.len() as f64;
                    ctx.set_font_size(self.font_size);
                    for (ri, row) in rows.iter().enumerate() {
                        cy -= row_h;
                        if ri == 0 && *header {
                            ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.5, 0.10));
                            ctx.begin_path();
                            ctx.rect(pad, cy, table_w, *row_h);
                            ctx.fill();
                        }
                        let mut cx = pad;
                        for (ci, width) in col_widths.iter().enumerate() {
                            ctx.set_fill_color(v.separator);
                            ctx.begin_path();
                            ctx.rect(cx, cy, 1.0, *row_h);
                            ctx.rect(cx, cy, *width, 1.0);
                            ctx.fill();
                            if let Some(text) = row.get(ci) {
                                let spans = emphasis::cell_spans(cell_styles, ri, ci);
                                self.paint_cell_matches(
                                    ctx,
                                    text,
                                    spans,
                                    cx + 8.0,
                                    cy,
                                    *row_h,
                                    v.accent,
                                );
                                ctx.set_fill_color(v.text_color);
                                self.paint_cell_text(ctx, text, spans, cx + 8.0, cy + row_h * 0.36);
                                ctx.set_font(Arc::clone(&font));
                            }
                            cx += width;
                        }
                        ctx.set_fill_color(v.separator);
                        ctx.begin_path();
                        ctx.rect(pad + table_w, cy, 1.0, *row_h);
                        ctx.fill();
                    }
                    ctx.set_fill_color(v.separator);
                    ctx.begin_path();
                    ctx.rect(pad, content_y + row_h * rows.len() as f64, table_w, 1.0);
                    ctx.fill();
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
                    ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.5, 0.12));
                    ctx.begin_path();
                    ctx.rounded_rect(pad, *y, *viewport_width, *height, 4.0);
                    ctx.fill();
                    ctx.set_fill_color(v.text_color);

                    ctx.save();
                    ctx.clip_rect(pad, content_y, *viewport_width, content_h);
                    ctx.translate(-scroll_x, 0.0);
                    let metrics = measure_text_metrics(&font, "", fs);
                    let mut line_top = content_y + content_h - code_pad_y - line_h;
                    for line in lines {
                        let ty = line_top + line_h * 0.5;
                        let text_y = ty - (metrics.ascent - metrics.descent) * 0.5;
                        ctx.fill_text(line, pad + code_pad_x, text_y);
                        line_top -= line_h;
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
            }
        }
    }

    /// Fill a match box behind each matching-text occurrence in a table cell
    /// whose text starts at `text_x`. Cells keep their measured widths, so the
    /// box's padding extends into the cell's own 8 px inset rather than
    /// shifting the text.
    #[allow(clippy::too_many_arguments)]
    fn paint_cell_matches(
        &self,
        ctx: &mut dyn DrawCtx,
        text: &str,
        spans: &[emphasis::CellSpan],
        text_x: f64,
        cy: f64,
        row_h: f64,
        accent: Color,
    ) {
        let Some(needle) = self.active_matching_text() else {
            return;
        };
        for range in matching::match_ranges(text, needle) {
            let x0 = self.cell_prefix_width(text, spans, range.start);
            let w = self.cell_prefix_width(text, spans, range.end) - x0;
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
    /// from baseline-left `(x, y)`. The caller restores the font.
    fn paint_cell_text(
        &self,
        ctx: &mut dyn DrawCtx,
        text: &str,
        spans: &[emphasis::CellSpan],
        x: f64,
        y: f64,
    ) {
        let mut pen = x;
        for (range, style) in emphasis::cell_pieces(text, spans) {
            let piece = &text[range];
            let w = self.styled_text_width(piece, LineStyle::Body, style);
            ctx.set_font(self.run_font(style));
            ctx.fill_text(piece, pen, y);
            if style.strikethrough {
                let space = piece.len() - piece.trim_start().len();
                let skip = self.styled_text_width(&piece[..space], LineStyle::Body, style);
                let ink = piece.trim_end();
                let ink_w = self.styled_text_width(ink, LineStyle::Body, style);
                self.paint_strike(ctx, self.font_size, pen + skip, y, ink_w - skip);
            }
            pen += w;
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
