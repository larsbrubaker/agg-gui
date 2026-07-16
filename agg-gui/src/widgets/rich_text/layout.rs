//! Rich-text **layout engine** — wraps a [`RichDoc`] to a fixed width with
//! per-run fonts and sizes, producing paint-ready line/fragment geometry.
//!
//! The engine is deliberately catalog-agnostic: it never loads fonts itself.
//! The caller supplies a `resolver: Fn(&InlineStyle) -> Arc<Font>` that maps a
//! run's style (family + bold/italic) to a concrete [`Font`].  The demo crate
//! provides a resolver backed by the system-font catalog (with faux-bold /
//! faux-italic fallback); the library and its tests stay independent of it.
//!
//! # What it computes
//!
//! * Word wrapping across style boundaries — a "word" is a maximal run of
//!   non-whitespace pieces even when they span several differently-styled runs,
//!   so a bold suffix never wraps away from its plain prefix.
//! * Per-visual-line height = `max(ascent + descent) * LINE_SPACING` across the
//!   fragments on that line, so a large glyph grows the whole line.
//! * Block alignment (left / center / right) within the text column.
//! * List markers: `•` for bullets, `N.` for ordered items, hung in a gutter to
//!   the left of the text.  Ordered numbering counts consecutive `Ordered`
//!   blocks at the same indent and restarts after any break.
//!
//! Geometry is expressed **top-down** (y grows downward from the document top);
//! the read-only view flips it into the framework's Y-up space at paint time.

use std::sync::Arc;

use super::model::{Block, InlineStyle, ListKind, RichDoc};
use crate::text::{measure_advance, Font};
use crate::widgets::text_area::TextHAlign;

/// Indent step, in logical pixels, per [`Block::indent`] level.
pub const INDENT_PX: f64 = 24.0;
/// Width reserved to the left of a list item's text for its marker.
pub const LIST_GUTTER_PX: f64 = 24.0;
/// Gap between a list marker's right edge and the text column.
pub const MARKER_GAP_PX: f64 = 6.0;
/// Line-height multiplier applied to the tallest (ascent + descent) on a line.
pub const LINE_SPACING: f64 = 1.35;

/// A resolver maps a run style to the concrete font to shape/measure it with.
pub type FontResolver<'a> = dyn Fn(&InlineStyle) -> Arc<Font> + 'a;

/// One piece of a run as it sits on a single visual line: the smallest unit the
/// painter draws.  A wrapped run may contribute several fragments (one per
/// line); adjacent same-style pieces on a line are merged into one fragment.
#[derive(Clone)]
pub struct LineFragment {
    pub text: String,
    pub style: InlineStyle,
    pub font: Arc<Font>,
    pub font_size: f64,
    /// X offset of this fragment's left edge from the start of the line's text.
    pub x: f64,
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
}

/// One visual line within a block.
#[derive(Clone)]
pub struct LineLayout {
    pub fragments: Vec<LineFragment>,
    /// Total advance width of the line's text.
    pub width: f64,
    /// Line-box height = `max(ascent + descent) * LINE_SPACING`.
    pub height: f64,
    pub ascent: f64,
    pub descent: f64,
    /// Alignment offset added to every fragment's `x` within the text column.
    pub align_dx: f64,
    /// Baseline distance measured downward from the top of the line box.
    pub baseline_from_top: f64,
}

/// Layout of one block/paragraph.
#[derive(Clone)]
pub struct BlockLayout {
    pub lines: Vec<LineLayout>,
    /// Left edge of the text column (indent + optional list gutter).
    pub text_left: f64,
    /// List marker glyph/number, if any.
    pub marker: Option<String>,
    /// Marker font (resolved from the block's leading style).
    pub marker_font: Option<Arc<Font>>,
    pub marker_font_size: f64,
    /// X of the marker's left edge (right-aligned into the gutter).
    pub marker_x: f64,
    pub height: f64,
    pub width: f64,
}

/// Layout of a whole document.
#[derive(Clone)]
pub struct DocLayout {
    pub blocks: Vec<BlockLayout>,
    pub width: f64,
    pub height: f64,
}

/// Effective font size for a run: its explicit size or the widget default.
fn run_size(style: &InlineStyle, default_size: f64) -> f64 {
    style.font_size.unwrap_or(default_size)
}

