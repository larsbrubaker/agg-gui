//! Read-only introspection and programmatic selection for [`MarkdownView`]:
//! the plain text the view lays out (MarkdigAgg `MarkdownWidget.PlainText`),
//! `SetSelection` over it, and what C#'s tests learn by walking the child
//! widgets MarkdigAgg builds (`QuoteBlockX`, `CodeBlockX`, `AggTable` /
//! `AggTableRow` cells, `ImageLinkSimpleX`): the laid-out blocks in order,
//! each table's cells with their widths, and each image's laid-out width.
//!
//! The view draws its layout items itself rather than building child
//! widgets, so these answer from the items of the last `layout()` pass
//! (`layout.rs`) and the selectable text model (`selection.rs`).

use super::cell_images::images_of;
use super::table_style;
use super::table_style::cell_content_x;
use super::{LayoutItem, LineRun, MarkdownAlign, MarkdownView};
use crate::color::Color;

/// A block-level element of the laid-out document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkdownBlockKind {
    /// A run of consecutive quoted rows (C#'s `QuoteBlockX`).
    Quote,
    /// A fenced or indented code block (C#'s `CodeBlockX`).
    Code,
    /// A pipe or grid table (C#'s `AggTable`).
    Table,
    /// A horizontal rule (C#'s `HorizontalLine`).
    Rule,
}

/// One laid-out table cell.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkdownTableCell {
    pub row: usize,
    pub column: usize,
    /// The cell's text as laid out (without markdown syntax).
    pub text: String,
    /// The cell's laid-out width: its column's width (C#'s `cell.Width`).
    pub width: f64,
    /// How much of that width shows inside the table's viewport at its
    /// current horizontal scroll (0 for a cell scrolled out of view).
    pub visible_width: f64,
    /// The laid-out width of each image in the cell, in order (C#'s
    /// `ImageLinkSimpleX.Width` of the cell's images).
    pub image_widths: Vec<f64>,
    /// Whether the cell is in the header row (C#'s `AggTableRow.IsHeadingRow`).
    pub header: bool,
    /// The cell's alignment, from its column's `:---:` marker (C#'s
    /// `AggTableCell.FlowHAnchor`).
    pub align: MarkdownAlign,
    /// Where the cell's content starts, from the cell's left edge.
    pub content_x: f64,
    /// The width of the cell's text and images (C#'s `cell.ContentWidth`).
    pub content_width: f64,
    /// The stripe behind the cell's row (C#'s `AggTableRow.StripeColor`),
    /// in the current theme.
    pub stripe: Color,
    /// The url of each link in the cell's text, in order (C#'s `TextLinkX`
    /// children of the cell).
    pub links: Vec<String>,
    /// The cell's box between its grid lines in the view's Y-up
    /// coordinates, at the table's current horizontal scroll (C#'s
    /// `cell.TransformToParentSpace(..., cell.LocalBounds)`).
    pub bounds: crate::geometry::Rect,
}

/// One laid-out paragraph, heading or list item (C#'s `ParagraphX` /
/// `HeadingRowX`), all its wrapped rows together.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkdownTextBlock {
    /// The heading level (1-6), 0 for a paragraph (C#'s `HeadingRowX.Level`).
    pub level: u8,
    /// How its rows sit across the view (C#'s `ContentHAnchor`).
    pub align: MarkdownAlign,
    /// Its words in order, each with where it starts.
    pub words: Vec<MarkdownWord>,
    /// The size its text draws at (C#'s `TextWidget.PointSize`).
    pub font_size: f64,
    /// How far its rows sit in from the view's padding (a list's nesting,
    /// a quote's bar; C#'s `ListX.Margin.Left`).
    pub indent: f64,
    /// Whether it is inside a block quote (C#'s `QuoteBlockX`).
    pub quote: bool,
}

/// A laid-out word of a [`MarkdownTextBlock`].
#[derive(Clone, Debug, PartialEq)]
pub struct MarkdownWord {
    pub text: String,
    /// The word's left edge, from the view's left edge (C#'s
    /// `TransformToParentSpace(root, Vector2.Zero).X` of its `TextWidget`).
    pub x: f64,
}

