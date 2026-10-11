//! The rich editor's laid-out document — a port of agg-sharp's
//! `RichDocumentView.cs` (`RichDocumentView`, with `EndsList`, its list
//! numbering, its block stacking and the painting of only the blocks in view).
//!
//! [`RichDocumentView`] keeps one cached [`RichLayout`] per block, stacked top
//! to bottom, and paints the ones in view through [`RichBlockPainter`].  Its
//! height is the document's, so the editor widget's scroll area scrolls it.
//! Blocks are matched to their cached layouts by [`RichBlockId`] (C# matched
//! by reference), so an edit re-lays out only the blocks it names, blocks it
//! inserted and blocks whose list number, list end or shape changed.
//!
//! Differences from C#, where the view was a `GuiWidget` child of the edit
//! widget and reached back into it:
//! - The view is plain state; the edit widget (`RichMarkdownEditWidget`)
//!   owns it, sizes it ([`RichDocumentView::set_width`]) and passes in what
//!   C# read from the editor: the document, the scroll offset, the device
//!   scale, the typography epoch (C#'s `TextStyleSettings.Epoch`), a style
//!   builder ([`RichViewSync`]) and the paint state ([`RichViewPaint`]).
//! - The scroll anchor is returned rather than applied: a relayout returns
//!   the scroll offset that keeps the block at the top of the view still,
//!   and the widget scrolls there once its scroll area is laid out.
//! - Raw blocks' rendered hosts and the image cache live in the widget; the
//!   view tells it where a Raw host goes ([`RichDocumentView::raw_host_origin`])
//!   and which blocks show an image ([`RichDocumentView::blocks_showing`]),
//!   and [`image_display_size`] is C#'s `ImageSize` sizing rule.
//! - A relayout does not run from inside painting (C#'s `OnDraw` re-laid out
//!   on a stale style); the widget checks [`RichDocumentView::style_is_current`]
//!   and relays out before it paints.
//! - A block the document has at an index whose layout is another block's
//!   (an edit not yet relaid out) is not painted; C# painted the stale
//!   layout of the removed block.

use std::collections::HashMap;
use std::ops::Range;

use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect, Size};

use super::block::{RichAlignment, RichBlock, RichBlockId, RichBlockKind};
use super::block_layout::{rect_lbrt, RichBlockGeometry, RichBlockLayout, RichLayoutError};
use super::block_painter::RichPaintInputs;
use super::block_painter::{draw_text, RichBlockPainter, RichEditColors, RichImage, RichLayout};
use super::document::RichDocument;
use super::inline::{InlineAtom, InlineAtomKind, RichInline};
use super::layout_line::RichCaret;
use super::layout_style::RichLayoutStyle;
use super::selection::RichSelection;
use super::table_layout::RichTableLayout;

/// What a relayout reads besides the view's own state (C# read these from
/// the edit widget and the process).
pub struct RichViewSync<'a> {
    /// The document being shown.
    pub document: &'a RichDocument,
    /// How far the view is scrolled down from its top, or `None` when it is
    /// not in a scroll area (C#'s `Parent == null`).
    pub scroll_top: Option<f64>,
    /// The device scale the style must be built at.
    pub device_scale: f64,
    /// [`crate::font_settings::current_typography_epoch`]: a text-size change
    /// rebuilds the style.
    pub typography_epoch: u64,
    /// Builds the style when the current one is stale (a theme, scale or text
    /// size change), with the widget's image and Raw block sizing callbacks.
    pub build_style: &'a mut dyn FnMut() -> RichLayoutStyle,
}

