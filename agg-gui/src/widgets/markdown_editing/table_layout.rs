//! A Table block laid out to a width — a port of agg-sharp's
//! `RichTableLayout.cs` (`RichTableLayout` and `RichTableCellLayout`).
//!
//! Drawn like the markdown viewer's table: a full grid of thin lines, cells
//! padded inside it, the header row bold and every other body row striped.
//! Each column is as wide as its widest unwrapped cell; when the table is
//! wider than the block, columns shrink in proportion to how much they can
//! give (none below [`RichLayoutStyle::table_min_column_text_width`]) and
//! their cells wrap.  A row is as tall as its tallest cell.  Coordinates
//! follow [`super::block_layout`]: y up, origin at the bottom-left of the
//! block's box with the block spacing included, so the widget stacks a table
//! by its height like any block.
//!
//! Each cell's lines are built by the paragraph line helpers in
//! [`super::block_layout`] (`stack_lines`, `flip_lines`, `hit_test_lines`,
//! `find_stop`), so a cell lays out exactly as a paragraph at its column's
//! text width.  The grid is snapped to whole device pixels with
//! [`crate::grid_pixels`].
//!
//! Where C# throws (a non-Table block, a cell outside the table, an offset
//! outside a cell) these return a [`RichLayoutError`].  C# bug not ported:
//! `HitTest` indexed `Rows[row][-1]` for a row with no cells; here a point in
//! such a row goes to the row's offset 0 (pinned by a `rust_only_*` test).

use crate::geometry::{Point, Rect};

use super::block::{RichAlignment, RichBlock, RichBlockKind};
use super::block_layout::{
    caret_box, find_stop, flip_lines, hit_test_lines, rect_lbrt, stack_lines, LineSource,
    RichBlockGeometry, RichLayoutError,
};
use super::layout_items::{self, LayoutItem, LayoutItemKind};
use super::layout_line::{RichCaret, RichLayoutLine};
use super::layout_style::RichLayoutStyle;

/// One laid-out table cell (C# `RichTableCellLayout`): its box inside the
/// grid lines and its text lines, laid out exactly as a paragraph's at the
/// column's text width.  Coordinates are the owning [`RichTableLayout`]'s.
/// Every caret stop on the lines carries this cell's row and column.
#[derive(Clone, Debug)]
pub struct RichTableCellLayout {
    /// The cell's row, header row 0.
    pub row: usize,
    /// The cell's column.
    pub column: usize,
    /// The cell's box between its grid lines, padding included; the row's
    /// height, not just the text's.
    pub rect: Rect,
    /// The text lines, top to bottom.
    pub lines: Vec<RichLayoutLine>,
    /// The cell's largest caret offset, in chars.
    pub length: usize,
}

impl RichTableCellLayout {
    /// The caret in this cell nearest `point`, wherever the point is: a click
    /// in the padding above or below the text goes to the first or last line,
    /// one left or right of it to that line's ends.
    pub fn hit_test(&self, point: Point) -> RichCaret {
        RichCaret {
            row: self.row,
            column: self.column,
            ..hit_test_lines(&self.lines, point)
        }
    }

    /// The caret box for `offset` in this cell (see
    /// [`super::RichBlockLayout::caret_rect_at`]).
    pub fn caret_rect_at(
        &self,
        offset: usize,
        at_line_end: bool,
        caret_width: f64,
    ) -> Result<Rect, RichLayoutError> {
        let (line, stop) = find_stop(&self.lines, self.length, offset, at_line_end)?;
        Ok(caret_box(line, stop, caret_width))
    }
}

/// The grid as [`RichTableLayout::snap_grid`] snapped it, in drawing
/// coordinates: each vertical line's left edge, each rule's bottom edge (top
/// rule first) and the whole-pixel line thickness.  The painter keeps one and
/// has it refilled in place each frame rather than reallocated.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichTableGridSnap {
    /// Each vertical grid line's left edge, left to right.
    pub lefts: Vec<f64>,
    /// Each horizontal rule's bottom edge, top to bottom.
    pub bottoms: Vec<f64>,
    /// The line thickness in whole device pixels.
    pub thickness: i32,
}

