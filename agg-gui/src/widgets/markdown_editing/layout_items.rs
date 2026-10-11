//! A block turned into layout items, and greedy line breaking over them — a
//! port of agg-sharp's `RichLayoutItems.cs`.
//!
//! Advances are summed per character rather than measured per string: the
//! faces do no kerning, so the sum is exact, and it gives every caret x as a
//! by-product.  Items index chars, as caret offsets do; agg-sharp indexes
//! UTF-16 code units, so its "second half of a surrogate pair" case has no
//! counterpart here (a character outside the BMP is one item).
//! [`super::block_layout`] turns the lines into [`super::layout_line`]s.

use crate::geometry::Size;

use super::block::RichBlock;
use super::inline::{InlineAtomKind, RichInline, RichInlineContent};
use super::layout_style::{RichFace, RichLayoutStyle};
use super::mark_class::is_mark;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayoutItemKind {
    Char,
    Space,
    Break,
    /// An image or a Raw block: a box the caret steps over whole, and a place
    /// a line may break before and after.
    Box,
}

/// One drawn piece of a block: a character, a box, a code line's '\n', or one
/// character of a text atom's raw markdown.  A text atom is many items but one
/// caret offset, so items carry their offset rather than being indexed by it.
#[derive(Clone, Debug)]
pub(crate) struct LayoutItem {
    pub kind: LayoutItemKind,
    /// Fixed advance; a tab's depends on where it starts (see `advance_at`).
    pub advance: f64,
    pub face: Option<RichFace>,
    /// The inline the item belongs to; `None` for code text and a Raw box.
    pub inline_index: Option<usize>,
    pub start_in_inline: usize,
    /// An image's height; text takes its height from `face`.
    pub box_height: f64,
    /// The caret offset before this item.
    pub offset: usize,
    /// The caret may sit before this item: false before a combining mark and
    /// inside a text atom.
    pub stop_before: bool,
    /// A line may never start here: a combining mark or a joined character.
    pub no_break_before: bool,
    pub is_tab: bool,
}

impl LayoutItem {
    fn plain(kind: LayoutItemKind, inline_index: Option<usize>, offset: usize) -> Self {
        Self {
            kind,
            advance: 0.0,
            face: None,
            inline_index,
            start_in_inline: 0,
            box_height: 0.0,
            offset,
            stop_before: true,
            no_break_before: false,
            is_tab: false,
        }
    }

    /// The item's advance when it starts at `x` from the line's start.  A tab
    /// advances to the next stop four spaces apart, rather than by the font's
    /// tab glyph.
    pub fn advance_at(&self, x: f64) -> f64 {
        if !self.is_tab {
            return self.advance;
        }
        let tab_stop = self
            .face
            .as_ref()
            .map_or(0.0, |face| 4.0 * face.advance(' '));
        if tab_stop <= 0.0 {
            0.0
        } else {
            tab_stop - (x % tab_stop)
        }
    }

    pub fn is_content(&self) -> bool {
        matches!(self.kind, LayoutItemKind::Char | LayoutItemKind::Box)
    }
}

/// The items of a list of inlines (a block's or a table cell's); `run_face`
/// picks each run's face, since a heading or a header cell draws its runs bold.
pub(crate) fn inline_items(
    inlines: &[RichInline],
    run_face: &dyn Fn(&RichInline) -> RichFace,
    style: &RichLayoutStyle,
    block_face: &RichFace,
) -> Vec<LayoutItem> {
    let mut items = Vec::new();
    let mut offset = 0;
    for (inline_index, inline) in inlines.iter().enumerate() {
        match &inline.content {
            RichInlineContent::Run(run) => {
                let face = run_face(inline);
                let text: Vec<char> = run.text.chars().collect();
                for i in 0..text.len() {
                    items.push(char_item(
                        &text,
                        i,
                        &face,
                        Some(inline_index),
                        offset,
                        false,
                    ));
                    offset += 1;
                }
            }
            RichInlineContent::Atom(atom) if atom.kind == InlineAtomKind::HardBreak => {
                let mut item = LayoutItem::plain(LayoutItemKind::Break, Some(inline_index), offset);
                item.face = Some(block_face.clone());
                items.push(item);
                offset += 1;
            }
            RichInlineContent::Atom(atom)
                if atom.kind == InlineAtomKind::Image || atom.raw_markdown.is_empty() =>
            {
                let line_height = block_face.ascent_px() + block_face.descent_px();
                let size = if atom.kind == InlineAtomKind::Image {
                    style
                        .image_size
                        .as_ref()
                        .map_or(Size::new(line_height, line_height), |f| f(atom))
                } else {
                    Size::new(0.0, 0.0)
                };
                let mut item = LayoutItem::plain(LayoutItemKind::Box, Some(inline_index), offset);
                item.advance = size.width;
                item.box_height = size.height;
                items.push(item);
                offset += 1;
            }
            RichInlineContent::Atom(atom) => {
                // Inline HTML, autolinks and unmodelled inlines show their
                // markdown as plain text and wrap like a word: no break inside
                // (even at its spaces) unless it is wider than the line.
                let text: Vec<char> = atom.raw_markdown.chars().collect();
                for i in 0..text.len() {
                    let mut item =
                        char_item(&text, i, block_face, Some(inline_index), offset, true);
                    item.stop_before = i == 0;
                    items.push(item);
                }
                offset += 1;
            }
        }
    }
    finish(items)
}

