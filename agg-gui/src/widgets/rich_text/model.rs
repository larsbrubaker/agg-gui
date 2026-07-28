//! Rich-text **document model** and structural editing primitives.
//!
//! This is the data core of the `rich_text` widget family (see the module
//! [`super`] header for the phase plan).  It defines the owner-specified
//! document types — [`InlineStyle`], [`TextRun`], [`ListKind`], [`Block`],
//! [`RichDoc`] — plus position/range helpers ([`DocPos`], [`DocRange`]) and
//! the low-level merge / split / normalize operations that every higher layer
//! (the command engine in [`super::commands`], and phase 2's keyboard editing)
//! builds on.
//!
//! # Position model
//!
//! A [`DocPos`] addresses a caret location as `(block, byte)` where `byte` is a
//! byte offset into the block's **flattened** text (the concatenation of all
//! its runs' strings).  All offsets are expected to fall on UTF-8 char
//! boundaries — callers that derive offsets from real text always satisfy this.
//!
//! # Normalization invariant
//!
//! After any structural edit the affected block is [`Block::normalize`]d:
//! zero-length runs are dropped and adjacent runs with identical
//! [`InlineStyle`] are merged.  This keeps the run list canonical so tests and
//! layout see the minimal representation.

use crate::color::Color;
use crate::widgets::text_area::TextHAlign;

/// Per-run inline character formatting.
///
/// Every `Option` field means "inherit the widget's default" when `None`;
/// `Some(_)` overrides it.  The four boolean attributes are absolute (a run is
/// either bold or not).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InlineStyle {
    /// Bold weight (the resolver picks the matching face).
    pub bold: bool,
    /// Italic slant (the resolver picks the matching face).
    pub italic: bool,
    /// Draw an underline under the run.
    pub underline: bool,
    /// Draw a line through the run.
    pub strikethrough: bool,
    /// Font family name, or `None` to inherit the widget default family.
    pub font_family: Option<String>,
    /// Font size in points, or `None` to inherit the widget default size.
    pub font_size: Option<f64>,
    /// Text colour, or `None` to inherit the theme's text colour.
    pub text_color: Option<Color>,
    /// Highlight (background) colour behind the run, or `None` for none.
    pub highlight: Option<Color>,
}

/// A contiguous span of text sharing one [`InlineStyle`].
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    /// The run's characters (never contains `\n` — newlines are block breaks).
    pub text: String,
    /// The inline formatting shared by every character in the run.
    pub style: InlineStyle,
}

impl TextRun {
    /// A run of `text` in `style`.
    pub fn new(text: impl Into<String>, style: InlineStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    /// A run of `text` in the default (fully-inherited) style.
    pub fn plain(text: impl Into<String>) -> Self {
        Self::new(text, InlineStyle::default())
    }
}

/// List decoration applied to a whole [`Block`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListKind {
    /// Not a list item — a plain paragraph.
    #[default]
    None,
    /// A bulleted (`•`) list item.
    Bullet,
    /// An ordered (`1.`, `2.`, …) list item.
    Ordered,
}

/// A paragraph: an ordered list of styled runs plus block-level attributes
/// (alignment, list decoration, indent depth).
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// The paragraph's styled runs, in order.
    pub runs: Vec<TextRun>,
    /// Horizontal text alignment within the block.
    pub align: TextHAlign,
    /// List decoration (none / bullet / ordered).
    pub list: ListKind,
    /// Indent depth; each level offsets the block by [`INDENT_PX`](super::layout::INDENT_PX).
    pub indent: u8,
}

impl Default for Block {
    fn default() -> Self {
        Self::new()
    }
}

impl Block {
    /// An empty paragraph (no runs) with default attributes.
    pub fn new() -> Self {
        Self {
            runs: Vec::new(),
            align: TextHAlign::Left,
            list: ListKind::None,
            indent: 0,
        }
    }

    /// A paragraph containing a single styled run.
    pub fn from_run(run: TextRun) -> Self {
        Self {
            runs: vec![run],
            ..Self::new()
        }
    }

    /// A paragraph of plain (default-style) text.
    pub fn plain(text: impl Into<String>) -> Self {
        let text = text.into();
        if text.is_empty() {
            Self::new()
        } else {
            Self::from_run(TextRun::plain(text))
        }
    }