/// A Table block laid out to a width (C# `RichTableLayout`).
#[derive(Clone, Debug)]
pub struct RichTableLayout {
    /// The block's whole height, including the style's spacing above and
    /// below it.
    pub height: f64,
    /// The grid's width, outer grid lines included; at most the width it was
    /// laid out to unless every column is already at its minimum.
    pub width: f64,
    /// Each column's width between its grid lines, cell padding included.
    pub column_widths: Vec<f64>,
    /// The cells by row, header row first; every row has one cell per column
    /// the block gives it.
    pub rows: Vec<Vec<RichTableCellLayout>>,
    /// The grid as laid out: a thin rectangle above every row, one below the
    /// last, and one left of every column and right of the last.  The widget
    /// paints the grid from [`Self::snap_grid`] instead, which puts each line
    /// on whole device pixels and keeps crossings from being painted twice.
    pub grid_lines: Vec<Rect>,
    /// The header row's box inside the outer grid lines, for a widget that
    /// shades it; `None` for a table with no rows.  The viewer leaves it
    /// unshaded and marks the header only by its bold text.
    pub header_background: Option<Rect>,
    /// The body rows the viewer stripes: the second, fourth, ... body rows.
    pub striped_rows: Vec<Rect>,
    /// The indices of the rows in [`Self::striped_rows`].
    pub striped_row_indices: Vec<usize>,
    grid_line_tops: Vec<f64>,
    column_lefts: Vec<f64>,
    grid_line_width: f64,
    // The left edge of every vertical grid line, left to right, and the
    // bottom edge of every rule, top to bottom, in block coordinates.
    line_lefts: Vec<f64>,
    line_bottoms: Vec<f64>,
}

impl RichTableLayout {
    /// Lays a Table block out to `width`.
    pub fn layout(
        block: &RichBlock,
        width: f64,
        style: &RichLayoutStyle,
    ) -> Result<Self, RichLayoutError> {
        if block.kind != RichBlockKind::Table {
            return Err(RichLayoutError::NotTableBlock);
        }

        let (before, after) = style.block_spacing(block, false);
        let line = style.table_grid_line_width();
        let pad_x = style.table_cell_padding_x();
        let pad_y = style.table_cell_padding_y();
        let column_count = block.table_rows.iter().map(Vec::len).max().unwrap_or(0);

        let mut layout = RichTableLayout {
            height: 0.0,
            width: 0.0,
            column_widths: Vec::new(),
            rows: Vec::new(),
            grid_lines: Vec::new(),
            header_background: None,
            striped_rows: Vec::new(),
            striped_row_indices: Vec::new(),
            grid_line_tops: Vec::new(),
            column_lefts: Vec::new(),
            grid_line_width: line,
            line_lefts: Vec::new(),
            line_bottoms: Vec::new(),
        };

        // Each cell's items are built once: they give the column's natural
        // width and are then broken into lines.
        let mut items: Vec<Vec<Vec<LayoutItem>>> = Vec::with_capacity(block.table_rows.len());
        let mut natural = vec![0.0; column_count];
        for (r, row) in block.table_rows.iter().enumerate() {
            let header = r == 0;
            let face = style.table_cell_face(header);
            let mut row_items = Vec::with_capacity(row.len());
            for (c, cell) in row.iter().enumerate() {
                let cell_items = layout_items::inline_items(
                    &cell.inlines,
                    &|run| style.table_run_face(run, header),
                    style,
                    &face,
                );
                natural[c] = f64::max(natural[c], natural_width(&cell_items));
                row_items.push(cell_items);
            }
            items.push(row_items);
        }

        let available_text =
            width - (column_count + 1) as f64 * line - column_count as f64 * 2.0 * pad_x;
        let text_widths = column_text_widths(
            &natural,
            available_text,
            style.table_min_column_text_width(),
        );

        let mut x = line;
        for &text_width in &text_widths {
            layout.column_lefts.push(x);
            layout.column_widths.push(text_width + 2.0 * pad_x);
            x += text_width + 2.0 * pad_x + line;
        }
        layout.width = if column_count == 0 { 0.0 } else { x };

        // Rows are stacked top-down first (y down from the block's top), then
        // flipped once the height is known.
        let mut row_tops = Vec::new();
        let mut row_bottoms = Vec::new();
        let mut y = before;
        for (r, row) in block.table_rows.iter().enumerate() {
            layout.grid_line_tops.push(y);
            let row_top = y + line;
            let mut row_bottom = row_top;
            let face = style.table_cell_face(r == 0);
            let mut row_layouts = Vec::with_capacity(row.len());
            for (c, cell) in row.iter().enumerate() {
                let mut cell_layout = RichTableCellLayout {
                    row: r,
                    column: c,
                    rect: Rect::default(),
                    lines: Vec::new(),
                    length: block.text_length(r, c),
                };
                let alignment = block
                    .column_alignments
                    .get(c)
                    .copied()
                    .flatten()
                    .unwrap_or(RichAlignment::Left);
                let source = LineSource {
                    inlines: &cell.inlines,
                    code_text: None,
                    alignment,
                    block_face: face.clone(),
                };
                let text_bottom = stack_lines(
                    &mut cell_layout.lines,
                    &items[r][c],
                    &source,
                    row_top + pad_y,
                    layout.column_lefts[c] + pad_x,
                    text_widths[c].max(1.0),
                    cell_layout.length,
                    true,
                    style.line_gap(),
                );
                row_bottom = f64::max(row_bottom, text_bottom + pad_y);
                row_layouts.push(cell_layout);
            }

            layout.rows.push(row_layouts);
            row_tops.push(row_top);
            row_bottoms.push(row_bottom);
            y = row_bottom;
        }

        let grid_top = before;
        let grid_bottom = if block.table_rows.is_empty() {
            before
        } else {
            y + line
        };
        if !block.table_rows.is_empty() {
            layout.grid_line_tops.push(y);
        }

        layout.height = grid_bottom + after;
        let height = layout.height;
        let flip = |down: f64| height - down;

        for (r, row) in layout.rows.iter_mut().enumerate() {
            for cell in row.iter_mut() {
                flip_lines(&mut cell.lines, height);
                mark_cell(cell);
                let left = layout.column_lefts[cell.column];
                cell.rect = rect_lbrt(
                    left,
                    flip(row_bottoms[r]),
                    left + layout.column_widths[cell.column],
                    flip(row_tops[r]),
                );
            }

            let row_box = rect_lbrt(
                line,
                flip(row_bottoms[r]),
                layout.width - line,
                flip(row_tops[r]),
            );
            if r == 0 {
                layout.header_background = Some(row_box);
            } else if r % 2 == 0 {
                // The viewer counts its header as row 0 and stripes the even
                // rows after it.
                layout.striped_rows.push(row_box);
                layout.striped_row_indices.push(r);
            }
        }

        for &top in &layout.grid_line_tops {
            layout
                .grid_lines
                .push(rect_lbrt(0.0, flip(top + line), layout.width, flip(top)));
            layout.line_bottoms.push(flip(top + line));
        }

        if !layout.rows.is_empty() {
            for &left in &layout.column_lefts {
                layout.grid_lines.push(rect_lbrt(
                    left - line,
                    flip(grid_bottom),
                    left,
                    flip(grid_top),
                ));
                layout.line_lefts.push(left - line);
            }
            layout.grid_lines.push(rect_lbrt(
                layout.width - line,
                flip(grid_bottom),
                layout.width,
                flip(grid_top),
            ));
            layout.line_lefts.push(layout.width - line);
        }

        Ok(layout)
    }

