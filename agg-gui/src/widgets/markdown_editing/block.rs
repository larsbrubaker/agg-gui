//! One top-level unit of a [`super::RichDocument`]: [`RichBlock`], its
//! [`RichBlockKind`], list info and table cells.  Ports agg-sharp's
//! `RichBlock.cs`.
//!
//! Which members matter depends on the kind: text-bearing kinds (Paragraph,
//! Heading, ListItem, Quote) use `inlines`; CodeBlock uses `code_text`; Table
//! uses `table_rows`; Raw uses only `original_source`.  Inline offset
//! arithmetic lives in [`super::inlines`]; the groups a block may share with
//! its neighbours in [`super::groups`].

use std::sync::atomic::{AtomicU64, Ordering};

use super::groups::{RichAlignGroupRef, RichListGroupRef, RichQuoteGroupRef};
use super::inline::RichInline;
use super::inlines;

/// What a block is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RichBlockKind {
    /// A paragraph.
    #[default]
    Paragraph,
    /// An ATX or setext heading.
    Heading,
    /// One list item (each item is its own block).
    ListItem,
    /// One paragraph of a blockquote.
    Quote,
    /// A fenced or indented code block.
    CodeBlock,
    /// A pipe table.
    Table,
    /// Markdown the rich view does not model (raw HTML, thematic breaks, ...).
    /// Shown rendered and edited only in the Markdown tab; always written back
    /// as `original_source`.
    Raw,
}

/// Paragraph, heading or table-column alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RichAlignment {
    /// Left (the default).
    #[default]
    Left,
    /// Centred.
    Center,
    /// Right.
    Right,
}

/// What makes a block a list item.  Each item is its own block so the caret,
/// Enter and Tab treat items like paragraphs; depth and the marker are enough
/// for the writer to rebuild the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RichListInfo {
    /// A numbered list.
    pub ordered: bool,
    /// Nesting level, 0 for a top-level item.
    pub depth: usize,
    /// The source marker character (`-`, `*`, `+` for bullets; `.` or `)` for
    /// numbered) so a rewrite keeps the author's choice.
    pub marker: char,
    /// The number an ordered list (or nested sublist) starts at.  Every item of
    /// the (sub)list carries it, so deleting the first item keeps the start.
    pub start_number: i32,
    /// Whether the (sub)list this item is in renders its items as paragraphs
    /// (markdown's "loose" list: items separated by blank lines).  Every item
    /// of the (sub)list carries it, like `start_number`, so a regenerated list
    /// keeps its spacing; edits create gaps of their own, so the gaps cannot
    /// tell.
    pub loose: bool,
}

impl Default for RichListInfo {
    fn default() -> Self {
        Self {
            ordered: false,
            depth: 0,
            marker: '-',
            start_number: 1,
            loose: false,
        }
    }
}

/// One table cell: its inlines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RichTableCell {
    /// The cell's styled inlines.
    pub inlines: Vec<RichInline>,
}

/// A block's identity: C#'s object reference, which `RichDocumentView` keys
/// its cached layouts on.  Every new block gets a fresh one, a clone included
/// (C#'s `Clone` makes a new object); editing a block in place keeps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RichBlockId(u64);

