//! Paints one laid-out block — a port of agg-sharp's `RichBlockPainter.cs`
//! (`RichBlockPainter` and `RichEditColors`).
//!
//! A [`RichLayout`] is a text block's [`RichBlockLayout`] or a table's
//! [`RichTableLayout`] (C#'s `IRichBlockLayout`, which the painter
//! downcast).  [`RichBlockPainter`] draws its backgrounds, quote bar, list
//! marker, table grid, text, images, selection and caret through agg-gui's
//! [`DrawCtx`], the way `MarkdownView` paints.  Every coordinate in a layout
//! is relative to its block's bottom, so each call takes the y that bottom
//! sits at in the widget.  The text and table colours come from
//! [`RichLayoutStyle`]; the rest from [`RichEditColors`].
//!
//! Differences from C#:
//! - The layouts do not hold their block, so each call takes the
//!   [`RichBlock`] the layout was built from (C#'s `layout.Block`).
//! - C#'s table layout kept its own snapped grid; here the painter keeps one
//!   [`RichTableGridSnap`], refilled in place each frame.
//! - A loaded image is a [`RichImage`] (straight-alpha RGBA8), C#'s
//!   `ImageBuffer`.
//! - An atom's link and strike come from the inline that holds it (agg-gui's
//!   atoms keep their style on the wrapping [`RichInline`]).
//!
//! C# bugs not ported (each pinned by a `rust_only_*` test):
//! - `DrawCaret` threw for a caret in a table row with no cells
//!   (`Math.Clamp(column, 0, -1)`); here nothing is drawn.
//! - `DrawSelection` divided by zero for a table whose first row has no
//!   cells; here nothing is drawn.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::{Point, Rect};

use super::block::RichBlock;
use super::block_layout::{RichBlockGeometry, RichBlockLayout, RichLayoutError};
use super::inline::{InlineAtom, InlineAtomKind, RichInline};
use super::layout_line::{RichCaret, RichLayoutLine};
use super::layout_style::{RichFace, RichLayoutStyle};
use super::selection::RichSelection;
use super::table_layout::{RichTableGridSnap, RichTableLayout};

/// The colours the rich editor paints with besides the text, link and table
/// colours its [`RichLayoutStyle`] holds (C# `RichEditColors`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RichEditColors {
    /// Code blocks, inline code and image placeholders.
    pub code_background: Color,
    /// A quote's bar.
    pub quote_bar: Color,
    /// The selection highlight.
    pub selection: Color,
    /// The caret.
    pub caret: Color,
    /// Placeholder hint text.
    pub hint: Color,
}

/// A loaded image: straight-alpha RGBA8 rows, bottom row last as
/// [`DrawCtx::draw_image_rgba`] takes them (C#'s `ImageBuffer`).
#[derive(Clone, Debug, PartialEq)]
pub struct RichImage {
    /// The pixels, `width * height * 4` bytes.
    pub data: Arc<Vec<u8>>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// One laid-out block, text or table (C# `IRichBlockLayout`).
#[derive(Clone, Debug)]
pub enum RichLayout {
    /// Any block but a table.
    Text(RichBlockLayout),
    /// A Table block.
    Table(RichTableLayout),
}

impl RichBlockGeometry for RichLayout {
    fn height(&self) -> f64 {
        match self {
            Self::Text(text) => text.height,
            Self::Table(table) => table.height,
        }
    }

    fn hit_test(&self, point: Point) -> RichCaret {
        match self {
            Self::Text(text) => text.hit_test(point),
            Self::Table(table) => table.hit_test(point),
        }
    }

    fn caret_rect(&self, caret: RichCaret, caret_width: f64) -> Result<Rect, RichLayoutError> {
        match self {
            Self::Text(text) => text.caret_rect(caret, caret_width),
            Self::Table(table) => table.caret_rect(caret, caret_width),
        }
    }
}

/// Paints laid-out blocks (C#'s static `RichBlockPainter`); keeps the
/// table grid it snaps so the snap reuses its lists every frame.
#[derive(Clone, Debug, Default)]
pub struct RichBlockPainter {
    snap: RichTableGridSnap,
}

/// What [`RichBlockPainter::draw_block`] draws with besides the layout.
pub struct RichPaintInputs<'a> {
    /// The layout's style: fonts, text, link and table colours.
    pub style: &'a RichLayoutStyle,
    /// The other colours.
    pub colors: &'a RichEditColors,
    /// An image atom's loaded image, or `None` while it is not loaded.
    pub loaded_image: &'a dyn Fn(&InlineAtom) -> Option<RichImage>,
}