    /// Total byte length of this block's flattened text.
    pub fn text_len(&self) -> usize {
        self.runs.iter().map(|r| r.text.len()).sum()
    }

    /// The block's flattened text (all runs concatenated).
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// Ensure a run boundary exists at flattened byte offset `byte`, splitting
    /// the straddling run when necessary.  Returns the index of the run that
    /// *begins* at `byte` (or `runs.len()` when `byte` is at/after the end).
    ///
    /// This is the fundamental primitive for applying a style or removing text
    /// across an arbitrary sub-range: split at both edges, then operate on the
    /// runs in between.
    pub fn ensure_boundary(&mut self, byte: usize) -> usize {
        let mut acc = 0usize;
        let mut i = 0usize;
        while i < self.runs.len() {
            if byte == acc {
                return i;
            }
            let len = self.runs[i].text.len();
            if byte < acc + len {
                let split_at = byte - acc;
                debug_assert!(
                    self.runs[i].text.is_char_boundary(split_at),
                    "DocPos byte offset must land on a UTF-8 char boundary"
                );
                let tail = self.runs[i].text.split_off(split_at);
                let style = self.runs[i].style.clone();
                self.runs.insert(i + 1, TextRun { text: tail, style });
                return i + 1;
            }
            acc += len;
            i += 1;
        }
        self.runs.len()
    }

    /// Drop empty runs and merge adjacent runs that share an identical style.
    pub fn normalize(&mut self) {
        self.runs.retain(|r| !r.text.is_empty());
        let mut i = 0;
        while i + 1 < self.runs.len() {
            if self.runs[i].style == self.runs[i + 1].style {
                let next = self.runs.remove(i + 1);
                self.runs[i].text.push_str(&next.text);
            } else {
                i += 1;
            }
        }
    }
}

/// A whole rich-text document: an ordered list of blocks (paragraphs).
#[derive(Clone, Debug, PartialEq)]
pub struct RichDoc {
    /// The document's paragraphs, in order (always at least one).
    pub blocks: Vec<Block>,
}

impl Default for RichDoc {
    fn default() -> Self {
        Self::new()
    }
}

impl RichDoc {
    /// A document with a single empty paragraph — the canonical "blank" state.
    pub fn new() -> Self {
        Self {
            blocks: vec![Block::new()],
        }
    }

    /// A document from `blocks`, substituting the blank state when empty.
    pub fn from_blocks(blocks: Vec<Block>) -> Self {
        if blocks.is_empty() {
            Self::new()
        } else {
            Self { blocks }
        }
    }

    /// Position just past the last character of the last block.
    pub fn end_pos(&self) -> DocPos {
        let block = self.blocks.len().saturating_sub(1);
        DocPos {
            block,
            byte: self.blocks.get(block).map(Block::text_len).unwrap_or(0),
        }
    }

    /// The document's text with blocks separated by `\n`.
    pub fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .map(Block::text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ---------------------------------------------------------------------------
// Positions and ranges
// ---------------------------------------------------------------------------

/// A caret position: block index + byte offset into that block's flattened
/// text.  Derives `Ord` so positions sort by `(block, byte)` — earlier in the
/// document is "less".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct DocPos {
    /// Index of the block (paragraph) this position lies in.
    pub block: usize,
    /// Byte offset into that block's flattened text.
    pub byte: usize,
}

impl DocPos {
    /// A position at `byte` within `block`.
    pub fn new(block: usize, byte: usize) -> Self {
        Self { block, byte }
    }
}

/// A selection between two positions.  `start`/`end` need not be ordered;
/// [`DocRange::ordered`] returns them low-to-high.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocRange {
    /// One endpoint (not necessarily the earlier one).
    pub start: DocPos,
    /// The other endpoint (not necessarily the later one).
    pub end: DocPos,
}

impl DocRange {
    /// A range between `start` and `end` (in either document order).
    pub fn new(start: DocPos, end: DocPos) -> Self {
        Self { start, end }
    }