impl MarkdownView {
    /// The paragraphs, headings and list items of the last layout, in
    /// document order (table cells and code blocks are not among them).
    pub fn text_blocks(&self) -> Vec<MarkdownTextBlock> {
        let mut blocks: Vec<MarkdownTextBlock> = Vec::new();
        for item in &self.items {
            let LayoutItem::Line {
                runs,
                style,
                indent,
                continues,
                align,
                level,
                quote,
                ..
            } = item
            else {
                continue;
            };
            let mut words = Vec::new();
            for run in runs {
                let LineRun::Text {
                    text,
                    style: run_style,
                    lead,
                    x,
                    ..
                } = run
                else {
                    continue;
                };
                for (offset, word) in super::matching::words_with_offsets(text) {
                    let before = self.styled_text_width(&text[..offset], *style, *run_style);
                    words.push(MarkdownWord {
                        text: word.to_string(),
                        x: self.padding + indent + x + lead + before,
                    });
                }
            }
            if words.is_empty() {
                continue;
            }
            match blocks.last_mut() {
                Some(block) if *continues => block.words.extend(words),
                _ => blocks.push(MarkdownTextBlock {
                    level: *level,
                    align: *align,
                    words,
                    font_size: style.font_size(self.font_size),
                    indent: *indent,
                    quote: *quote,
                }),
            }
        }
        blocks
    }

    /// The laid-out document as plain text (MarkdigAgg's `PlainText`): the
    /// rendered words without markdown syntax, a space where a row wraps, a
    /// newline between blocks, tabs between table cells. The offsets
    /// [`set_selection`](Self::set_selection) takes index into it. Reflects
    /// the last layout.
    pub fn plain_text(&self) -> &str {
        self.laid_out_text()
    }

    /// Select bytes `start..end` of [`plain_text`](Self::plain_text)
    /// (MarkdigAgg's `SetSelection`), as a drag across them would; Ctrl+C
    /// then copies them. The ends may come in either order. A selection set
    /// before the first layout is kept and clamped to the text then.
    pub fn set_selection(&mut self, start: usize, end: usize) {
        self.selection_anchor = Some(start);
        self.selection_cursor = Some(end);
        if !self.selectable_text.is_empty() {
            self.clamp_selection_to_text();
        }
        crate::animation::request_draw();
    }