/// `Rect` from agg-sharp's `RectangleDouble(left, bottom, right, top)`.
fn lbrt(left: f64, bottom: f64, right: f64, top: f64) -> Rect {
    Rect::new(left, bottom, right - left, top - bottom)
}

/// C# `FillRectangle(left, bottom, right, top, color)`.
fn fill_lbrt(ctx: &mut dyn DrawCtx, left: f64, bottom: f64, right: f64, top: f64, color: Color) {
    fill_rect(ctx, lbrt(left, bottom, right, top), color);
}

fn fill_rect(ctx: &mut dyn DrawCtx, rect: Rect, color: Color) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.rect(rect.x, rect.y, rect.width, rect.height);
    ctx.fill();
}

fn offset(rect: Rect, origin_y: f64) -> Rect {
    Rect::new(rect.x, rect.y + origin_y, rect.width, rect.height)
}

impl RichBlockPainter {
    /// A painter with an empty grid snap.
    pub fn new() -> Self {
        Self::default()
    }

    /// The table grid as the last table draw snapped it.
    pub fn snapped_grid(&self) -> &RichTableGridSnap {
        &self.snap
    }

    /// Paints `layout` (laid out from `block`) with its bottom at `origin_y`
    /// (C# `DrawBlock`).
    pub fn draw_block(
        &mut self,
        ctx: &mut dyn DrawCtx,
        layout: &RichLayout,
        block: &RichBlock,
        origin_y: f64,
        inputs: &RichPaintInputs<'_>,
    ) {
        let text = match layout {
            RichLayout::Table(table) => {
                self.draw_table(ctx, table, block, origin_y, inputs);
                return;
            }
            RichLayout::Text(text) => text,
        };

        if let Some(code) = text.code_background {
            fill_rect(ctx, offset(code, origin_y), inputs.colors.code_background);
        }

        if let Some(bar) = text.quote_bar {
            fill_rect(ctx, offset(bar, origin_y), inputs.colors.quote_bar);
        }

        if let (Some(marker), Some(face)) = (&text.marker, &text.marker_face) {
            if text.marker_is_bullet {
                draw_bullet(ctx, text, marker, face, origin_y, inputs.style);
            } else {
                draw_text(
                    ctx,
                    marker,
                    face,
                    text.marker_x,
                    origin_y + text.marker_baseline,
                    inputs.style.text_color,
                );
            }
        }

        draw_lines(ctx, &text.lines, &block.inlines, origin_y, inputs);
    }

    fn snap_grid(&mut self, ctx: &dyn DrawCtx, table: &RichTableLayout, origin_y: f64) -> bool {
        let transform = ctx.transform();
        table.snap_grid(origin_y, transform.tx, transform.ty, &mut self.snap)
    }

    fn draw_table(
        &mut self,
        ctx: &mut dyn DrawCtx,
        table: &RichTableLayout,
        block: &RichBlock,
        origin_y: f64,
        inputs: &RichPaintInputs<'_>,
    ) {
        // Painted like the viewer's AggTable: every line on whole device
        // pixels, stripes between the lines and verticals between the rules,
        // so no pixel of the translucent grid colour is painted twice.  The
        // header is marked only by its bold face, which the layout chose.
        if self.snap_grid(ctx, table, origin_y) {
            let style = inputs.style;
            let lefts = &self.snap.lefts;
            let bottoms = &self.snap.bottoms;
            let line = f64::from(self.snap.thickness);
            for &r in &table.striped_row_indices {
                if r + 1 >= bottoms.len() {
                    continue;
                }
                for c in 0..lefts.len().saturating_sub(1) {
                    fill_lbrt(
                        ctx,
                        lefts[c] + line,
                        bottoms[r + 1] + line,
                        lefts[c + 1],
                        bottoms[r],
                        style.table_stripe_color(),
                    );
                }
            }

            if let (Some(&grid_left), Some(&last_left)) = (lefts.first(), lefts.last()) {
                let grid_right = last_left + line;
                for &bottom in bottoms {
                    let color = style.table_grid_color();
                    fill_lbrt(ctx, grid_left, bottom, grid_right, bottom + line, color);
                }
            }

            for &left in lefts {
                for r in 0..bottoms.len().saturating_sub(1) {
                    fill_lbrt(
                        ctx,
                        left,
                        bottoms[r + 1] + line,
                        left + line,
                        bottoms[r],
                        style.table_grid_color(),
                    );
                }
            }
        }

        for cell in table.rows.iter().flatten() {
            let inlines = block
                .table_rows
                .get(cell.row)
                .and_then(|row| row.get(cell.column))
                .map_or(&[][..], |c| &c.inlines[..]);
            draw_lines(ctx, &cell.lines, inlines, origin_y, inputs);
        }
    }