/// Lay `doc` out to `width` logical pixels using `resolver` for fonts and
/// `default_font_size` where a run leaves its size inherited.
pub fn layout_doc(
    doc: &RichDoc,
    width: f64,
    default_font_size: f64,
    resolver: &FontResolver,
) -> DocLayout {
    let numbers = ordered_numbers(&doc.blocks);
    let mut blocks = Vec::with_capacity(doc.blocks.len());
    let mut y = 0.0f64;
    for (bi, block) in doc.blocks.iter().enumerate() {
        let bl = layout_block(block, numbers[bi], width, default_font_size, resolver);
        y += bl.height;
        blocks.push(bl);
    }
    let max_w = blocks.iter().map(|b| b.width).fold(0.0f64, f64::max);
    DocLayout {
        blocks,
        width: width.max(max_w),
        height: y,
    }
}

/// Compute the ordered-list ordinal for each block (0 when the block is not an
/// ordered item).  A sequence counts consecutive `Ordered` blocks at the same
/// indent and restarts after any break (a non-ordered block, or a different
/// indent).
fn ordered_numbers(blocks: &[Block]) -> Vec<usize> {
    let mut out = vec![0usize; blocks.len()];
    let mut counters = [0usize; (MAX_LEVELS)];
    let mut active = [false; MAX_LEVELS];
    for (i, block) in blocks.iter().enumerate() {
        let d = (block.indent as usize).min(MAX_LEVELS - 1);
        match block.list {
            ListKind::Ordered => {
                counters[d] = if active[d] { counters[d] + 1 } else { 1 };
                out[i] = counters[d];
                // Starting/continuing a run at `d` breaks every other level.
                for (k, a) in active.iter_mut().enumerate() {
                    *a = k == d;
                }
            }
            // Any non-ordered block breaks all running sequences.
            _ => {
                for a in active.iter_mut() {
                    *a = false;
                }
            }
        }
    }
    out
}

const MAX_LEVELS: usize = 16;

/// Lay out one block: resolve its list marker, wrap its runs, and position the
/// resulting lines.
fn layout_block(
    block: &Block,
    ordinal: usize,
    width: f64,
    default_font_size: f64,
    resolver: &FontResolver,
) -> BlockLayout {
    let base_indent = block.indent as f64 * INDENT_PX;
    let is_list = block.list != ListKind::None;
    let text_left = base_indent + if is_list { LIST_GUTTER_PX } else { 0.0 };
    let avail = (width - text_left).max(1.0);

    // The marker inherits the block's leading run style so it visually matches.
    let lead_style = block
        .runs
        .first()
        .map(|r| r.style.clone())
        .unwrap_or_default();
    let marker_font = resolver(&lead_style);
    let marker_font_size = run_size(&lead_style, default_font_size);
    let marker = match block.list {
        ListKind::None => None,
        ListKind::Bullet => Some("\u{2022}".to_string()),
        ListKind::Ordered => Some(format!("{ordinal}.")),
    };
    let marker_x = if let Some(m) = &marker {
        let mw = measure_advance(&marker_font, m, marker_font_size);
        (text_left - MARKER_GAP_PX - mw).max(base_indent)
    } else {
        base_indent
    };

    let pieces = tokenize(block, default_font_size, resolver);
    let mut lines = wrap(pieces, avail);
    if lines.is_empty() {
        // Empty paragraph: reserve one blank line at the default metrics.
        let font = resolver(&lead_style);
        let size = run_size(&lead_style, default_font_size);
        lines.push(LineLayout {
            fragments: Vec::new(),
            width: 0.0,
            height: (font.ascender_px(size) + font.descender_px(size)) * LINE_SPACING,
            ascent: font.ascender_px(size),
            descent: font.descender_px(size),
            align_dx: 0.0,
            baseline_from_top: 0.0,
        });
    }

    // Finalise per-line metrics and alignment.
    let mut height = 0.0f64;
    let mut max_line_w = 0.0f64;
    for line in &mut lines {
        finalize_line(line, avail, block.align);
        height += line.height;
        max_line_w = max_line_w.max(line.width);
    }

    BlockLayout {
        lines,
        text_left,
        marker,
        marker_font: Some(marker_font),
        marker_font_size,
        marker_x,
        height,
        width: text_left + max_line_w,
    }
}

/// A tokenized piece of a run: either whitespace or a non-whitespace chunk,
/// carrying everything needed to measure and later paint it.
struct Piece {
    text: String,
    style: InlineStyle,
    font: Arc<Font>,
    font_size: f64,
    width: f64,
    ascent: f64,
    descent: f64,
    is_ws: bool,
}

