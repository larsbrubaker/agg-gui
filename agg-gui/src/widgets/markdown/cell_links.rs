//! Links inside `MarkdownView` table cells (C#'s `TextLinkX` children of an
//! `AggTableCell`): where each linked run of a cell's text sits, for the
//! click and hover hit-test in `markdown.rs` (`link_at`) and for the cell
//! introspection in `introspect.rs`.
//!
//! A cell's links are `link` spans of its `CellSpan`s (`emphasis.rs`,
//! written by `parse.rs`); `paint.rs` draws them in the accent colour,
//! underlined. Geometry follows the table paint: rows from the top, columns
//! shifted by the table's horizontal scroll and clipped to its viewport.

use crate::geometry::Point;

use super::cell_images::images_of;
use super::table_style::cell_content_x;
use super::{emphasis, point_in_rect, LayoutItem, MarkdownView};

/// The urls a cell's text links to, one per linked run, in order.
pub(super) fn cell_link_urls(spans: &[emphasis::CellSpan]) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    let mut prev_end = None;
    let mut prev_url: Option<&str> = None;
    for span in spans {
        let Some(url) = span.link.as_deref() else {
            prev_url = None;
            continue;
        };
        // Pieces of one link split by emphasis (`[a **b**](u)`) are one link.
        if prev_end == Some(span.start) && prev_url == Some(url) {
            prev_end = Some(span.end);
            continue;
        }
        urls.push(url.to_string());
        prev_end = Some(span.end);
        prev_url = Some(url);
    }
    urls
}

impl MarkdownView {
    /// The url of the table-cell link under `pos` (this view's coordinates).
    pub(super) fn table_link_at(&self, pos: Point) -> Option<&str> {
        let pad = self.padding;
        for item in &self.items {
            let LayoutItem::Table {
                block_idx,
                rows,
                cell_styles,
                cell_images,
                aligns,
                y,
                row_heights,
                col_widths,
                viewport_width,
                content_width,
                ..
            } = item
            else {
                continue;
            };
            if pos.x < pad || pos.x > pad + viewport_width {
                continue;
            }
            let scroll_x = self.block_scroll_offset(*block_idx);
            let scrollbar_h = if content_width > viewport_width {
                super::BLOCK_SCROLLBAR_H + super::BLOCK_SCROLLBAR_GAP
            } else {
                0.0
            };
            let mut cy = y + scrollbar_h + row_heights.iter().sum::<f64>();
            for (ri, row) in rows.iter().enumerate() {
                let row_h = row_heights.get(ri).copied().unwrap_or(0.0);
                cy -= row_h;
                let mut cx = pad - scroll_x;
                for (ci, width) in col_widths.iter().enumerate() {
                    if let Some(text) = row.get(ci) {
                        let spans = emphasis::cell_spans(cell_styles, ri, ci);
                        let images = images_of(cell_images, ri, ci);
                        let align = aligns.get(ci).copied().unwrap_or_default();
                        let content_w = self.cell_content_width(text, spans, images);
                        let tx = cx + cell_content_x(*width, content_w, align);
                        for span in spans {
                            let Some(url) = span.link.as_deref() else {
                                continue;
                            };
                            let x0 = tx + self.cell_text_x(text, spans, images, span.start);
                            let w = self.cell_prefix_width(text, spans, span.end)
                                - self.cell_prefix_width(text, spans, span.start);
                            if point_in_rect(pos, x0, cy, w, row_h) {
                                return Some(url);
                            }
                        }
                    }
                    cx += width;
                }
            }
        }
        None
    }
}