    /// Highlights the part of `selection` inside block `block_index`: the
    /// whole block for a whole-block selection, otherwise its text line by
    /// line.  In a table the range runs through the cells in reading order,
    /// as the table edit operations walk them (C# `DrawSelection`).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_selection(
        &mut self,
        ctx: &mut dyn DrawCtx,
        layout: &RichLayout,
        origin_y: f64,
        selection: &RichSelection,
        block_index: usize,
        width: f64,
        color: Color,
    ) {
        let start = selection.start();
        let end = selection.end();
        if block_index < start.block_index || block_index > end.block_index {
            return;
        }

        if selection.whole_block {
            match layout {
                RichLayout::Table(table) if self.snap_grid(ctx, table, origin_y) => {
                    self.fill_around_grid(ctx, table, origin_y, width, color);
                }
                _ => fill_lbrt(ctx, 0.0, origin_y, width, origin_y + layout.height(), color),
            }
            return;
        }

        let table = match layout {
            RichLayout::Text(text) => {
                let from = if block_index == start.block_index {
                    start.offset
                } else {
                    0
                };
                let to = if block_index == end.block_index {
                    end.offset
                } else {
                    usize::MAX
                };
                draw_line_selection(ctx, &text.lines, origin_y, from, to, color);
                return;
            }
            RichLayout::Table(table) => table,
        };

        let columns = table.rows.first().map_or(0, |row| row.len());
        if columns == 0 {
            return;
        }

        let first_cell = if block_index == start.block_index {
            start.row * columns + start.column
        } else {
            0
        };
        let last_cell = if block_index == end.block_index {
            end.row * columns + end.column
        } else {
            table.rows.len() * columns - 1
        };
        let mut index = first_cell;
        while index <= last_cell && index / columns < table.rows.len() {
            if let Some(cell) = table.rows[index / columns].get(index % columns) {
                let from = if block_index == start.block_index && index == first_cell {
                    start.offset
                } else {
                    0
                };
                let to = if block_index == end.block_index && index == last_cell {
                    end.offset
                } else {
                    usize::MAX
                };
                draw_line_selection(ctx, &cell.lines, origin_y, from, to, color);
            }
            index += 1;
        }
    }

    // A selected table is shaded everywhere in its block except on the grid
    // lines (just snapped by the caller), so the translucent selection never
    // darkens a line and the grid keeps one tone, as when unselected.
    fn fill_around_grid(
        &self,
        ctx: &mut dyn DrawCtx,
        table: &RichTableLayout,
        origin_y: f64,
        width: f64,
        color: Color,
    ) {
        let lefts = &self.snap.lefts;
        let bottoms = &self.snap.bottoms;
        let (Some(&grid_left), Some(&last_left), Some(&first_bottom), Some(&grid_bottom)) =
            (lefts.first(), lefts.last(), bottoms.first(), bottoms.last())
        else {
            return;
        };
        let line = f64::from(self.snap.thickness);
        let grid_right = last_left + line;
        let grid_top = first_bottom + line;

        // A table wider than the block runs past width, so a band can be
        // empty; skip it rather than fill it inverted.
        let mut fill = |left: f64, bottom: f64, right: f64, top: f64| {
            if right > left && top > bottom {
                fill_lbrt(ctx, left, bottom, right, top, color);
            }
        };

        fill(0.0, grid_top, width, origin_y + table.height);
        fill(0.0, origin_y, width, grid_bottom);
        fill(0.0, grid_bottom, grid_left, grid_top);
        fill(grid_right, grid_bottom, width, grid_top);
        for r in 0..bottoms.len().saturating_sub(1) {
            for c in 0..lefts.len().saturating_sub(1) {
                fill(
                    lefts[c] + line,
                    bottoms[r + 1] + line,
                    lefts[c + 1].min(width),
                    bottoms[r],
                );
            }
        }
    }

    /// Draws the caret.  A stale caret (the document changed under it) is
    /// clamped into the block, or the table, so it still draws somewhere
    /// sensible (C# `DrawCaret`).  `block` is the block `layout` was built
    /// from.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_caret(
        &self,
        ctx: &mut dyn DrawCtx,
        layout: &RichLayout,
        block: &RichBlock,
        origin_y: f64,
        caret: RichCaret,
        style: &RichLayoutStyle,
        color: Color,
    ) {
        let mut caret = caret;
        match layout {
            RichLayout::Table(table) => {
                if table.rows.is_empty() {
                    return;
                }
                let row = caret.row.min(table.rows.len() - 1);
                let Some(last_column) = table.rows[row].len().checked_sub(1) else {
                    return;
                };
                let column = caret.column.min(last_column);
                let length = table.rows[row][column].length;
                caret = RichCaret {
                    row,
                    column,
                    offset: caret.offset.min(length),
                    ..caret
                };
            }
            RichLayout::Text(_) => {
                caret.offset = caret.offset.min(block.text_length(0, 0));
            }
        }

        if let Ok(rect) = layout.caret_rect(caret, style.caret_width()) {
            fill_rect(ctx, offset(rect, origin_y), color);
        }
    }
}