/// What painting reads besides the layouts.
pub struct RichViewPaint<'a> {
    /// The document the view was laid out from.
    pub document: &'a RichDocument,
    /// The colours besides the style's.
    pub colors: &'a RichEditColors,
    /// An image atom's loaded image, or `None` while it is not loaded.
    pub loaded_image: &'a dyn Fn(&InlineAtom) -> Option<RichImage>,
    /// The selection to highlight and the caret to draw.
    pub selection: &'a RichSelection,
    /// Whether the caret shows on the side of a wrap before it.
    pub caret_at_line_end: bool,
    /// Whether the editor has focus: only then the caret draws, and only
    /// without focus the empty hint.
    pub focused: bool,
    /// Whether the blink is in its showing half.
    pub caret_showing: bool,
    /// The hint an empty document shows, or `None` when the document is not
    /// empty or has no hint.
    pub empty_hint: Option<&'a str>,
    /// The visible part of the view as (bottom, top) in view coordinates, or
    /// `None` to paint every block.
    pub visible: Option<(f64, f64)>,
}

/// A block's shape as its layout sees it: kind, heading level, list level and
/// marker, alignment (C#'s `LayoutInputs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LayoutInputs {
    kind: RichBlockKind,
    heading_level: u8,
    depth: usize,
    ordered: bool,
    marker: char,
    alignment: RichAlignment,
}

impl LayoutInputs {
    fn of(block: &RichBlock) -> Self {
        let list = block.list.as_ref();
        Self {
            kind: block.kind,
            heading_level: block.heading_level,
            depth: list.map_or(0, |l| l.depth),
            ordered: list.is_some_and(|l| l.ordered),
            marker: list.map_or('\0', |l| l.marker),
            alignment: block.alignment,
        }
    }
}

struct Entry {
    block: RichBlockId,
    /// What the layout was built from besides the text, to notice a block
    /// changed in place by an edit that did not name it.
    inputs: LayoutInputs,
    layout: RichLayout,
    number: i32,
    /// Whether the layout was built as its list's last item, with a
    /// paragraph's space below.
    ends_list: bool,
    /// Distance from the document's top to the top of this block's box.
    top: f64,
    /// Space at the bottom of the layout the next block overlaps (the gap
    /// inside a multi-paragraph quote).
    trim: f64,
}

/// The rich editor's laid-out, stacked document (C#'s `RichDocumentView`).
pub struct RichDocumentView {
    entries: Vec<Entry>,
    style: Option<RichLayoutStyle>,
    style_epoch: Option<u64>,
    width: f64,
    layout_width: f64,
    height: f64,
    layout_count: usize,
    drawn: Range<usize>,
    painter: RichBlockPainter,
}

impl Default for RichDocumentView {
    fn default() -> Self {
        Self::new()
    }
}