    /// The cell at `row`, `column`.
    pub fn cell(&self, row: usize, column: usize) -> Result<&RichTableCellLayout, RichLayoutError> {
        self.rows
            .get(row)
            .and_then(|cells| cells.get(column))
            .ok_or(RichLayoutError::CellOutOfRange { row, column })
    }

    /// The caret nearest `point` (block coordinates), in the cell whose box
    /// holds it.  A point on a grid line goes to the nearer of the two cells
    /// it divides (the boundary is the line's middle), and a point outside
    /// the grid to the nearest edge cell; within the cell, padding clicks go
    /// to the nearest caret.
    pub fn hit_test(&self, point: Point) -> RichCaret {
        if self.rows.is_empty() || self.column_lefts.is_empty() {
            return RichCaret::new(0);
        }

        let down = self.height - point.y;
        let mut row = 0;
        for r in 1..self.rows.len() {
            if down >= self.grid_line_tops[r] + self.grid_line_width / 2.0 {
                row = r;
            }
        }

        let mut column = 0;
        for c in 1..self.column_lefts.len() {
            if point.x >= self.column_lefts[c] - self.grid_line_width / 2.0 {
                column = c;
            }
        }

        let cells = &self.rows[row];
        match cells.len() {
            // A row with no cells (C# indexed cell -1 here) holds only its
            // start.
            0 => RichCaret {
                row,
                ..RichCaret::default()
            },
            count => cells[column.min(count - 1)].hit_test(point),
        }
    }