// A bullet is drawn, not typed: a filled dot, a hollow dot, then a small
// square by depth (RichLayoutStyle::bullet_marker), centred on the x-height
// in the box bullet_box_width gave it.
fn draw_bullet(
    ctx: &mut dyn DrawCtx,
    text: &RichBlockLayout,
    marker: &str,
    face: &RichFace,
    origin_y: f64,
    style: &RichLayoutStyle,
) {
    let size = RichLayoutStyle::bullet_box_width(face);
    let center_x = text.marker_x + size / 2.0;
    let center_y = origin_y + text.marker_baseline + face.x_height_px() / 2.0;
    let radius = size * 0.32;
    match marker {
        "\u{2022}" => {
            ctx.set_fill_color(style.text_color);
            ctx.begin_path();
            ctx.circle(center_x, center_y, radius);
            ctx.fill();
        }
        "\u{25E6}" => {
            ctx.set_stroke_color(style.text_color);
            ctx.set_line_width(style.scale().max(1.0));
            ctx.begin_path();
            ctx.circle(center_x, center_y, radius);
            ctx.stroke();
        }
        _ => fill_lbrt(
            ctx,
            center_x - radius,
            center_y - radius,
            center_x + radius,
            center_y + radius,
            style.text_color,
        ),
    }
}