/// Split a block's runs into whitespace / non-whitespace pieces, resolving the
/// font and metrics of each.  Runs are assumed to contain no `\n` (newlines are
/// block boundaries in the model).
fn tokenize(block: &Block, default_font_size: f64, resolver: &FontResolver) -> Vec<Piece> {
    let mut pieces = Vec::new();
    for run in &block.runs {
        if run.text.is_empty() {
            continue;
        }
        let font = resolver(&run.style);
        let size = run_size(&run.style, default_font_size);
        let ascent = font.ascender_px(size);
        let descent = font.descender_px(size);
        for chunk in split_ws(&run.text) {
            let is_ws = chunk.chars().next().map(|c| c.is_whitespace()).unwrap_or(false);
            let width = measure_advance(&font, chunk, size);
            pieces.push(Piece {
                text: chunk.to_string(),
                style: run.style.clone(),
                font: Arc::clone(&font),
                font_size: size,
                width,
                ascent,
                descent,
                is_ws,
            });
        }
    }
    pieces
}

/// Yield maximal alternating whitespace / non-whitespace substrings of `s`.
fn split_ws(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut prev_ws: Option<bool> = None;
    for (i, c) in s.char_indices() {
        let ws = c.is_whitespace();
        match prev_ws {
            Some(p) if p != ws => {
                out.push(&s[start..i]);
                start = i;
            }
            _ => {}
        }
        prev_ws = Some(ws);
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// Greedy word-wrap `pieces` into visual lines fitting `avail` width.  A word is
/// a maximal group of consecutive non-whitespace pieces (possibly spanning
/// style boundaries); words are never broken.  A trailing space that would sit
/// at a wrap point is dropped.
fn wrap(pieces: Vec<Piece>, avail: f64) -> Vec<LineLayout> {
    let mut lines: Vec<LineLayout> = Vec::new();
    let mut cur: Vec<Piece> = Vec::new();
    let mut cur_w = 0.0f64;
    let mut pending_space: Option<Piece> = None;

    // Group into words separated by single whitespace pieces.
    let mut iter = pieces.into_iter().peekable();
    while let Some(piece) = iter.next() {
        if piece.is_ws {
            if !cur.is_empty() {
                pending_space = Some(piece);
            }
            continue;
        }
        // Accumulate the whole word (this piece + following non-ws pieces).
        let mut word = vec![piece];
        while let Some(next) = iter.peek() {
            if next.is_ws {
                break;
            }
            word.push(iter.next().unwrap());
        }
        let word_w: f64 = word.iter().map(|p| p.width).sum();
        let space_w = pending_space.as_ref().map(|p| p.width).unwrap_or(0.0);

        if !cur.is_empty() && cur_w + space_w + word_w > avail {
            lines.push(finish_pieces(std::mem::take(&mut cur)));
            cur_w = 0.0;
            pending_space = None;
        } else if let Some(sp) = pending_space.take() {
            cur_w += sp.width;
            cur.push(sp);
        }
        cur_w += word_w;
        cur.extend(word);
    }
    if !cur.is_empty() {
        lines.push(finish_pieces(cur));
    }
    lines
}

/// Turn a line's pieces into a [`LineLayout`], merging adjacent same-style
/// pieces into fragments and computing x offsets and metrics.
fn finish_pieces(pieces: Vec<Piece>) -> LineLayout {
    let mut fragments: Vec<LineFragment> = Vec::new();
    let mut x = 0.0f64;
    let mut ascent = 0.0f64;
    let mut descent = 0.0f64;
    for p in pieces {
        ascent = ascent.max(p.ascent);
        descent = descent.max(p.descent);
        if let Some(last) = fragments.last_mut() {
            if last.style == p.style
                && Arc::ptr_eq(&last.font, &p.font)
                && (last.font_size - p.font_size).abs() < 1e-9
            {
                last.text.push_str(&p.text);
                last.width += p.width;
                x += p.width;
                continue;
            }
        }
        fragments.push(LineFragment {
            text: p.text,
            style: p.style,
            font: p.font,
            font_size: p.font_size,
            x,
            width: p.width,
            ascent: p.ascent,
            descent: p.descent,
        });
        x += p.width;
    }
    LineLayout {
        fragments,
        width: x,
        height: 0.0,
        ascent,
        descent,
        align_dx: 0.0,
        baseline_from_top: 0.0,
    }
}

/// Fill in a line's height, baseline, and alignment offset now that its width
/// and the column width are known.
fn finalize_line(line: &mut LineLayout, avail: f64, align: TextHAlign) {
    let content = line.ascent + line.descent;
    line.height = content * LINE_SPACING;
    let leading = line.height - content;
    line.baseline_from_top = leading * 0.5 + line.ascent;
    line.align_dx = match align {
        TextHAlign::Left => 0.0,
        TextHAlign::Center => ((avail - line.width) * 0.5).max(0.0),
        TextHAlign::Right => (avail - line.width).max(0.0),
    };
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod layout_tests;
