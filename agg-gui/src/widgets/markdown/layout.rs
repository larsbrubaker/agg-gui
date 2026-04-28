//! Layout pass for `MarkdownView`.
//!
//! Converts parsed markdown flow items into positioned rows in the framework's
//! Y-up coordinate system.

use crate::geometry::Size;
use crate::text::measure_text_metrics;

use super::{
    clamp_block_offset, ImageState, InlineItem, LayoutItem, LineRun, LineStyle, MarkdownView,
    BLOCK_SCROLLBAR_GAP, BLOCK_SCROLLBAR_H,
};

impl MarkdownView {
    pub(super) fn layout_markdown(&mut self, available: Size) -> Size {
        let pad = self.padding;
        let viewport_w = crate::widget::current_viewport().width;
        let wrap_w = if available.width.is_finite() && available.width < viewport_w * 2.0 {
            available.width
        } else {
            viewport_w
        };
        let max_w = (wrap_w - pad * 2.0).max(1.0);

        let paragraphs = self.parse_paragraphs();
        let mut laid_out = Vec::new();
        let mut block_idx = 0usize;

        for item in &paragraphs {
            match item {
                super::ParagraphItem::Rule => laid_out.push(LayoutItem::Line {
                    runs: Vec::new(),
                    style: LineStyle::Rule,
                    indent: 0.0,
                    quote: false,
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
                        y: 0.0,
                        height: metrics.line_height * 0.65,
                    });
                }
                super::ParagraphItem::Table(rows) => {
                    let (col_widths, row_h, mut height) = self.layout_table(rows);
                    let content_width = col_widths.iter().sum::<f64>();
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
                        y: 0.0,
                        height,
                        row_h,
                        col_widths,
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
                } => {
                    let font_size = style.font_size(self.font_size);
                    let metrics = measure_text_metrics(&self.active_font(), "", font_size);
                    let line_h = metrics.line_height * 1.3;
                    let avail = (max_w - indent).max(1.0);
                    let mut runs = Vec::new();
                    let mut used = 0.0;
                    let mut row_h = line_h;

                    for inline in items {
                        match inline {
                            InlineItem::Text { text, link, code } => {
                                for word in text.split_whitespace() {
                                    let mut value = word.to_string();
                                    if used > 0.0 {
                                        value.insert(0, ' ');
                                    }
                                    let mut w = self.run_width(&value, *style, *code);
                                    if used > 0.0 && used + w > avail {
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
                                        value = word.to_string();
                                        w = self.run_width(&value, *style, *code);
                                    }
                                    Self::push_text_run(
                                        &mut runs,
                                        value,
                                        link.clone(),
                                        *code,
                                        used,
                                        w,
                                    );
                                    used += w;
                                }
                            }
                            InlineItem::Image { url, alt, link } => {
                                let cache_idx = self.get_or_load_image(url);
                                let (iw, ih) = self.inline_image_size(cache_idx, alt, avail);
                                if used > 0.0 && used + iw > avail {
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
                                used += iw + 4.0;
                                row_h = row_h.max(ih);
                            }
                        }
                    }
                    if !runs.is_empty() {
                        Self::push_line(&mut laid_out, &mut runs, *style, *indent, *quote, row_h);
                    }
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
        self.bounds = crate::geometry::Rect::new(0.0, 0.0, wrap_w, total_h);
        self.rebuild_selection_model();
        Size::new(wrap_w, total_h)
    }

    fn text_width(&self, text: &str, style: LineStyle) -> f64 {
        let font_size = style.font_size(self.font_size);
        measure_text_metrics(&self.active_font(), text, font_size).width
    }

    fn run_width(&self, text: &str, style: LineStyle, code: bool) -> f64 {
        let width = self.text_width(text, style);
        if code {
            width + self.font_size * 0.75
        } else {
            width
        }
    }

    fn inline_image_size(&self, cache_idx: usize, alt: &str, max_w: f64) -> (f64, f64) {
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
            let w = self.text_width(label, LineStyle::Body) + 16.0;
            (w.min(max_w), self.font_size * 1.45)
        }
    }

    fn push_text_run(
        runs: &mut Vec<LineRun>,
        text: String,
        link: Option<String>,
        code: bool,
        x: f64,
        width: f64,
    ) {
        if let Some(LineRun::Text {
            text: last,
            width: last_w,
            link: last_link,
            code: last_code,
            ..
        }) = runs.last_mut()
        {
            if *last_link == link && *last_code == code {
                last.push_str(&text);
                *last_w += width;
                return;
            }
        }
        runs.push(LineRun::Text {
            text,
            link,
            code,
            x,
            width,
        });
    }

    fn layout_table(&self, rows: &[Vec<String>]) -> (Vec<f64>, f64, f64) {
        let row_h = measure_text_metrics(&self.active_font(), "", self.font_size).line_height * 1.6;
        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut widths = vec![0.0_f64; cols];
        for row in rows {
            for (col, text) in row.iter().enumerate() {
                widths[col] = widths[col].max(self.text_width(text, LineStyle::Body) + 24.0);
            }
        }
        (widths, row_h, row_h * rows.len() as f64)
    }

    fn layout_code_block(&self, lines: &[String], max_w: f64) -> (f64, f64, f64) {
        let font_size = LineStyle::Code.font_size(self.font_size);
        let metrics = measure_text_metrics(&self.active_font(), "", font_size);
        let line_h = metrics.line_height * 1.35;
        let max_line_w = lines
            .iter()
            .map(|line| self.text_width(line, LineStyle::Code))
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
            y: 0.0,
            height,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::text::Font;
    use crate::widget::Widget;

    use super::super::ImagePixels;
    use super::*;

    const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

    fn test_font() -> Arc<Font> {
        Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
    }

    #[test]
    fn wide_code_block_does_not_expand_document_width() {
        crate::widget::set_current_viewport(Size::new(220.0, 200.0));
        let mut view = MarkdownView::new(
            "```text\nthis line is intentionally much much wider than the viewport\n```",
            test_font(),
        )
        .with_font_size(12.0)
        .with_padding(8.0);

        let size = view.layout_markdown(Size::new(220.0, 1000.0));

        assert_eq!(size.width, 220.0);
        let code = view.items.iter().find_map(|item| {
            if let LayoutItem::CodeBlock {
                viewport_width,
                content_width,
                ..
            } = item
            {
                Some((*viewport_width, *content_width))
            } else {
                None
            }
        });
        let (viewport_width, content_width) = code.expect("code block item");
        assert!(content_width > viewport_width);
    }

    #[test]
    fn wide_table_does_not_expand_document_width() {
        crate::widget::set_current_viewport(Size::new(220.0, 200.0));
        let mut view = MarkdownView::new(
            "| Column | Value |\n| --- | --- |\n| ThisIsAnExtremelyLongUnbrokenTableCell | another-long-value |",
            test_font(),
        )
        .with_font_size(12.0)
        .with_padding(8.0);

        let size = view.layout_markdown(Size::new(220.0, 1000.0));

        assert_eq!(size.width, 220.0);
        let table = view.items.iter().find_map(|item| {
            if let LayoutItem::Table {
                viewport_width,
                content_width,
                ..
            } = item
            {
                Some((*viewport_width, *content_width))
            } else {
                None
            }
        });
        let (viewport_width, content_width) = table.expect("table item");
        assert!(content_width > viewport_width);
    }

    #[test]
    fn ready_remote_image_stays_dirty_until_painted() {
        crate::widget::set_current_viewport(Size::new(220.0, 200.0));
        let mut view = MarkdownView::new("![badge](https://example.com/badge.svg)", test_font())
            .with_font_size(12.0)
            .with_padding(8.0);

        view.layout_markdown(Size::new(220.0, 1000.0));
        let state = Arc::clone(&view.image_cache[0].state);
        *state.lock().expect("image state") = ImageState::Ready {
            image: ImagePixels {
                data: Arc::new(vec![255, 0, 0, 255]),
                width: 1,
                height: 1,
            },
            seen: false,
        };

        view.layout_markdown(Size::new(220.0, 1000.0));

        assert!(
            view.needs_draw(),
            "layout must not clear a freshly loaded image before retained parents repaint"
        );
    }
}
