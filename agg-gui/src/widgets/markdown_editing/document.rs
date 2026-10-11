//! The rich editor's model of a markdown file, [`RichDocument`], and caret
//! positions in it, [`DocPosition`].  Ports agg-sharp's `RichDocument.cs`.
//!
//! Writing a document back is the frontmatter, then for each block its
//! `separator_before` and its markdown (`original_source` unless dirty), then
//! `trailing_text` — so a document nobody edited round-trips byte-identical.
//! Blocks live in [`super::block`]; the groups several blocks share in
//! [`super::groups`].

use std::cmp::Ordering;

use super::block::RichBlock;
use super::groups::GroupRemap;

/// The rich editor's model of a markdown file.
#[derive(Debug, Default)]
pub struct RichDocument {
    /// YAML frontmatter (with its `---` fences and trailing newline) the editor
    /// hides and never changes, or "".
    pub frontmatter: String,
    /// The document's blocks, in order.
    pub blocks: Vec<RichBlock>,
    /// Whatever follows the last block (usually the final newline).  Like
    /// [`RichBlock::separator_before`] it holds only blank/whitespace text,
    /// never an alignment wrapper's closing `</div>`.
    pub trailing_text: String,
}

impl RichDocument {
    /// An empty document.
    pub fn new() -> Self {
        Self::default()
    }

    /// The largest caret offset at a position's block (and cell, for a table).
    pub fn text_length(&self, position: DocPosition) -> usize {
        self.blocks[position.block_index].text_length(position.row, position.column)
    }

    /// A deep copy sharing no mutable state, so an undo snapshot is unaffected
    /// by later edits (C#'s `RichDocument.Clone`).
    ///
    /// Each group is cloned once and shared among the copied members, so the
    /// copy's blocks group exactly as the original's do while editing one
    /// document's group never touches the other's.
    pub fn deep_clone(&self) -> RichDocument {
        let mut align = GroupRemap::new();
        let mut quote = GroupRemap::new();
        let mut list = GroupRemap::new();
        let blocks = self
            .blocks
            .iter()
            .map(|block| {
                let mut copy = block.clone();
                copy.align_group = align.remap(&block.align_group);
                copy.quote_group = quote.remap(&block.quote_group);
                copy.list_group = list.remap(&block.list_group);
                copy
            })
            .collect();
        RichDocument {
            frontmatter: self.frontmatter.clone(),
            blocks,
            trailing_text: self.trailing_text.clone(),
        }
    }
}

/// A caret position: a block, and a plain-text offset within it (see
/// [`super::inlines`]).  In a Table the offset is within the cell at `row`,
/// `column`; elsewhere both are 0.  Positions order in reading order: block,
/// then row, then column, then offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DocPosition {
    /// The block's index in [`RichDocument::blocks`].
    pub block_index: usize,
    /// The plain-text offset within the block (or cell).
    pub offset: usize,
    /// The table row, 0 outside a table.
    pub row: usize,
    /// The table column, 0 outside a table.
    pub column: usize,
}

impl DocPosition {
    /// A position outside a table (C#'s `new DocPosition(block, offset)`).
    pub fn new(block_index: usize, offset: usize) -> Self {
        Self {
            block_index,
            offset,
            row: 0,
            column: 0,
        }
    }

    /// A position in a table cell.
    pub fn in_cell(block_index: usize, offset: usize, row: usize, column: usize) -> Self {
        Self {
            block_index,
            offset,
            row,
            column,
        }
    }
}

impl Ord for DocPosition {
    fn cmp(&self, other: &Self) -> Ordering {
        self.block_index
            .cmp(&other.block_index)
            .then(self.row.cmp(&other.row))
            .then(self.column.cmp(&other.column))
            .then(self.offset.cmp(&other.offset))
    }
}

impl PartialOrd for DocPosition {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
