//! `markdown_editing` — the rich Markdown editor's document model, a port of
//! agg-sharp's `Markdig.Agg.Editing` (`MarkdigAgg/Editing/`).
//!
//! The editor reads Markdown into a [`RichDocument`] (blocks of styled inlines
//! that partition the source exactly), edits that model, and writes Markdown
//! back: unedited blocks verbatim, edited (`dirty`) blocks regenerated.  It is
//! a separate model from [`super::rich_text`]'s `RichDoc`: this one keeps the
//! Markdown source structure (headings, list groups, quotes, code blocks,
//! tables, `<div align>` wrappers) a byte-identical round trip needs.
//!
//! # Module layout
//!
//! * [`inline`] — [`RichInline`]: a styled run of text or an atom the caret
//!   steps over whole (`RichInline.cs`).
//! * [`inlines`] — offset arithmetic over a list of inlines (`RichInlines.cs`).
//! * [`block`] — [`RichBlock`] and its kinds, list info and table cells
//!   (`RichBlock.cs`).
//! * [`groups`] — the shared `<div align>` / blockquote / list groups
//!   (`RichBlockGroups.cs`).
//! * [`document`] — [`RichDocument`] and [`DocPosition`] (`RichDocument.cs`).
//! * [`markdown_parser`] — markdown into the document, its blocks partitioning
//!   the source exactly (`RichMarkdownParser.cs`), on pulldown-cmark's source
//!   ranges (`parse_tree`); `parser_inlines` reads inlines,
//!   `code_and_tables` code blocks and pipe tables (`RichCodeAndTables.cs`),
//!   `align_groups` folds `<div align>` wrappers (`RichAlignGroups.cs`), and
//!   `pipe_tables` finds pipe tables as Markdig does (the viewer uses it too).
//! * [`markdown_writer`] — the document back to markdown: unedited blocks and
//!   groups verbatim, edited ones regenerated (`RichMarkdownWriter.cs`).
//! * [`group_writer`] — changed lists, quotes and `<div align>` wrappers
//!   rebuilt whole (`RichGroupWriter.cs`).
//! * [`inline_writer`] — a list of inlines as markdown (`RichInlineWriter.cs`).
//! * `escaping` — escaped text, code spans and link tails
//!   (`RichMarkdownEscaping.cs`).
//! * [`selection`] — [`RichSelection`], what an edit returns to show next
//!   (`RichSelection.cs`).
//! * [`edit_operations`] — typing, range deletes, Enter, Backspace and Delete
//!   (`RichEditOperations.cs`); [`table_code_operations`] the same inside a
//!   code block or table, plus the table's row/column operations
//!   (`RichTableCodeOperations.cs`); `group_repair` restores the group
//!   invariants after an edit (`RichGroupRepair.cs`).  Where C# throws on a
//!   caller's mistake the operations return a [`RichEditError`]
//!   ([`edit_error`]).
//! * [`style_operations`] — bold, italic, code, strike and links over a
//!   selection, and the style the toolbar shows (`RichStyleOperations.cs`),
//!   with the style types in [`inline_style`].
//! * [`block_operations`] — Normal/Heading, lists, quotes, alignment,
//!   indent/outdent and the line-start shortcuts (`RichBlockOperations.cs`);
//!   `block_containers` turns blocks into list items or quotes and joins the
//!   groups they touch.
//! * `char_class` — .NET's character classes, as the writer sees them.
//!
//! # Offsets
//!
//! A caret offset counts each run character (a Rust `char`, a Unicode scalar
//! value) and each atom as one.  agg-sharp counts UTF-16 code units, so a
//! character outside the Basic Multilingual Plane (most emoji) is one caret
//! position here and two there; the two agree on all BMP text.

mod align_groups;
pub mod block;
mod block_containers;
pub mod block_operations;
mod char_class;
mod code_and_tables;
pub mod document;
pub mod edit_error;
pub mod edit_operations;
mod escaping;
mod group_repair;
pub mod group_writer;
pub mod groups;
pub mod inline;
pub mod inline_style;
pub mod inline_writer;
pub mod inlines;
pub mod markdown_parser;
pub mod markdown_writer;
mod parse_tree;
pub(crate) mod parser_inlines;
pub(crate) mod pipe_tables;
pub mod selection;
pub mod style_operations;
pub mod table_code_operations;

pub use block::{RichAlignment, RichBlock, RichBlockKind, RichListInfo, RichTableCell};
pub use block_operations::{RichBlockState, RichListKind};
pub use document::{DocPosition, RichDocument};
pub use edit_error::RichEditError;
pub use groups::shared as shared_group;
pub use groups::{
    RichAlignGroup, RichAlignGroupRef, RichListGroup, RichListGroupRef, RichQuoteGroup,
    RichQuoteGroupRef,
};
pub use inline::{InlineAtom, InlineAtomKind, RichInline, RichInlineContent, RichRun};
pub use inline_style::{RichInlineStyle, RichPendingStyle, RichStyleCoverage, RichStyleState};
pub use inline_writer::RichInlineContext;
pub use inlines::OffsetOutOfRange;
pub use selection::RichSelection;