/// A code block's items: its characters, with each '\n' a break.
pub(crate) fn code_items(code_text: &str, face: &RichFace) -> Vec<LayoutItem> {
    let text: Vec<char> = code_text.chars().collect();
    let mut items = Vec::with_capacity(text.len());
    for i in 0..text.len() {
        let mut item = char_item(&text, i, face, None, i, true);
        if text[i] == '\n' {
            item.kind = LayoutItemKind::Break;
            item.advance = 0.0;
        }
        items.push(item);
    }
    finish(items)
}

/// A Raw block is one box (its rendered markdown, drawn by a child widget)
/// with a caret before and after it.
pub(crate) fn raw_items(
    block: &RichBlock,
    available: f64,
    style: &RichLayoutStyle,
    block_face: &RichFace,
) -> Vec<LayoutItem> {
    let line_height = block_face.ascent_px() + block_face.descent_px();
    let size = style
        .raw_block_size
        .as_ref()
        .map_or(Size::new(available, line_height), |f| f(block, available));
    let mut item = LayoutItem::plain(LayoutItemKind::Box, None, 0);
    item.advance = size.width;
    item.box_height = size.height;
    vec![item]
}

/// Greedy breaking; returns each line's first item.  A line breaks after its
/// last space, or before or after a box, when the next item would cross the
/// width, and between characters when there is no such place (never before a
/// combining mark).  Spaces never cause a break — they stay at the end of the
/// line they follow.  A line always takes at least one item, so something
/// wider than the line gets a line of its own instead of looping.
pub(crate) fn break_lines(
    items: &[LayoutItem],
    available: f64,
    wrap_at_spaces: bool,
) -> Vec<usize> {
    let mut starts = vec![0];
    let mut line_start = 0;
    let mut last_break: Option<usize> = None;
    let mut x = 0.0;
    let mut i = 0;
    while i < items.len() {
        let item = &items[i];
        let advance = item.advance_at(x);
        if item.kind == LayoutItemKind::Break {
            i += 1;
            starts.push(i);
            line_start = i;
            last_break = None;
            x = 0.0;
            continue;
        }

        if wrap_at_spaces && item.kind == LayoutItemKind::Space {
            x += advance;
            i += 1;
            if can_start_line(items, i) {
                last_break = Some(i);
            }
            continue;
        }

        if wrap_at_spaces && item.kind == LayoutItemKind::Box && i > line_start {
            last_break = Some(i);
        }

        if i > line_start && x + advance > available {
            let break_at = match last_break {
                Some(b) if b > line_start => b,
                _ => character_break(items, line_start, i),
            };
            if break_at > line_start && break_at < items.len() {
                starts.push(break_at);
                line_start = break_at;
                last_break = None;
                x = 0.0;
                i = break_at;
                continue;
            }
        }

        x += advance;
        i += 1;
        if wrap_at_spaces && item.kind == LayoutItemKind::Box && can_start_line(items, i) {
            last_break = Some(i);
        }
    }
    starts
}

// A mark attached to a space (or a box) belongs with it, so the line cannot
// break between them.
fn can_start_line(items: &[LayoutItem], i: usize) -> bool {
    i >= items.len() || !items[i].no_break_before
}

// Where to break a word wider than the line at item i: back off to the start
// of the character i is part of, or, when that character began the line,
// forward past it.
fn character_break(items: &[LayoutItem], line_start: usize, i: usize) -> usize {
    let mut break_at = i;
    while break_at > line_start + 1 && items[break_at].no_break_before {
        break_at -= 1;
    }
    if items[break_at].no_break_before {
        break_at = i;
        while break_at < items.len() && items[break_at].no_break_before {
            break_at += 1;
        }
    }
    break_at
}

fn char_item(
    text: &[char],
    i: usize,
    face: &RichFace,
    inline_index: Option<usize>,
    offset: usize,
    code: bool,
) -> LayoutItem {
    let c = text[i];
    let attached = is_attached(text, i);
    let tab = c == '\t';
    let space = !code && (c == ' ' || tab);
    LayoutItem {
        kind: if space {
            LayoutItemKind::Space
        } else {
            LayoutItemKind::Char
        },
        advance: if tab { 0.0 } else { face.advance(c) },
        face: Some(face.clone()),
        inline_index,
        start_in_inline: i,
        box_height: 0.0,
        offset,
        stop_before: !attached,
        no_break_before: attached,
        is_tab: tab,
    }
}

const ZERO_WIDTH_JOINER: char = '\u{200D}';

/// True when the char at `i` continues the character before it, so the caret
/// never stops and a line never breaks before it: a combining or spacing mark,
/// a variation selector, a zero width joiner and whatever it joins on, and the
/// second flag letter of a regional-indicator pair.  Looks only within one
/// run's text.
fn is_attached(text: &[char], i: usize) -> bool {
    let c = text[i];
    if is_mark(c)
        || c == ZERO_WIDTH_JOINER
        || ('\u{FE00}'..='\u{FE0F}').contains(&c)
        || (i > 0 && text[i - 1] == ZERO_WIDTH_JOINER)
    {
        return true;
    }

    // Flags are pairs of regional indicators: the second, fourth, ... of a run
    // of them joins the one before.
    if is_regional_indicator(c) {
        let before = text[..i]
            .iter()
            .rev()
            .take_while(|&&p| is_regional_indicator(p))
            .count();
        return before % 2 == 1;
    }
    false
}

// U+1F1E6..U+1F1FF, the regional indicator letters.
fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

// The block's start is always a caret stop, even when it begins with a
// combining mark.
fn finish(mut items: Vec<LayoutItem>) -> Vec<LayoutItem> {
    if let Some(first) = items.first_mut() {
        first.stop_before = true;
        first.no_break_before = false;
    }
    items
}