impl RichDocumentView {
    /// An empty view with no width yet.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            style: None,
            style_epoch: None,
            width: 0.0,
            layout_width: -1.0,
            height: 1.0,
            layout_count: 0,
            drawn: 0..0,
            painter: RichBlockPainter::new(),
        }
    }

    /// The style the blocks are laid out with, `None` before the first layout
    /// or after [`Self::invalidate_style`].
    pub fn style(&self) -> Option<&RichLayoutStyle> {
        self.style.as_ref()
    }

    /// How many block layouts the view has built: tests count relayouts.
    pub fn layout_count(&self) -> usize {
        self.layout_count
    }

    /// The blocks the last paint drew; empty before any.
    pub fn drawn_blocks(&self) -> Range<usize> {
        self.drawn.clone()
    }

    /// The whole stack's height (at least 1).
    pub fn height(&self) -> f64 {
        self.height
    }

    /// The width the view is laid out to.
    pub fn width(&self) -> f64 {
        self.width
    }

    /// Sizes the view (C#'s `OnBoundsChanged`).  Returns whether the width
    /// changed from the laid-out one, when the widget must
    /// [`Self::relayout_all`].
    pub fn set_width(&mut self, width: f64) -> bool {
        self.width = width;
        width > 0.0 && (width - self.layout_width).abs() > 0.001
    }

    /// How many blocks have a layout: fewer than the document's before the
    /// first width, or between an edit and its relayout.
    pub fn laid_out_count(&self) -> usize {
        self.entries.len()
    }

    /// The layout of block `index`.
    pub fn layout_of(&self, index: usize) -> Option<&RichLayout> {
        self.entries.get(index).map(|e| &e.layout)
    }

    /// Where block `index`'s layout bottom sits in view coordinates.
    pub fn origin_of(&self, index: usize) -> Option<f64> {
        let entry = self.entries.get(index)?;
        Some(self.height - entry.top - entry.layout.height())
    }

    /// The block's box in view coordinates, from the top of its text to the
    /// top of the next block.
    pub fn block_bounds(&self, index: usize) -> Option<Rect> {
        let entry = self.entries.get(index)?;
        let origin = self.height - entry.top - entry.layout.height();
        Some(rect_lbrt(
            0.0,
            origin + entry.trim,
            self.layout_width,
            origin + entry.layout.height(),
        ))
    }

    /// Where Raw block `index`'s rendered host goes (its bottom-left, view
    /// coordinates): the Raw layout's one fragment, at its baseline.
    pub fn raw_host_origin(&self, index: usize) -> Option<Point> {
        let entry = self.entries.get(index)?;
        let RichLayout::Text(layout) = &entry.layout else {
            return None;
        };
        let fragment = layout.lines.first()?.fragments.first()?;
        Some(Point::new(
            fragment.x,
            self.height - entry.top - layout.height + fragment.baseline,
        ))
    }

    /// Whether the style was built at `device_scale` and `typography_epoch`.
    pub fn style_is_current(&self, device_scale: f64, typography_epoch: u64) -> bool {
        self.style
            .as_ref()
            .is_some_and(|s| s.scale() == device_scale)
            && self.style_epoch == Some(typography_epoch)
    }

    /// Forgets the style, so the next layout rebuilds it (a theme change).
    pub fn invalidate_style(&mut self) {
        self.style = None;
    }

    /// Lays every block out again: for a new document, a new width, or a new
    /// style.  Returns the scroll offset that keeps the content at the top of
    /// the view still, when the view is in a scroll area.
    pub fn relayout_all(&mut self, sync: RichViewSync<'_>) -> Result<Option<f64>, RichLayoutError> {
        self.sync(sync, None)
    }

    /// Re-lays out the given blocks after an edit.  Blocks are matched to
    /// their cached layouts by id, so blocks an edit inserted are laid out and
    /// removed ones dropped, while every other block keeps its layout.
    pub fn relayout(
        &mut self,
        sync: RichViewSync<'_>,
        changed: &[RichBlockId],
    ) -> Result<Option<f64>, RichLayoutError> {
        self.sync(sync, Some(changed))
    }

    fn sync(
        &mut self,
        sync: RichViewSync<'_>,
        changed: Option<&[RichBlockId]>,
    ) -> Result<Option<f64>, RichLayoutError> {
        let mut changed = changed;
        if self.width <= 0.0 {
            // Not sized yet; the first width change lays everything out.
            self.entries.clear();
            return Ok(None);
        }

        let anchor = sync.scroll_top.and_then(|top| self.scroll_anchor(top));
        // Taken out while laying out so `layout_block` can count into `self`.
        let current = self.style_is_current(sync.device_scale, sync.typography_epoch);
        let style = match self.style.take() {
            Some(style) if current => style,
            _ => {
                self.style_epoch = Some(sync.typography_epoch);
                changed = None;
                (sync.build_style)()
            }
        };

        if (self.width - self.layout_width).abs() > 0.001 {
            self.layout_width = self.width;
            changed = None;
        }

        let blocks = &sync.document.blocks;
        let mut cached: HashMap<RichBlockId, Entry> = std::mem::take(&mut self.entries)
            .into_iter()
            .map(|e| (e.block, e))
            .collect();
        let numbers = list_numbers(blocks);
        let result = self.layout_entries(blocks, &numbers, &mut cached, changed, &style);
        self.style = Some(style);
        self.entries = result?;
        self.restack(blocks);
        Ok(anchor.map(|(block, into, scroll_top)| {
            self.entries
                .iter()
                .find(|e| e.block == block)
                .map_or(scroll_top, |e| e.top + into)
        }))
    }

    fn layout_entries(
        &mut self,
        blocks: &[RichBlock],
        numbers: &[i32],
        cached: &mut HashMap<RichBlockId, Entry>,
        changed: Option<&[RichBlockId]>,
        style: &RichLayoutStyle,
    ) -> Result<Vec<Entry>, RichLayoutError> {
        let mut next = Vec::with_capacity(blocks.len());
        for (i, block) in blocks.iter().enumerate() {
            let ends = ends_list(blocks, i);
            let reusable = cached.remove(&block.id()).filter(|entry| {
                changed.is_some_and(|c| !c.contains(&block.id()))
                    && entry.number == numbers[i]
                    && entry.ends_list == ends
                    && entry.inputs == LayoutInputs::of(block)
            });
            let entry = match reusable {
                Some(entry) => entry,
                // A renumbered ordered item re-lays out too: an edit above it
                // moved its number.  So does a block an edit reshaped without
                // naming it - list children lifted a level when their parent
                // left the list.
                None => Entry {
                    block: block.id(),
                    inputs: LayoutInputs::of(block),
                    layout: self.layout_block(block, numbers[i], ends, style)?,
                    number: numbers[i],
                    ends_list: ends,
                    top: 0.0,
                    trim: 0.0,
                },
            };
            next.push(entry);
        }
        Ok(next)
    }

    /// The block at the top of the view and how far into it the view starts,
    /// so a relayout can keep that content still: a block above the view
    /// growing (or the one being typed in at the top) must not scroll the text
    /// under the reader's eyes, and a scroll area on its own keeps the bottom
    /// fixed instead.
    fn scroll_anchor(&self, scroll_top: f64) -> Option<(RichBlockId, f64, f64)> {
        if self.entries.is_empty() {
            return None;
        }
        let index = self
            .first_entry_ending_below(scroll_top)
            .min(self.entries.len() - 1);
        let entry = &self.entries[index];
        Some((entry.block, scroll_top - entry.top, scroll_top))
    }

    fn layout_block(
        &mut self,
        block: &RichBlock,
        number: i32,
        ends_list: bool,
        style: &RichLayoutStyle,
    ) -> Result<RichLayout, RichLayoutError> {
        self.layout_count += 1;
        Ok(if block.kind == RichBlockKind::Table {
            RichLayout::Table(RichTableLayout::layout(block, self.layout_width, style)?)
        } else {
            RichLayout::Text(RichBlockLayout::layout(
                block,
                self.layout_width,
                style,
                number,
                ends_list,
            )?)
        })
    }

    /// Recomputes each block's top (a prefix sum of heights) and the total
    /// height.  Consecutive blocks of one quote are stacked without the space
    /// after each, so they read as one quote with one unbroken bar.
    fn restack(&mut self, blocks: &[RichBlock]) {
        let mut top = 0.0;
        for i in 0..self.entries.len() {
            let block = &blocks[i];
            let joined = i + 1 < self.entries.len()
                && block.kind == RichBlockKind::Quote
                && block.quote_group.is_some()
                && blocks[i + 1].kind == RichBlockKind::Quote
                && same_group(&blocks[i + 1], block);
            let trim = match (&self.style, joined) {
                (Some(style), true) => style.block_spacing(block, false).1,
                _ => 0.0,
            };
            let entry = &mut self.entries[i];
            entry.top = top;
            entry.trim = trim;
            top += entry.layout.height() - trim;
        }

        self.height = f64::max(top, 1.0);
    }

    /// The blocks whose inlines (or table cells' inlines) hold an image atom
    /// `matches` accepts: the blocks to relayout when that image loads (C#'s
    /// `ImageLoaded` / `ShowsImage`, the URL match supplied by the caller).
    pub fn blocks_showing(
        &self,
        document: &RichDocument,
        matches: &dyn Fn(&InlineAtom) -> bool,
    ) -> Vec<RichBlockId> {
        let shows = |inlines: &[RichInline]| {
            inlines.iter().any(|inline| {
                inline
                    .as_atom()
                    .is_some_and(|a| a.kind == InlineAtomKind::Image && matches(a))
            })
        };
        let laid_out: Vec<RichBlockId> = self.entries.iter().map(|e| e.block).collect();
        document
            .blocks
            .iter()
            .filter(|block| laid_out.contains(&block.id()))
            .filter(|block| {
                if block.kind != RichBlockKind::Table {
                    shows(&block.inlines)
                } else {
                    block
                        .table_rows
                        .iter()
                        .any(|row| row.iter().any(|cell| shows(&cell.inlines)))
                }
            })
            .map(RichBlock::id)
            .collect()
    }

    /// Paints the blocks in view, the selection, the caret and the empty
    /// hint.  Only the blocks in view are painted, so a long help document
    /// costs what a screenful does.
    pub fn draw(&mut self, ctx: &mut dyn DrawCtx, paint: &RichViewPaint<'_>) {
        let Some(style) = self.style.as_ref() else {
            return;
        };
        if self.entries.is_empty() {
            return;
        }

        let (bottom, top) = paint.visible.unwrap_or((0.0, self.height));
        let from_top = self.height - top;
        let to_top = self.height - bottom;
        let first = self.first_entry_ending_below(from_top);
        let mut end = first;
        let selection = paint.selection;
        let inputs = RichPaintInputs {
            style,
            colors: paint.colors,
            loaded_image: paint.loaded_image,
        };
        let mut i = first;
        while i < self.entries.len() && self.entries[i].top < to_top {
            end = i + 1;
            let entry = &self.entries[i];
            let origin = self.height - entry.top - entry.layout.height();
            let block = paint
                .document
                .blocks
                .get(i)
                .filter(|b| b.id() == entry.block);
            if let Some(block) = block {
                self.painter.draw_selection(
                    ctx,
                    &entry.layout,
                    origin,
                    selection,
                    i,
                    self.layout_width,
                    paint.colors.selection,
                );
                self.painter
                    .draw_block(ctx, &entry.layout, block, origin, &inputs);
                if paint.focused
                    && paint.caret_showing
                    && selection.is_empty()
                    && i == selection.caret.block_index
                {
                    let caret = selection.caret;
                    let caret = RichCaret {
                        offset: caret.offset,
                        at_line_end: paint.caret_at_line_end,
                        row: caret.row,
                        column: caret.column,
                    };
                    self.painter.draw_caret(
                        ctx,
                        &entry.layout,
                        block,
                        origin,
                        caret,
                        style,
                        paint.colors.caret,
                    );
                }
            }
            i += 1;
        }
        self.drawn = first..end;

        let hint = paint.empty_hint.filter(|h| !h.is_empty());
        if let (false, Some(hint)) = (paint.focused, hint) {
            let entry = &self.entries[0];
            let block = paint
                .document
                .blocks
                .first()
                .filter(|b| b.id() == entry.block);
            if let (RichLayout::Text(layout), Some(block)) = (&entry.layout, block) {
                if let Some(line) = layout.lines.first() {
                    let origin = self.height - entry.top - layout.height;
                    let face = style.block_face(block);
                    draw_text(
                        ctx,
                        hint,
                        &face,
                        line.left,
                        origin + line.baseline,
                        paint.colors.hint,
                    );
                }
            }
        }
    }

    /// The first block whose box ends below `from_top` (distance from the
    /// view's top), or the block count when none does.
    fn first_entry_ending_below(&self, from_top: f64) -> usize {
        self.entries
            .partition_point(|e| e.top + e.layout.height() <= from_top)
    }
}

