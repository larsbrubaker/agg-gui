//! Rich-text **command engine** — applies editor commands to a [`RichDoc`]
//! over a selection [`DocRange`].
//!
//! Sits on top of [`super::model`]'s structural primitives.  Inline commands
//! (bold/italic/color/…) split runs at the exact range edges and mutate only
//! the covered runs, using egui/Word toggle semantics: if the *entire* range
//! already carries an attribute, the toggle removes it, otherwise it sets it.
//! Block commands (align/list/indent) apply to every block the range touches.
//!
//! Undo/redo is intentionally **not** modelled here — phase 2 wraps the whole
//! document in the existing [`crate::undo`] `Undoer`, so commands stay pure
//! forward mutations.

use super::model::{Block, DocPos, DocRange, InlineStyle, ListKind, RichDoc};
use crate::color::Color;
use crate::widgets::text_area::TextHAlign;

/// Maximum indent depth an [`RichCommand::Indent`] will reach.
pub const MAX_INDENT: u8 = 8;

/// A single editor command.  Deliberately excludes undo/redo (see module docs).
#[derive(Clone, Debug, PartialEq)]
pub enum RichCommand {
    ToggleBold,
    ToggleItalic,
    ToggleUnderline,
    ToggleStrikethrough,
    SetFontFamily(String),
    SetFontSize(f64),
    SetTextColor(Color),
    SetHighlight(Option<Color>),
    SetAlign(TextHAlign),
    SetList(ListKind),
    Indent,
    Outdent,
}

/// Per-attribute summary of the styles across a selection, for toolbar state.
///
/// Each field is `None` when the selected runs **disagree** on that attribute
/// ("mixed"), or `Some(value)` when they all share `value`.  For the
/// inherit-able attributes the value is itself an `Option` (`None` = "all
/// inherit the widget default", `Some(x)` = "all set to `x`").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommonStyle {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub font_family: Option<Option<String>>,
    pub font_size: Option<Option<f64>>,
    pub text_color: Option<Option<Color>>,
    pub highlight: Option<Option<Color>>,
    /// Block-level alignment shared by every block the selection touches, or
    /// `None` when they disagree (mixed). Drives the toolbar's L/C/R alignment
    /// radio group. Only populated by [`range_common_style`] — the inline-only
    /// [`CommonStyle::of_style`] path leaves it `None`.
    pub align: Option<TextHAlign>,
    /// Block-level list decoration shared by every touched block, or `None`
    /// when mixed. Drives the ordered/bullet toggles: `Some(ListKind::None)`
    /// means "all plain paragraphs", so neither list button is active.
    pub list: Option<ListKind>,
}

impl CommonStyle {
    /// A `CommonStyle` in which every attribute agrees with `style` — the
    /// summary of a single uniform run.  Used by the editor to report the
    /// *pending* caret style (a toggled format awaiting the next keystroke) to
    /// the toolbar, where no run yet carries it.
    pub fn of_style(style: &InlineStyle) -> Self {
        Self::from_style(style)
    }

    /// Seed a `CommonStyle` where every attribute agrees with `style`.
    fn from_style(style: &InlineStyle) -> Self {
        Self {
            bold: Some(style.bold),
            italic: Some(style.italic),
            underline: Some(style.underline),
            strikethrough: Some(style.strikethrough),
            font_family: Some(style.font_family.clone()),
            font_size: Some(style.font_size),
            text_color: Some(style.text_color),
            highlight: Some(style.highlight),
            // Block-level attributes are unknown from a run's inline style;
            // `range_common_style` folds them in afterwards from the blocks.
            align: None,
            list: None,
        }
    }

    /// Fold the block-level align/list attributes of every block `range`
    /// touches into this summary. Called by [`range_common_style`] after the
    /// inline attributes are seeded, so an all-agree selection reports a
    /// concrete `Some(_)` and a mixed one reports `None`.
    ///
    /// `pub(crate)` so the editor's collapsed-caret + pending-style path can
    /// fold the caret block's align/list into a [`CommonStyle::of_style`]
    /// summary — block state is independent of the armed inline pending style.
    pub(crate) fn merge_blocks(&mut self, doc: &RichDoc, range: DocRange) {
        let (a, b) = range.ordered();
        let mut first = true;
        for bi in a.block..=b.block {
            let Some(block) = doc.blocks.get(bi) else {
                continue;
            };
            if first {
                self.align = Some(block.align);
                self.list = Some(block.list);
                first = false;
            } else {
                fold(&mut self.align, block.align);
                fold(&mut self.list, block.list);
            }
        }
    }

