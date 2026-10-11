//! How `MarkdownView` styles a table, from MarkdigAgg's
//! `Tables/AggTableRenderer.cs`: the grid in the theme's text colour at
//! alpha 150 (`TableBorderAlpha`), and every even-indexed body row striped in
//! the text colour at alpha 24 (`ZebraStripeAlpha`, the rich editor's shade
//! alpha). The header row is never striped; its text is bold instead
//! (`header_bold`), and each cell's content sits where its column's `:---:`
//! marker says, the same inset in from either edge (`cell_content_x`).
//! `layout.rs` sizes with these, `paint.rs` and `selection.rs` place cell
//! text with them, and `introspect.rs` reports them.

use crate::color::Color;
use crate::theme::Visuals;

use super::{emphasis, MarkdownAlign, MarkdownRunStyle};

/// C# `TableBorderAlpha`.
const TABLE_BORDER_ALPHA: f32 = 150.0 / 255.0;
/// C# `ZebraStripeAlpha`.
const ZEBRA_STRIPE_ALPHA: f32 = 24.0 / 255.0;

/// The colour the table grid's lines draw in (C# `AggTable.GridColor`).
pub(super) fn grid_color(visuals: &Visuals) -> Color {
    visuals.text_color.with_alpha(TABLE_BORDER_ALPHA)
}

/// The stripe behind row `row` (C# `AggTableRow.StripeColor`), transparent
/// for the header row and every odd-indexed row.
pub(super) fn stripe_color(visuals: &Visuals, row: usize, header: bool) -> Color {
    let is_header = header && row == 0;
    if !is_header && row % 2 == 0 {
        visuals.text_color.with_alpha(ZEBRA_STRIPE_ALPHA)
    } else {
        Color::transparent()
    }
}

/// The gap either side of a table cell's content (C#'s `AggTableColumn`
/// pads each cell, and its row insets each one, by the same amount on both
/// sides, so a left- and a right-aligned cell sit the same distance in).
pub(super) const CELL_INSET: f64 = 12.0;

/// Where a cell's content of width `content_w` starts in its column of
/// width `col_w`, aligned as its column's `:---:` marker says (C#'s
/// `AggTableCell.FlowHAnchor`). A column is always at least its widest
/// content plus both insets (`layout_table`), so every alignment keeps the
/// inset clear.
pub(super) fn cell_content_x(col_w: f64, content_w: f64, align: MarkdownAlign) -> f64 {
    match align {
        MarkdownAlign::Left => CELL_INSET,
        MarkdownAlign::Center => (col_w - content_w) * 0.5,
        MarkdownAlign::Right => col_w - CELL_INSET - content_w,
    }
}

/// The cell styles with every header cell's text bold, as C#'s
/// `AggTableRow` sets `Bold` on each text widget of a heading row.
pub(super) fn header_bold(
    rows: &[Vec<String>],
    cell_styles: &emphasis::CellStyles,
    header: bool,
) -> emphasis::CellStyles {
    let mut styles = cell_styles.clone();
    let (true, Some(first)) = (header, rows.first()) else {
        return styles;
    };
    let bold_row = first
        .iter()
        .enumerate()
        .map(|(ci, text)| {
            emphasis::cell_pieces(text, emphasis::cell_spans(cell_styles, 0, ci))
                .into_iter()
                .map(|(range, style)| emphasis::CellSpan {
                    start: range.start,
                    end: range.end,
                    style: MarkdownRunStyle {
                        bold: true,
                        ..style
                    },
                })
                .collect()
        })
        .collect();
    match styles.first_mut() {
        Some(row) => *row = bold_row,
        None => styles.push(bold_row),
    }
    styles
}
