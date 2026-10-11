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
//! * [`markdown_writer`] — the document back to markdown: unedited blocks and
//!   groups verbatim, edited ones regenerated (`RichMarkdownWriter.cs`).
//! * [`group_writer`] — changed lists, quotes and `<div align>` wrappers
//!   rebuilt whole (`RichGroupWriter.cs`).
//! * [`inline_writer`] — a list of inlines as markdown (`RichInlineWriter.cs`).
//! * `escaping` — escaped text, code spans and link tails
//!   (`RichMarkdownEscaping.cs`).
//! * `edit_operations` — editing operations (`RichEditOperations.cs`).
//! * `char_class` — .NET's character classes, as the writer sees them.
//!
//! # Offsets
//!
//! A caret offset counts each run character (a Rust `char`, a Unicode scalar
//! value) and each atom as one.  agg-sharp counts UTF-16 code units, so a
//! character outside the Basic Multilingual Plane (most emoji) is one caret
//! position here and two there; the two agree on all BMP text.

pub mod block;
mod char_class;
pub mod document;
mod edit_operations;
mod escaping;
pub mod group_writer;
pub mod groups;
pub mod inline;
pub mod inline_writer;
pub mod inlines;
pub mod markdown_writer;

pub use block::{RichAlignment, RichBlock, RichBlockKind, RichListInfo, RichTableCell};
pub use document::{DocPosition, RichDocument};
pub use groups::shared as shared_group;
pub use groups::{
    RichAlignGroup, RichAlignGroupRef, RichListGroup, RichListGroupRef, RichQuoteGroup,
    RichQuoteGroupRef,
};
pub use inline::{InlineAtom, InlineAtomKind, RichInline, RichInlineContent, RichRun};
pub use inline_writer::RichInlineContext;
pub use inlines::OffsetOutOfRange;