fn draw_lines(
    ctx: &mut dyn DrawCtx,
    lines: &[RichLayoutLine],
    inlines: &[RichInline],
    origin_y: f64,
    inputs: &RichPaintInputs<'_>,
) {
    let style = inputs.style;
    for fragment in lines.iter().flat_map(|line| &line.fragments) {
        let inline = fragment.inline_index.and_then(|i| inlines.get(i));
        let baseline = origin_y + fragment.baseline;
        let Some(face) = &fragment.face else {
            // A box: an image draws itself here; a Raw block's box is covered
            // by its hosted child widget.
            if let Some(atom) = fragment
                .atom
                .as_ref()
                .filter(|a| a.kind == InlineAtomKind::Image)
            {
                let image_box = Rect::new(fragment.x, baseline, fragment.width, fragment.ascent);
                draw_image(ctx, (inputs.loaded_image)(atom), image_box, inputs.colors);
            }
            continue;
        };

        if inline.and_then(|i| i.as_run()).is_some_and(|run| run.code) {
            fill_lbrt(
                ctx,
                fragment.x,
                baseline - fragment.descent,
                fragment.x + fragment.width,
                baseline + fragment.ascent,
                inputs.colors.code_background,
            );
        }

        // A tab is laid out as an advance to the next stop; drawn as a glyph
        // it would show a missing-glyph box.
        if fragment.text.is_empty() || fragment.text == "\t" {
            continue;
        }

        let link = inline.is_some_and(|i| i.link_url.is_some())
            || fragment
                .atom
                .as_ref()
                .is_some_and(|a| a.kind == InlineAtomKind::Autolink);
        let color = if link {
            style.link_color
        } else {
            style.text_color
        };
        draw_text(ctx, &fragment.text, face, fragment.x, baseline, color);
        if inline.is_some_and(|i| i.strike) {
            // The fonts have no strikethrough of their own, so draw it
            // through the middle of the lower case.
            let strike_y = baseline + fragment.ascent * 0.3;
            fill_lbrt(
                ctx,
                fragment.x,
                strike_y,
                fragment.x + fragment.width,
                strike_y + style.scale().max(1.0),
                color,
            );
        }
    }
}

fn draw_line_selection(
    ctx: &mut dyn DrawCtx,
    lines: &[RichLayoutLine],
    origin_y: f64,
    start: usize,
    end: usize,
    color: Color,
) {
    if start >= end {
        return;
    }

    for line in lines {
        // The offset a line wraps at is the next line's start, so a range
        // ending there stops on this line.
        let line_end = if line.is_last { usize::MAX } else { line.end };
        if end <= line.start || start > line_end {
            continue;
        }

        let left = if start <= line.start {
            line.left
        } else {
            stop_x(line, start, false).unwrap_or(line.left)
        };
        let right = if end >= line_end && !line.is_last {
            (line.left + line.width).max(stop_x(line, line.end, true).unwrap_or(0.0))
        } else {
            stop_x(line, end, true).unwrap_or(line.left + line.width)
        };
        if right > left {
            fill_lbrt(
                ctx,
                left,
                origin_y + line.bottom,
                right,
                origin_y + line.top,
                color,
            );
        }
    }
}

// The x of the caret stop at `offset`, preferring the one whose line-end flag
// is `prefer_line_end`.
fn stop_x(line: &RichLayoutLine, offset: usize, prefer_line_end: bool) -> Option<f64> {
    let mut plain = None;
    for stop in line.caret_stops.iter().filter(|s| s.caret.offset == offset) {
        if stop.caret.at_line_end == prefer_line_end {
            return Some(stop.x);
        }
        plain = plain.or(Some(stop.x));
    }
    plain
}

fn draw_image(
    ctx: &mut dyn DrawCtx,
    image: Option<RichImage>,
    rect: Rect,
    colors: &RichEditColors,
) {
    match image {
        Some(image) if image.width > 0 && image.height > 0 => ctx.draw_image_rgba_arc(
            &image.data,
            image.width,
            image.height,
            rect.x,
            rect.y,
            rect.width,
            rect.height,
        ),
        // Not loaded (yet): a shaded box keeps the space visible so a caret
        // beside it makes sense.
        _ => fill_rect(ctx, rect, colors.code_background),
    }
}

// C# `new TypeFacePrinter(text, face, (x, baseline)).Render(...)`: the face's
// underline (a link) is a bar under the text's whole advance.
pub(crate) fn draw_text(
    ctx: &mut dyn DrawCtx,
    text: &str,
    face: &RichFace,
    x: f64,
    baseline: f64,
    color: Color,
) {
    ctx.set_font(Arc::clone(face.font()));
    ctx.set_font_size(face.em_px());
    ctx.set_fill_color(color);
    ctx.fill_text(text, x, baseline);
    if face.underline() {
        let (position, thickness) = face.underline_metrics_px();
        if thickness > 0.0 {
            let bottom = baseline + position;
            fill_lbrt(
                ctx,
                x,
                bottom,
                x + face.measure(text),
                bottom + thickness,
                color,
            );
        }
    }
}
