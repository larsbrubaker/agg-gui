//! Horizontal scrolling of `MarkdownView`'s wide blocks (tables and code
//! blocks): per-block scroll offsets, the scrollbar's geometry and hit
//! testing, and thumb dragging. `event.rs` drives these from the mouse;
//! `layout.rs` clamps offsets and `paint.rs` draws the bars.

use crate::geometry::{Point, Rect};

use super::{point_in_rect, BlockScroll, LayoutItem, MarkdownView};

impl MarkdownView {
    pub(super) fn block_scroll_mut(&mut self, block_idx: usize) -> &mut BlockScroll {
        if block_idx >= self.block_scrolls.len() {
            self.block_scrolls
                .resize(block_idx + 1, BlockScroll::default());
        }
        &mut self.block_scrolls[block_idx]
    }

    pub(super) fn block_scroll_offset(&self, block_idx: usize) -> f64 {
        self.block_scrolls
            .get(block_idx)
            .map(|s| s.offset)
            .unwrap_or(0.0)
    }

    pub(super) fn hit_scrollbar(&self, pos: Point) -> Option<BlockHit> {
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

    pub(super) fn point_over_scrollable_block(&self, pos: Point) -> Option<(usize, f64, f64)> {
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

    pub(super) fn block_metrics(&self, block_idx: usize) -> Option<(Rect, f64, f64)> {
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

    pub(super) fn dragging_block(&self) -> Option<usize> {
        self.block_scrolls
            .iter()
            .enumerate()
            .find_map(|(idx, scroll)| scroll.dragging.then_some(idx))
    }

    pub(super) fn scroll_block_to(
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

    pub(super) fn drag_block_scrollbar(&mut self, block_idx: usize, pos: Point) -> bool {
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

#[derive(Clone, Copy)]
pub(super) struct BlockHit {
    pub(super) block_idx: usize,
    pub(super) viewport_width: f64,
    pub(super) content_width: f64,
    pub(super) bar: Rect,
    pub(super) thumb: Rect,
    pub(super) on_thumb: bool,
}

pub(super) const BLOCK_SCROLLBAR_H: f64 = 10.0;
pub(super) const BLOCK_SCROLLBAR_GAP: f64 = 4.0;
const BLOCK_SCROLLBAR_MIN_THUMB: f64 = 24.0;

pub(super) fn scrollbar_rect(block_y: f64, viewport_width: f64) -> Rect {
    Rect::new(0.0, block_y + 1.0, viewport_width, BLOCK_SCROLLBAR_H)
}

pub(super) fn scrollbar_thumb(
    bar: Rect,
    viewport_width: f64,
    content_width: f64,
    offset: f64,
) -> Rect {
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

pub(super) fn clamp_block_offset(offset: f64, viewport_width: f64, content_width: f64) -> f64 {
    offset
        .clamp(0.0, (content_width - viewport_width).max(0.0))
        .round()
}