    /// The selected bytes of [`plain_text`](Self::plain_text), sorted, or
    /// `None` when nothing is selected.
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.selection_range().map(|r| (r.start, r.end))
    }

    /// The selected plain text (empty when nothing is selected).
    pub fn selected_text(&self) -> &str {
        match self.selection_range() {
            Some(r) => self.selectable_text.get(r).unwrap_or(""),
            None => "",
        }
    }

    /// The quote, code and table blocks of the last layout, in document
    /// order.
    pub fn blocks(&self) -> Vec<MarkdownBlockKind> {
        let mut blocks = Vec::new();
        let mut in_quote = false;
        for item in &self.items {
            match item {
                LayoutItem::Line { quote, style, .. } => {
                    if matches!(style, super::LineStyle::Rule) {
                        blocks.push(MarkdownBlockKind::Rule);
                    }
                    if *quote && !in_quote {
                        blocks.push(MarkdownBlockKind::Quote);
                    }
                    in_quote = *quote;
                }
                LayoutItem::CodeBlock { .. } => {
                    in_quote = false;
                    blocks.push(MarkdownBlockKind::Code);
                }
                LayoutItem::Table { .. } => {
                    in_quote = false;
                    blocks.push(MarkdownBlockKind::Table);
                }
            }
        }
        blocks
    }

    /// Each code block of the last layout, in document order, as its lines
    /// (C#'s `CodeBlockX` text widgets), leading spaces kept.
    pub fn code_block_lines(&self) -> Vec<Vec<String>> {
        self.items
            .iter()
            .filter_map(|item| match item {
                LayoutItem::CodeBlock { lines, .. } => Some(lines.clone()),
                _ => None,
            })
            .collect()
    }

    /// The height of one line of each code block of the last layout, in
    /// document order (C#'s `CodeBlockX` `TextWidget.Height`).
    pub fn code_block_line_heights(&self) -> Vec<f64> {
        self.items
            .iter()
            .filter_map(|item| match item {
                LayoutItem::CodeBlock { line_h, .. } => Some(*line_h),
                _ => None,
            })
            .collect()
    }

    /// The size code block text draws at (C#'s `CodeBlockX` point size).
    pub fn code_font_size(&self) -> f64 {
        super::LineStyle::Code.font_size(self.font_size)
    }

    /// Every table of the last layout, in document order, each as its cells
    /// row by row.
    pub fn table_cells(&self) -> Vec<Vec<MarkdownTableCell>> {
        let mut tables = Vec::new();
        for item in &self.items {
            let LayoutItem::Table {
                block_idx,
                rows,
                cell_styles,
                cell_images,
                header,
                aligns,
                y,
                row_heights,
                col_widths,
                line,
                viewport_width,
                content_width,
                ..
            } = item
            else {
                continue;
            };
            let text_color = self.resolved_colors(&crate::theme::current_visuals()).text;
            let scroll_x = self.block_scroll_offset(*block_idx);
            let grid = super::table_grid::TableGrid {
                col_widths,
                row_heights,
                line: *line,
            };
            let scrollbar_h = if content_width > viewport_width {
                super::BLOCK_SCROLLBAR_H + super::BLOCK_SCROLLBAR_GAP
            } else {
                0.0
            };
            let mut cells = Vec::new();
            for (ri, row) in rows.iter().enumerate() {
                for (ci, width) in col_widths.iter().enumerate() {
                    let x = grid.cell_left(ci) - scroll_x;
                    let visible = (x + width).min(*viewport_width) - x.max(0.0);
                    let text = row.get(ci).map_or("", String::as_str);
                    let images = images_of(cell_images, ri, ci);
                    let spans = super::emphasis::cell_spans(cell_styles, ri, ci);
                    let content_width = self.cell_content_width(text, spans, images);
                    let align = aligns.get(ci).copied().unwrap_or_default();
                    cells.push(MarkdownTableCell {
                        row: ri,
                        column: ci,
                        text: row.get(ci).cloned().unwrap_or_default(),
                        width: *width,
                        visible_width: visible.max(0.0),
                        image_widths: images.iter().map(|image| image.width).collect(),
                        header: *header && ri == 0,
                        align,
                        content_x: cell_content_x(*width, content_width, align),
                        content_width,
                        stripe: table_style::stripe_color(text_color, ri, *header),
                        links: super::cell_links::cell_link_urls(spans),
                        bounds: crate::geometry::Rect::new(
                            self.padding + x,
                            y + scrollbar_h + grid.row_bottom(ri),
                            *width,
                            row_heights.get(ri).copied().unwrap_or(0.0),
                        ),
                    });
                }
            }
            tables.push(cells);
        }
        tables
    }

    /// Each table's laid-out size, in document order: it stretches across
    /// the view's content width (C#'s `AggTable.HAnchor = Stretch`), its
    /// columns scrolling inside when wider, and is as tall as its rows.
    pub fn table_sizes(&self) -> Vec<crate::geometry::Size> {
        self.items
            .iter()
            .filter_map(|item| match item {
                LayoutItem::Table {
                    viewport_width,
                    height,
                    ..
                } => Some(crate::geometry::Size::new(*viewport_width, *height)),
                _ => None,
            })
            .collect()
    }

    /// Each table's box in the view's Y-up coordinates, in document order:
    /// across the view's content width, as tall as its grid and its
    /// scrollbar (C#'s `table.TransformToParentSpace(..., LocalBounds)`).
    pub fn table_bounds(&self) -> Vec<crate::geometry::Rect> {
        self.items
            .iter()
            .filter_map(|item| match item {
                LayoutItem::Table {
                    y,
                    viewport_width,
                    height,
                    ..
                } => Some(crate::geometry::Rect::new(
                    self.padding,
                    *y,
                    *viewport_width,
                    *height,
                )),
                _ => None,
            })
            .collect()
    }

    /// Each table's row tops, top row first, in the view's Y-up
    /// coordinates (C#'s `row.TransformToParentSpace(...).Top`): the top
    /// of each row is the bottom of the grid line above it.
    pub fn table_row_tops(&self) -> Vec<Vec<f64>> {
        self.items
            .iter()
            .filter_map(|item| match item {
                LayoutItem::Table {
                    y,
                    row_heights,
                    col_widths,
                    line,
                    viewport_width,
                    content_width,
                    ..
                } => {
                    let scrollbar_h = if content_width > viewport_width {
                        super::BLOCK_SCROLLBAR_H + super::BLOCK_SCROLLBAR_GAP
                    } else {
                        0.0
                    };
                    let grid = super::table_grid::TableGrid {
                        col_widths,
                        row_heights,
                        line: *line,
                    };
                    Some(
                        (0..row_heights.len())
                            .map(|ri| y + scrollbar_h + grid.row_top(ri))
                            .collect(),
                    )
                }
                _ => None,
            })
            .collect()
    }

    /// The colour the tables' grid lines draw in, in the current theme
    /// (C#'s `AggTable.GridColor`).
    pub fn table_grid_color(&self) -> Color {
        table_style::grid_color(self.resolved_colors(&crate::theme::current_visuals()).text)
    }

    /// The laid-out width of every image of the last layout, in document
    /// order (a loaded image scaled to fit, or its placeholder), table cells'
    /// images included, row by row.
    pub fn image_widths(&self) -> Vec<f64> {
        let mut widths = Vec::new();
        for item in &self.items {
            match item {
                LayoutItem::Line { runs, .. } => {
                    for run in runs {
                        if let LineRun::Image { width, .. } = run {
                            widths.push(*width);
                        }
                    }
                }
                LayoutItem::Table { cell_images, .. } => {
                    for image in cell_images.iter().flatten().flatten() {
                        widths.push(image.width);
                    }
                }
                LayoutItem::CodeBlock { .. } => {}
            }
        }
        widths
    }
}