    /// A zero-width range at a single position.
    pub fn collapsed(pos: DocPos) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }

    /// `true` when the range selects nothing.
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// `(low, high)` — the two endpoints in document order.
    pub fn ordered(&self) -> (DocPos, DocPos) {
        if self.start <= self.end {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    /// The earlier endpoint in document order.
    pub fn min(&self) -> DocPos {
        self.ordered().0
    }

    /// The later endpoint in document order.
    pub fn max(&self) -> DocPos {
        self.ordered().1
    }
}

// ---------------------------------------------------------------------------
// Structural edits
// ---------------------------------------------------------------------------

/// Insert `text` (all in `style`) at `pos`.  Adjacent identical-style runs are
/// merged afterward, so passing the surrounding run's style yields a seamless
/// splice.
pub fn insert_text(doc: &mut RichDoc, pos: DocPos, text: &str, style: InlineStyle) {
    if text.is_empty() {
        return;
    }
    let Some(block) = doc.blocks.get_mut(pos.block) else {
        return;
    };
    let idx = block.ensure_boundary(pos.byte);
    block.runs.insert(
        idx,
        TextRun {
            text: text.to_string(),
            style,
        },
    );
    block.normalize();
}

/// Remove everything inside `range`, preserving the styles of the surviving
/// text.  When the range spans multiple blocks the tail of the last block is
/// merged into the first (Backspace-across-paragraphs semantics).  Returns the
/// collapse position (the range's low endpoint).
pub fn remove_range(doc: &mut RichDoc, range: DocRange) -> DocPos {
    let (a, b) = range.ordered();
    if a == b {
        return a;
    }

    // Clamp the high endpoint's block into range so the primitive is total — a
    // range whose `end` names a block past the document (as a select-all to
    // `end_pos` of a shrunk doc might) never indexes out of bounds.
    let b = DocPos {
        block: b.block.min(doc.blocks.len().saturating_sub(1)),
        byte: b.byte,
    };

    if a.block == b.block {
        if let Some(block) = doc.blocks.get_mut(a.block) {
            let start = block.ensure_boundary(a.byte);
            let end = block.ensure_boundary(b.byte);
            block.runs.drain(start..end);
            block.normalize();
        }
        return a;
    }

    // Multi-block: keep the head of `a.block`, the tail of `b.block`, drop the
    // interior, then splice the two survivors into one paragraph.
    if let Some(block) = doc.blocks.get_mut(a.block) {
        let start = block.ensure_boundary(a.byte);
        block.runs.truncate(start);
    }
    let tail_runs = if let Some(block) = doc.blocks.get_mut(b.block) {
        let end = block.ensure_boundary(b.byte);
        block.runs.drain(0..end);
        std::mem::take(&mut block.runs)
    } else {
        Vec::new()
    };
    if let Some(block) = doc.blocks.get_mut(a.block) {
        block.runs.extend(tail_runs);
        block.normalize();
    }
    doc.blocks.drain(a.block + 1..=b.block);
    a
}

/// Split the block at `pos` into two paragraphs (Enter semantics).  The new
/// block inherits the original's alignment, list kind, and indent.  Returns the
/// position at the start of the new block.
pub fn split_block(doc: &mut RichDoc, pos: DocPos) -> DocPos {
    let Some(block) = doc.blocks.get_mut(pos.block) else {
        return pos;
    };
    let idx = block.ensure_boundary(pos.byte);
    let tail_runs = block.runs.split_off(idx);
    let (align, list, indent) = (block.align, block.list, block.indent);
    block.normalize();
    let mut new_block = Block {
        runs: tail_runs,
        align,
        list,
        indent,
    };
    new_block.normalize();
    doc.blocks.insert(pos.block + 1, new_block);
    DocPos {
        block: pos.block + 1,
        byte: 0,
    }
}

/// Merge block `idx` into block `idx - 1` (Backspace-at-start semantics),
/// keeping the previous block's paragraph attributes.  Returns the position of
/// the join point.  A no-op (returns the start of `idx`) when `idx` is 0 or out
/// of range.
pub fn merge_block_with_prev(doc: &mut RichDoc, idx: usize) -> DocPos {
    if idx == 0 || idx >= doc.blocks.len() {
        return DocPos {
            block: idx,
            byte: 0,
        };
    }
    let join_byte = doc.blocks[idx - 1].text_len();
    let block = doc.blocks.remove(idx);
    doc.blocks[idx - 1].runs.extend(block.runs);
    doc.blocks[idx - 1].normalize();
    DocPos {
        block: idx - 1,
        byte: join_byte,
    }
}

/// Extract the content within `range` as a standalone list of blocks, keeping
/// each run's [`InlineStyle`] and (for wholly-selected blocks) the block-level
/// attributes.  Drives styled Copy / Cut — the inverse shape of
/// [`splice_fragment`].  Returns an empty vec for a collapsed range.
pub fn extract_range(doc: &RichDoc, range: DocRange) -> Vec<Block> {
    let (a, b) = range.ordered();
    if a == b {
        return Vec::new();
    }
    let b_block = b.block.min(doc.blocks.len().saturating_sub(1));
    let mut out = Vec::new();
    for bi in a.block..=b_block {
        let Some(src) = doc.blocks.get(bi) else {
            continue;
        };
        let mut block = src.clone();
        let hi = if bi == b_block {
            b.byte
        } else {
            block.text_len()
        };
        let lo = if bi == a.block { a.byte } else { 0 };
        // Trim the tail first so the head boundary offset stays valid.
        let end = block.ensure_boundary(hi);
        block.runs.truncate(end);
        let start = block.ensure_boundary(lo);
        block.runs.drain(0..start);
        block.normalize();
        out.push(block);
    }
    out
}

/// Insert a `fragment` (a list of blocks produced by [`extract_range`]) at
/// `pos`, preserving run styles.  A single-block fragment splices inline (no new
/// paragraph); a multi-block fragment splits the target paragraph, appends the
/// fragment's first block to the head, inserts the interior blocks verbatim, and
/// merges the fragment's last block with the original tail (which keeps the
/// target paragraph's block attributes).  Returns the caret position at the end
/// of the inserted content.
pub fn splice_fragment(doc: &mut RichDoc, pos: DocPos, fragment: &[Block]) -> DocPos {
    if fragment.is_empty() {
        return pos;
    }
    let Some(target) = doc.blocks.get_mut(pos.block) else {
        return pos;
    };
    if fragment.len() == 1 {
        // Pasting one paragraph's worth of runs is an inline splice — it must
        // not change the target paragraph's block attributes. The exception is
        // pasting into a pristine empty paragraph (a blank doc, or a fresh line
        // after Enter): there the fragment's own block attributes (e.g. a bullet
        // list) should carry over, so a copied list item pastes as a list item.
        let default = Block::new();
        let adopt_attrs = target.runs.is_empty()
            && target.align == default.align
            && target.list == default.list
            && target.indent == default.indent;
        let mut idx = target.ensure_boundary(pos.byte);
        let mut added = 0usize;
        for run in &fragment[0].runs {
            target.runs.insert(idx, run.clone());
            idx += 1;
            added += run.text.len();
        }
        target.normalize();
        if adopt_attrs {
            target.align = fragment[0].align;
            target.list = fragment[0].list;
            target.indent = fragment[0].indent;
        }
        return DocPos::new(pos.block, pos.byte + added);
    }

    // Multi-block: split the target paragraph at `pos`, keeping the head in
    // place and stashing the tail runs to re-attach after the fragment.
    let split_idx = target.ensure_boundary(pos.byte);
    let tail_runs = target.runs.split_off(split_idx);
    let (t_align, t_list, t_indent) = (target.align, target.list, target.indent);
    target.runs.extend(fragment[0].runs.iter().cloned());
    target.normalize();

    // Interior blocks verbatim, then the final block: the fragment's last block
    // merged with the original tail. The merged line keeps the target
    // paragraph's block attributes (it is a continuation of that paragraph).
    let last = &fragment[fragment.len() - 1];
    let end_byte = last.text_len();
    let mut new_blocks: Vec<Block> = fragment[1..fragment.len() - 1].to_vec();
    let mut last_block = Block {
        runs: last.runs.clone(),
        align: t_align,
        list: t_list,
        indent: t_indent,
    };
    last_block.runs.extend(tail_runs);
    last_block.normalize();
    new_blocks.push(last_block);

    let count = new_blocks.len(); // == fragment.len() - 1
    for (k, block) in new_blocks.into_iter().enumerate() {
        doc.blocks.insert(pos.block + 1 + k, block);
    }
    DocPos::new(pos.block + count, end_byte)
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod model_tests;
