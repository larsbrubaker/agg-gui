//! [`RichEditError`], what an edit operation returns instead of panicking on a
//! caller's mistake, and [`check_position`], the one check every operation
//! runs on the positions it is given.
//!
//! Where agg-sharp's edit operations (`RichEditOperations.cs`,
//! `RichTableCodeOperations.cs`, `RichBlockOperations.cs`, ...) throw — an
//! indexer's `ArgumentOutOfRangeException` on a position the document does
//! not have, `InvalidOperationException` for a table operation on a non-table
//! block, `ArgumentException` for a code/table range across two blocks, a history
//! asked to keep no steps (`RichEditHistory.cs`) — the
//! Rust operations return one of these variants and leave the document
//! unchanged.  Library code never panics on bad input.

use std::fmt;

use super::block::RichBlockKind;
use super::document::{DocPosition, RichDocument};

/// Why an edit operation refused its input.  The document is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RichEditError {
    /// A position the document does not have: no such block, no such table
    /// cell, or an offset past the end of its block or cell (C#'s indexers'
    /// `ArgumentOutOfRangeException`).
    PositionOutOfRange(DocPosition),
    /// A table operation on a block that is not a table (C#'s
    /// `InvalidOperationException`), naming the block's kind.
    NotATable(RichBlockKind),
    /// A code-block or table range whose ends are in different blocks (C#'s
    /// `ArgumentException`); `edit_operations::delete_range` handles ranges
    /// across blocks.
    RangeSpansBlocks,
    /// A heading level other than 0 (Normal) or 1-6 (C#'s
    /// `ArgumentOutOfRangeException` from `SetBlockKind`).
    HeadingLevelOutOfRange(u8),
    /// A [`super::edit_history::RichEditHistory`] asked to keep no steps
    /// (C#'s `ArgumentOutOfRangeException` from its constructor).
    MaxStepsOutOfRange,
}

impl fmt::Display for RichEditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PositionOutOfRange(position) => {
                write!(f, "{position:?} is not a position in the document")
            }
            Self::NotATable(kind) => write!(f, "A {kind:?} block is not a table."),
            Self::RangeSpansBlocks => f.write_str(
                "Both positions must be in one block; edit_operations::delete_range handles ranges across blocks.",
            ),
            Self::HeadingLevelOutOfRange(level) => {
                write!(f, "heading level {level}: 0 (Normal) or a heading level 1-6.")
            }
            Self::MaxStepsOutOfRange => f.write_str("The history must keep at least one step."),
        }
    }
}

impl std::error::Error for RichEditError {}

/// Checks that `position` is a caret position the document has: a block that
/// exists and an offset within its text (a table position also needs its row
/// and column to name a cell; outside a table row and column are ignored, as
/// C# ignores them).  An empty document has exactly one position, the start
/// of block 0, where the operations add a paragraph to type into.
pub fn check_position(document: &RichDocument, position: DocPosition) -> Result<(), RichEditError> {
    let error = Err(RichEditError::PositionOutOfRange(position));
    let Some(block) = document.blocks.get(position.block_index) else {
        let start_of_empty =
            document.blocks.is_empty() && position.block_index == 0 && position.offset == 0;
        return if start_of_empty { Ok(()) } else { error };
    };

    if block.kind == RichBlockKind::Table {
        let has_cell = block
            .table_rows
            .get(position.row)
            .is_some_and(|row| position.column < row.len());
        if !has_cell {
            return error;
        }
    }

    if position.offset > block.text_length(position.row, position.column) {
        return error;
    }

    Ok(())
}

/// [`check_position`] for both ends of a range.
pub fn check_range(
    document: &RichDocument,
    a: DocPosition,
    b: DocPosition,
) -> Result<(), RichEditError> {
    check_position(document, a)?;
    check_position(document, b)
}

/// [`check_position`] for an operation that needs a block to exist (a block
/// operation on block `index`), as a position at its start.
pub fn check_block(document: &RichDocument, index: usize) -> Result<(), RichEditError> {
    if index < document.blocks.len() {
        Ok(())
    } else {
        Err(RichEditError::PositionOutOfRange(DocPosition::new(
            index, 0,
        )))
    }
}