    /// Fold another run's `style` in: any attribute that now disagrees becomes
    /// `None` (mixed).
    fn merge(&mut self, style: &InlineStyle) {
        fold(&mut self.bold, style.bold);
        fold(&mut self.italic, style.italic);
        fold(&mut self.underline, style.underline);
        fold(&mut self.strikethrough, style.strikethrough);
        fold(&mut self.font_family, style.font_family.clone());
        fold(&mut self.font_size, style.font_size);
        fold(&mut self.text_color, style.text_color);
        fold(&mut self.highlight, style.highlight);
    }
}

/// If `slot` currently agrees on a value that differs from `value`, collapse it
/// to `None` (mixed).
fn fold<T: PartialEq>(slot: &mut Option<T>, value: T) {
    if let Some(existing) = slot {
        if *existing != value {
            *slot = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Style introspection
// ---------------------------------------------------------------------------

/// The style that a newly-typed character at `pos` would inherit: the style of
/// the character immediately *before* `pos` (egui/Word caret semantics), or the
/// first run's style at the very start of a block, or the default in an empty
/// block.
pub fn style_at(doc: &RichDoc, pos: DocPos) -> InlineStyle {
    let Some(block) = doc.blocks.get(pos.block) else {
        return InlineStyle::default();
    };
    if block.runs.is_empty() {
        return InlineStyle::default();
    }
    if pos.byte == 0 {
        return block.runs[0].style.clone();
    }
    // Style of the char just before `pos`.
    let target = pos.byte - 1;
    let mut acc = 0usize;
    for run in &block.runs {
        let len = run.text.len();
        if target < acc + len {
            return run.style.clone();
        }
        acc += len;
    }
    block
        .runs
        .last()
        .map(|r| r.style.clone())
        .unwrap_or_default()
}

/// Summarise the styles across `range`.  An empty range reports the style at
/// its caret position (via [`style_at`]).
pub fn range_common_style(doc: &RichDoc, range: DocRange) -> CommonStyle {
    let (a, b) = range.ordered();
    let mut cs = if a == b {
        CommonStyle::from_style(&style_at(doc, a))
    } else {
        let styles = collect_styles_in_range(doc, range);
        let mut iter = styles.iter();
        match iter.next() {
            Some(first) => {
                let mut cs = CommonStyle::from_style(first);
                for s in iter {
                    cs.merge(s);
                }
                cs
            }
            // Range covers only empty blocks — fall back to the caret style.
            None => CommonStyle::from_style(&style_at(doc, a)),
        }
    };
    // Block attributes (align/list) always summarise every block the range
    // touches, independent of the inline-run folding above.
    cs.merge_blocks(doc, range);
    cs
}

/// Collect (clones of) the styles of every run that intersects `range`.
fn collect_styles_in_range(doc: &RichDoc, range: DocRange) -> Vec<InlineStyle> {
    let (a, b) = range.ordered();
    let mut out = Vec::new();
    for bi in a.block..=b.block {
        let Some(block) = doc.blocks.get(bi) else {
            continue;
        };
        let lo = if bi == a.block { a.byte } else { 0 };
        let hi = if bi == b.block {
            b.byte
        } else {
            block.text_len()
        };
        if lo >= hi {
            continue;
        }
        let mut acc = 0usize;
        for run in &block.runs {
            let len = run.text.len();
            let (rs, re) = (acc, acc + len);
            if len > 0 && re > lo && rs < hi {
                out.push(run.style.clone());
            }
            acc = re;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Command application
// ---------------------------------------------------------------------------

/// Apply `cmd` to `doc` over `range`.
pub fn apply_command(doc: &mut RichDoc, range: DocRange, cmd: &RichCommand) {
    match cmd {
        RichCommand::ToggleBold => toggle_bool(doc, range, |cs| cs.bold, |s, v| s.bold = v),
        RichCommand::ToggleItalic => toggle_bool(doc, range, |cs| cs.italic, |s, v| s.italic = v),
        RichCommand::ToggleUnderline => {
            toggle_bool(doc, range, |cs| cs.underline, |s, v| s.underline = v)
        }
        RichCommand::ToggleStrikethrough => {
            toggle_bool(doc, range, |cs| cs.strikethrough, |s, v| s.strikethrough = v)
        }
        RichCommand::SetFontFamily(family) => {
            for_each_run_in_range(doc, range, |s| s.font_family = Some(family.clone()))
        }
        RichCommand::SetFontSize(size) => {
            for_each_run_in_range(doc, range, |s| s.font_size = Some(*size))
        }
        RichCommand::SetTextColor(color) => {
            for_each_run_in_range(doc, range, |s| s.text_color = Some(*color))
        }
        RichCommand::SetHighlight(color) => {
            for_each_run_in_range(doc, range, |s| s.highlight = *color)
        }
        RichCommand::SetAlign(align) => for_each_block_in_range(doc, range, |b| b.align = *align),
        RichCommand::SetList(kind) => apply_set_list(doc, range, *kind),
        RichCommand::Indent => {
            for_each_block_in_range(doc, range, |b| b.indent = (b.indent + 1).min(MAX_INDENT))
        }
        RichCommand::Outdent => {
            for_each_block_in_range(doc, range, |b| b.indent = b.indent.saturating_sub(1))
        }
    }
}

/// Toggle a boolean attribute using egui/Word semantics: if the whole range
/// already has it set, clear it; otherwise set it everywhere.
fn toggle_bool(
    doc: &mut RichDoc,
    range: DocRange,
    read: impl Fn(&CommonStyle) -> Option<bool>,
    write: impl Fn(&mut InlineStyle, bool),
) {
    let all_set = read(&range_common_style(doc, range)) == Some(true);
    let target = !all_set;
    for_each_run_in_range(doc, range, |s| write(s, target));
}

/// `SetList` toggles back to `None` when every touched block is already that
/// (non-`None`) kind; otherwise it sets the kind.
fn apply_set_list(doc: &mut RichDoc, range: DocRange, kind: ListKind) {
    let (a, b) = range.ordered();
    let all_same = kind != ListKind::None
        && (a.block..=b.block)
            .filter_map(|bi| doc.blocks.get(bi))
            .all(|blk| blk.list == kind);
    let target = if all_same { ListKind::None } else { kind };
    for_each_block_in_range(doc, range, |blk| blk.list = target);
}

/// Split runs at the range edges within each intersecting block, then apply
/// `f` to the style of every run inside the range.  Empty ranges are a no-op
/// (there is nothing to style until phase 2 adds a pending-format caret).
fn for_each_run_in_range(doc: &mut RichDoc, range: DocRange, mut f: impl FnMut(&mut InlineStyle)) {
    let (a, b) = range.ordered();
    if a == b {
        return;
    }
    for bi in a.block..=b.block {
        let Some(block) = doc.blocks.get_mut(bi) else {
            continue;
        };
        let lo = if bi == a.block { a.byte } else { 0 };
        let hi = if bi == b.block {
            b.byte
        } else {
            block.text_len()
        };
        if lo >= hi {
            continue;
        }
        let start = block.ensure_boundary(lo);
        let end = block.ensure_boundary(hi);
        for run in &mut block.runs[start..end] {
            f(&mut run.style);
        }
        block.normalize();
    }
}

/// Apply `f` to every block the range intersects (inclusive of both endpoints'
/// blocks), even when the range is empty (a single block).
fn for_each_block_in_range(doc: &mut RichDoc, range: DocRange, mut f: impl FnMut(&mut Block)) {
    let (a, b) = range.ordered();
    for bi in a.block..=b.block {
        if let Some(block) = doc.blocks.get_mut(bi) {
            f(block);
        }
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod commands_tests;