fn same_group(a: &RichBlock, b: &RichBlock) -> bool {
    match (&a.quote_group, &b.quote_group) {
        (Some(x), Some(y)) => std::rc::Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// Whether the block at `index` is a list's last item: a list item followed
/// by the document's end, a block that is not a list item, or an item of
/// another list.  Its layout takes a paragraph's space below so the list does
/// not run into what follows (a bullet list straight into a numbered one).
/// An index past the end is no list item, so it ends nothing.
pub fn ends_list(blocks: &[RichBlock], index: usize) -> bool {
    let Some(block) = blocks.get(index) else {
        return false;
    };
    if block.kind != RichBlockKind::ListItem {
        return false;
    }

    let Some(next) = blocks.get(index + 1) else {
        return true;
    };
    let ordered = |b: &RichBlock| b.list.as_ref().is_some_and(|l| l.ordered);
    let depth = |b: &RichBlock| b.list.as_ref().map_or(0, |l| l.depth);
    next.kind != RichBlockKind::ListItem
        || !same_list_group(next, block)
        || ordered(next) != ordered(block) && depth(next) == depth(block)
}

fn same_list_group(a: &RichBlock, b: &RichBlock) -> bool {
    match (&a.list_group, &b.list_group) {
        (Some(x), Some(y)) => std::rc::Rc::ptr_eq(x, y),
        (None, None) => true,
        _ => false,
    }
}

/// The number each block's list marker shows: ordered items count up from
/// their list's start at each depth, and a shallower item restarts the deeper
/// count.  Every other block (bullets included, whose marker never changes)
/// is 1.
pub fn list_numbers(blocks: &[RichBlock]) -> Vec<i32> {
    let mut numbers = vec![1; blocks.len()];
    let mut counters: HashMap<usize, i32> = HashMap::new();
    let mut group: Option<&RichBlock> = None;
    for (i, block) in blocks.iter().enumerate() {
        if block.kind != RichBlockKind::ListItem {
            group = None;
            continue;
        }

        if !group.is_some_and(|g| same_list_group(g, block)) {
            counters.clear();
        }

        group = Some(block);
        let depth = block.list.as_ref().map_or(0, |l| l.depth);
        counters.retain(|&deeper, _| deeper <= depth);
        let start = block.list.as_ref().map_or(1, |l| l.start_number);
        let count = counters
            .entry(depth)
            .and_modify(|c| *c += 1)
            .or_insert(start);

        // Only an ordered item shows its number, so a bullet's never changes
        // and never forces a relayout.
        if block.list.as_ref().is_some_and(|l| l.ordered) {
            numbers[i] = *count;
        }
    }
    numbers
}

/// How big an image atom lays out (C#'s `ImageSize`): a loaded image of
/// `image` (width, height) pixels never draws wider than the column and
/// scales down keeping its shape; one not loaded yet is the layout's own
/// placeholder, a square [`placeholder_line`] tall.
pub fn image_display_size(
    image: Option<(u32, u32)>,
    layout_width: f64,
    placeholder_line: f64,
) -> Size {
    match image {
        // A zero-width image divides to an infinite scale, clamped to 1, as
        // in C#.
        Some((width, height)) => {
            let scale = f64::min(1.0, f64::max(layout_width, 1.0) / f64::from(width));
            Size::new(f64::from(width) * scale, f64::from(height) * scale)
        }
        None => Size::new(placeholder_line, placeholder_line),
    }
}

/// One body line's height in `style`: the size of an unloaded image's
/// placeholder.
pub fn placeholder_line(style: &RichLayoutStyle) -> f64 {
    let face = style.block_face(&RichBlock::default());
    face.ascent_px() + face.descent_px().abs()
}