impl RichBlockId {
    fn fresh() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// One top-level unit of a document.
///
/// `Clone` is C#'s `RichBlock.Clone`: a deep copy of the block's own content
/// that keeps its group references, so the copy is still a member of the same
/// groups; [`super::RichDocument::deep_clone`] remaps them to cloned groups.
/// The copy is a new block, with its own [`RichBlockId`].
#[derive(Debug)]
pub struct RichBlock {
    /// What the block is.
    pub kind: RichBlockKind,
    /// 1-6 for a Heading, 0 otherwise.
    pub heading_level: u8,
    /// Set only for a ListItem.
    pub list: Option<RichListInfo>,
    /// Paragraph and heading alignment, stored in markdown as a `<div align>`
    /// wrapper.  An edit changes this; `align_group` keeps the wrapper's parsed
    /// alignment.
    pub alignment: RichAlignment,
    /// A text block's styled inlines.
    pub inlines: Vec<RichInline>,
    /// A CodeBlock's text, lines joined with `\n` and no trailing newline,
    /// without fences or indentation.
    pub code_text: String,
    /// A CodeBlock's language for display: the first word of `code_info`, or
    /// "" when it has none.
    pub code_language: String,
    /// A fenced CodeBlock's whole info string exactly as written after the
    /// opening fence (trimmed; entities, escapes and words after the language
    /// kept), so a rewrite reproduces it.  "" for none or indented code.
    pub code_info: String,
    /// A CodeBlock's opening fence as written (```` ``` ````, `~~~~`, ...), so
    /// a rewrite keeps the author's fence (a longer fence is how a block shows
    /// ```` ``` ```` inside it).  "" for an indented code block.
    pub code_fence: String,
    /// A Table's rows, header row first; every row has one cell per column (a
    /// row written short is padded with empty cells).
    pub table_rows: Vec<Vec<RichTableCell>>,
    /// One entry per table column; `None` is a column with no alignment colon
    /// (`---` rather than `:---`), which writes and renders differently from
    /// an explicit Left.
    pub column_alignments: Vec<Option<RichAlignment>>,
    /// The exact markdown this block was parsed from; written back verbatim
    /// while the block is not dirty.  A ListItem's source runs from its marker
    /// to the end of its paragraph (its indentation and any nested items' lines
    /// are not in it: indentation is whitespace in `separator_before`, nested
    /// items are blocks of their own).  A fenced CodeBlock's source starts at
    /// its line's start, so an indented fence's indentation is in it, not the
    /// separator: that indent is stripped from the content lines too, so it
    /// belongs to the block.  A Quote's source carries its own `> ` prefixes;
    /// for the second and later paragraphs of one blockquote it also starts
    /// with the `>` line that separates them from the paragraph before, so the
    /// separator between them stays a bare line break.  Which quote or list a
    /// block belongs to is `quote_group` / `list_group`, never inferred from
    /// these bytes.
    pub original_source: String,
    /// The markdown between the previous block (or the frontmatter, for the
    /// first block) and this one, usually blank lines.  Kept on the block so
    /// inserting and deleting blocks keeps each gap with its block.  Holds only
    /// blank/whitespace text: alignment wrappers are never stored here; an
    /// untouched wrapper's original bytes live on `align_group`.
    pub separator_before: String,
    /// The `<div align>` wrapper this paragraph or heading sits in, shared with
    /// the other members; `None` outside a wrapper.
    pub align_group: Option<RichAlignGroupRef>,
    /// The blockquote this Quote block is a paragraph of, shared with its other
    /// paragraphs.
    pub quote_group: Option<RichQuoteGroupRef>,
    /// The top-level list this ListItem belongs to, shared with every item at
    /// any depth of that list.
    pub list_group: Option<RichListGroupRef>,
    /// True once an edit touched this block; the writer regenerates dirty
    /// blocks and copies the rest verbatim.
    pub dirty: bool,
    /// The block's identity (see [`RichBlock::id`]).
    pub(crate) id: RichBlockId,
}

impl Clone for RichBlock {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind,
            heading_level: self.heading_level,
            list: self.list.clone(),
            alignment: self.alignment,
            inlines: self.inlines.clone(),
            code_text: self.code_text.clone(),
            code_language: self.code_language.clone(),
            code_info: self.code_info.clone(),
            code_fence: self.code_fence.clone(),
            table_rows: self.table_rows.clone(),
            column_alignments: self.column_alignments.clone(),
            original_source: self.original_source.clone(),
            separator_before: self.separator_before.clone(),
            align_group: self.align_group.clone(),
            quote_group: self.quote_group.clone(),
            list_group: self.list_group.clone(),
            dirty: self.dirty,
            id: RichBlockId::fresh(),
        }
    }
}

impl Default for RichBlock {
    fn default() -> Self {
        Self {
            kind: RichBlockKind::Paragraph,
            heading_level: 0,
            list: None,
            alignment: RichAlignment::Left,
            inlines: Vec::new(),
            code_text: String::new(),
            code_language: String::new(),
            code_info: String::new(),
            code_fence: "```".to_string(),
            table_rows: Vec::new(),
            column_alignments: Vec::new(),
            original_source: String::new(),
            separator_before: String::new(),
            align_group: None,
            quote_group: None,
            list_group: None,
            dirty: false,
            id: RichBlockId::fresh(),
        }
    }
}

impl RichBlock {
    /// An empty block of `kind`.
    pub fn new(kind: RichBlockKind) -> Self {
        Self {
            kind,
            ..Self::default()
        }
    }

    /// This block's identity: the same while it is edited in place, new for
    /// every new block or clone.
    pub fn id(&self) -> RichBlockId {
        self.id
    }

    /// True for the kinds that hold inlines directly: Paragraph, Heading,
    /// ListItem and Quote.
    pub fn is_text_block(&self) -> bool {
        matches!(
            self.kind,
            RichBlockKind::Paragraph
                | RichBlockKind::Heading
                | RichBlockKind::ListItem
                | RichBlockKind::Quote
        )
    }

    /// The inlines a position's offset counts through: the block's own, or a
    /// table cell's.  `None` for CodeBlock and Raw, which have no inlines (C#
    /// throws `InvalidOperationException`).  A cell outside the table panics,
    /// as C#'s indexer throws.
    pub fn inlines_at(&self, row: usize, column: usize) -> Option<&Vec<RichInline>> {
        if self.kind == RichBlockKind::Table {
            Some(&self.table_rows[row][column].inlines)
        } else if self.is_text_block() {
            Some(&self.inlines)
        } else {
            None
        }
    }

    /// [`Self::inlines_at`], mutably.
    pub fn inlines_at_mut(&mut self, row: usize, column: usize) -> Option<&mut Vec<RichInline>> {
        if self.kind == RichBlockKind::Table {
            Some(&mut self.table_rows[row][column].inlines)
        } else if self.is_text_block() {
            Some(&mut self.inlines)
        } else {
            None
        }
    }

    /// The largest caret offset in the block (in the given cell for a Table).
    /// A Raw block is one atom.
    pub fn text_length(&self, row: usize, column: usize) -> usize {
        match self.kind {
            RichBlockKind::CodeBlock => self.code_text.chars().count(),
            RichBlockKind::Raw => 1,
            _ => self
                .inlines_at(row, column)
                .map_or(0, |i| inlines::length(i)),
        }
    }
}
