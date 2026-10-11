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
use super::{LayoutItem, LineRun, MarkdownAlign, MarkdownView};

/// A block-level element of the laid-out document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkdownBlockKind {
    /// A run of consecutive quoted rows (C#'s `QuoteBlockX`).
    Quote,
    /// A fenced or indented code block (C#'s `CodeBlockX`).
    Code,
    /// A pipe or grid table (C#'s `AggTable`).
    Table,
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
                LayoutItem::Line { quote, .. } => {
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

    /// Every table of the last layout, in document order, each as its cells
    /// row by row.
    pub fn table_cells(&self) -> Vec<Vec<MarkdownTableCell>> {
        let mut tables = Vec::new();
        for item in &self.items {
            let LayoutItem::Table {
                block_idx,
                rows,
                cell_images,
                col_widths,
                viewport_width,
                ..
            } = item
            else {
                continue;
            };
            let scroll_x = self.block_scroll_offset(*block_idx);
            let mut cells = Vec::new();
            for (ri, row) in rows.iter().enumerate() {
                let mut x = -scroll_x;
                for (ci, width) in col_widths.iter().enumerate() {
                    let visible = (x + width).min(*viewport_width) - x.max(0.0);
                    cells.push(MarkdownTableCell {
                        row: ri,
                        column: ci,
                        text: row.get(ci).cloned().unwrap_or_default(),
                        width: *width,
                        visible_width: visible.max(0.0),
                        image_widths: images_of(cell_images, ri, ci)
                            .iter()
                            .map(|image| image.width)
                            .collect(),
                    });
                    x += width;
                }
            }
            tables.push(cells);
        }
        tables
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