    /// The caret box for `offset` in the cell at `row`, `column` (see
    /// [`super::RichBlockLayout::caret_rect_at`]).
    pub fn caret_rect_at(
        &self,
        row: usize,
        column: usize,
        offset: usize,
        at_line_end: bool,
        caret_width: f64,
    ) -> Result<Rect, RichLayoutError> {
        self.cell(row, column)?
            .caret_rect_at(offset, at_line_end, caret_width)
    }

    /// [`Self::caret_rect_at`] for a [`RichCaret`], which names its cell.
    pub fn caret_rect(&self, caret: RichCaret, caret_width: f64) -> Result<Rect, RichLayoutError> {
        self.caret_rect_at(
            caret.row,
            caret.column,
            caret.offset,
            caret.at_line_end,
            caret_width,
        )
    }

    /// Snaps the grid, drawn with the block's bottom at `origin_y`, to whole
    /// device pixels under a draw transform translated by `translate_x`,
    /// `translate_y` (C# `SnapGrid`, which read the translation from the
    /// Graphics2D), refilling `snap`.  Returns false for a table with no
    /// rows, which has no grid.
    pub fn snap_grid(
        &self,
        origin_y: f64,
        translate_x: f64,
        translate_y: f64,
        snap: &mut RichTableGridSnap,
    ) -> bool {
        snap.lefts.clear();
        snap.bottoms.clear();
        if self.rows.is_empty() {
            return false;
        }

        // The width is already whole device pixels; rounding again keeps the
        // thickness an integer (C# `Math.Max(1, (int)Math.Round(width))`).
        snap.thickness = (self.grid_line_width.round() as i32).max(1);
        snap.lefts.extend(
            self.line_lefts
                .iter()
                .map(|&left| crate::grid_pixels::snap(left, translate_x)),
        );
        snap.bottoms.extend(
            self.line_bottoms
                .iter()
                .map(|&bottom| crate::grid_pixels::snap(origin_y + bottom, translate_y)),
        );
        true
    }
}

impl RichBlockGeometry for RichTableLayout {
    fn height(&self) -> f64 {
        self.height
    }

    fn hit_test(&self, point: Point) -> RichCaret {
        RichTableLayout::hit_test(self, point)
    }

    fn caret_rect(&self, caret: RichCaret, caret_width: f64) -> Result<Rect, RichLayoutError> {
        RichTableLayout::caret_rect(self, caret, caret_width)
    }
}

/// Each column's text width.  Columns get their natural width when the table
/// fits.  Otherwise each column keeps a minimum (its natural width when that
/// is smaller) and the room left over is shared in proportion to how much
/// wider than its minimum each column would like to be, so a long column
/// gives up the most and a short one keeps its text on one line.  With no
/// room past the minimums every column sits at its minimum and the table runs
/// past the width rather than squeezing text to nothing.
fn column_text_widths(natural: &[f64], available: f64, minimum_width: f64) -> Vec<f64> {
    let minimums: Vec<f64> = natural.iter().map(|&n| n.min(minimum_width)).collect();
    let natural_sum: f64 = natural.iter().sum();
    let minimum_sum: f64 = minimums.iter().sum();
    if natural_sum <= available {
        return natural.to_vec();
    }

    // natural_sum > available here, so natural_sum == minimum_sum would mean
    // available < minimum_sum and the share is 0: no division by zero.
    let share = if available > minimum_sum {
        (available - minimum_sum) / (natural_sum - minimum_sum)
    } else {
        0.0
    };
    natural
        .iter()
        .zip(&minimums)
        .map(|(&n, &m)| m + (n - m) * share)
        .collect()
}

// The width of the cell's widest hard-broken line laid out unwrapped,
// trailing spaces not counted, summed the same way line breaking sums so a
// column at this width does not wrap.
fn natural_width(items: &[LayoutItem]) -> f64 {
    let mut widest: f64 = 0.0;
    let mut x = 0.0;
    for item in items {
        if item.kind == LayoutItemKind::Break {
            x = 0.0;
            continue;
        }

        x += item.advance_at(x);
        if item.is_content() {
            widest = widest.max(x);
        }
    }
    widest
}

// Stamps the cell's row and column onto every caret stop, so a stop the
// widget reads (for up/down movement) already names its cell.
fn mark_cell(cell: &mut RichTableCellLayout) {
    for line in &mut cell.lines {
        for stop in &mut line.caret_stops {
            stop.caret.row = cell.row;
            stop.caret.column = cell.column;
        }
    }
}
