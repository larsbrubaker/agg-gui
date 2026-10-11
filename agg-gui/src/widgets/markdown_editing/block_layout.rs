//! One rich block laid out to a width — a port of agg-sharp's
//! `RichBlockLayout.cs` and `IRichBlockLayout.cs`.
//!
//! Wrapped lines of positioned fragments, the list marker, the quote bar and
//! the code background, plus caret geometry.  Coordinates are y-up like every
//! agg widget, with the origin at the bottom-left of the block's box (spacing
//! above and below included), so the editor widget draws a block by offsetting
//! it to the block's bottom and stacks blocks by adding heights.  Blocks lay
//! out independently, so an edit re-lays out only the block it touched.
//!
//! [`super::layout_items`] makes the items and breaks lines; this builds the
//! [`super::layout_line`]s.  The line building, flipping, hit testing and stop
//! finding are shared with a table's cells (crate-visible).  Where C# throws
//! (a Table block given to [`RichBlockLayout::layout`], an offset outside the
//! block) these return a [`RichLayoutError`].

use std::fmt;

use crate::geometry::{Point, Rect};

use super::block::{RichAlignment, RichBlock, RichBlockKind};
use super::inline::{RichInline, RichInlineContent};
use super::layout_items::{self, LayoutItem, LayoutItemKind};
use super::layout_line::{RichCaret, RichCaretStop, RichLayoutFragment, RichLayoutLine};
use super::layout_style::{RichFace, RichLayoutStyle};

/// Why a layout call refused its input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RichLayoutError {
    /// A Table block, which `RichTableLayout` lays out (C#'s
    /// `NotSupportedException`).
    TableBlock,
    /// An offset past the block's end (C#'s `ArgumentOutOfRangeException`).
    OffsetOutOfRange {
        /// The offset asked for.
        offset: usize,
        /// The block's length.
        length: usize,
    },
}

impl fmt::Display for RichLayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TableBlock => {
                f.write_str("Table blocks are laid out by RichTableLayout, not RichBlockLayout.")
            }
            Self::OffsetOutOfRange { offset, length } => {
                write!(f, "offset {offset} is outside the block (length {length})")
            }
        }
    }
}

impl std::error::Error for RichLayoutError {}

/// What the editor widget needs from any laid-out block, so it keeps one list
/// of layouts whether a block is text or a table grid (C# `IRichBlockLayout`).
/// Coordinates are y up from the bottom-left of the block's box.
pub trait RichBlockGeometry {
    /// The block's whole height, including the style's spacing above and
    /// below it; blocks stack by adding these.
    fn height(&self) -> f64;

    /// The caret nearest `point` (block coordinates); in a table it names its
    /// cell.
    fn hit_test(&self, point: Point) -> RichCaret;

    /// The thin box the caret draws as, in block coordinates.
    fn caret_rect(&self, caret: RichCaret, caret_width: f64) -> Result<Rect, RichLayoutError>;
}

/// One block laid out to a width (C# `RichBlockLayout`).
#[derive(Clone, Debug)]
pub struct RichBlockLayout {
    /// The lines, top to bottom.
    pub lines: Vec<RichLayoutLine>,
    /// The block's whole height, including spacing above and below.
    pub height: f64,
    /// Where the block's text starts: after the list gutter, the quote bar or
    /// the code padding.
    pub text_left: f64,
    /// A list item's marker (a bullet from
    /// [`RichLayoutStyle::bullet_marker`], or "3."), or `None`.
    pub marker: Option<String>,
    /// Whether `marker` is a bullet, drawn as a shape rather than as text:
    /// the UI font is not guaranteed to hold the bullet glyphs.
    pub marker_is_bullet: bool,
    /// The marker's left edge.
    pub marker_x: f64,
    /// The marker's baseline.
    pub marker_baseline: f64,
    /// The marker's face (a list item's block face).
    pub marker_face: Option<RichFace>,
    /// A quote's bar, or `None`.
    pub quote_bar: Option<Rect>,
    /// A code block's shaded background, or `None`.
    pub code_background: Option<Rect>,
    length: usize,
}

/// `Rect` from agg-sharp's `RectangleDouble(left, bottom, right, top)`.
pub(crate) fn rect_lbrt(left: f64, bottom: f64, right: f64, top: f64) -> Rect {
    Rect::new(left, bottom, right - left, top - bottom)
}

/// What an inline-bearing text needs besides its items to become lines: the
/// inlines a fragment's text comes from (or the code text, only for a code
/// block), the alignment and the face every line is at least as tall as.  A
/// table cell is one of these, so cells reuse the paragraph line building.
pub(crate) struct LineSource<'a> {
    pub inlines: &'a [RichInline],
    pub code_text: Option<&'a str>,
    pub alignment: RichAlignment,
    pub block_face: RichFace,
}

impl RichBlockLayout {
    /// Lays a block out to `width`.  `list_number` is the number an ordered
    /// list item shows; the caller counts it because numbering depends on the
    /// blocks around this one.  Text wraps at spaces and a word wider than the
    /// line breaks between characters; code keeps its spaces and breaks only
    /// at its newlines and where a line is wider than the block.  `ends_list`
    /// marks a list's last item, which takes a paragraph's space below.
    pub fn layout(
        block: &RichBlock,
        width: f64,
        style: &RichLayoutStyle,
        list_number: i32,
        ends_list: bool,
    ) -> Result<Self, RichLayoutError> {
        if block.kind == RichBlockKind::Table {
            // A table is a grid of cells, each laid out like a paragraph.
            return Err(RichLayoutError::TableBlock);
        }

        let mut layout = RichBlockLayout {
            lines: Vec::new(),
            height: 0.0,
            text_left: 0.0,
            marker: None,
            marker_is_bullet: false,
            marker_x: 0.0,
            marker_baseline: 0.0,
            marker_face: None,
            quote_bar: None,
            code_background: None,
            length: 0,
        };
        let block_face = style.block_face(block);
        let (mut before, mut after) = style.block_spacing(block, ends_list);
        let code = block.kind == RichBlockKind::CodeBlock;
        let mut right_inset = 0.0;
        match block.kind {
            RichBlockKind::ListItem => {
                let depth = block.list.as_ref().map_or(0, |l| l.depth);
                layout.text_left = (depth + 1) as f64 * style.list_indent() + style.marker_gutter();
                let ordered = block.list.as_ref().is_some_and(|l| l.ordered);
                let number_marker = if block.list.as_ref().is_some_and(|l| l.marker == ')') {
                    ')'
                } else {
                    '.'
                };
                let marker = if ordered {
                    format!("{list_number}{number_marker}")
                } else {
                    style.bullet_marker(depth).to_string()
                };
                let marker_width = if ordered {
                    block_face.measure(&marker)
                } else {
                    RichLayoutStyle::bullet_box_width(&block_face)
                };

                // A wide number ("1000.") grows the gutter rather than hanging
                // past the item's indent.
                layout.text_left = layout
                    .text_left
                    .max(depth as f64 * style.list_indent() + marker_width + style.marker_gap());
                layout.marker_x = layout.text_left - style.marker_gap() - marker_width;
                layout.marker = Some(marker);
                layout.marker_is_bullet = !ordered;
                layout.marker_face = Some(block_face.clone());
            }
            RichBlockKind::Quote => layout.text_left = style.quote_indent(),
            RichBlockKind::CodeBlock => {
                layout.text_left = style.code_padding();
                right_inset = style.code_padding();
                before += style.code_padding();
                after += style.code_padding();
            }
            _ => {}
        }

        let available = (width - layout.text_left - right_inset).max(1.0);
        let items = match block.kind {
            RichBlockKind::CodeBlock => layout_items::code_items(&block.code_text, &block_face),
            RichBlockKind::Raw => layout_items::raw_items(block, available, style, &block_face),
            _ => layout_items::inline_items(
                &block.inlines,
                &|run| style.run_face(block, run),
                style,
                &block_face,
            ),
        };

        layout.length = block.text_length(0, 0);

        // Lines are stacked top-down first (y down from the block's top), then
        // flipped once the height is known.
        let source = LineSource {
            inlines: &block.inlines,
            code_text: if code { Some(&block.code_text) } else { None },
            alignment: block.alignment,
            block_face,
        };
        let y = stack_lines(
            &mut layout.lines,
            &items,
            &source,
            before,
            layout.text_left,
            available,
            layout.length,
            !code,
            style.line_gap(),
        );

        let lines_top = before;
        let lines_bottom = y;
        layout.height = y + after;
        let height = layout.height;
        let flip = |down: f64| height - down;
        flip_lines(&mut layout.lines, height);

        layout.marker_baseline = layout.lines[0].baseline;
        if block.kind == RichBlockKind::Quote {
            layout.quote_bar = Some(rect_lbrt(
                0.0,
                flip(lines_bottom),
                style.quote_bar_width(),
                flip(lines_top),
            ));
        } else if code {
            layout.code_background = Some(rect_lbrt(
                0.0,
                flip(lines_bottom + style.code_padding()),
                width,
                flip(lines_top - style.code_padding()),
            ));
        }
        Ok(layout)
    }

    /// The caret nearest `point` (block coordinates): the line whose band
    /// holds its y (the first or last line when it is above or below them
    /// all), then the stop on it nearest its x.  Right of a line that wraps
    /// with no space this is the line's end (`at_line_end`).
    pub fn hit_test(&self, point: Point) -> RichCaret {
        hit_test_lines(&self.lines, point)
    }

    /// The caret at `offset`: a thin box centred on the caret x, spanning its
    /// line's ascent and descent.  Where a line wraps, the offset shows at the
    /// start of the next line, or with `at_line_end` at the end of the line
    /// before when it wrapped with no space.
    pub fn caret_rect_at(
        &self,
        offset: usize,
        at_line_end: bool,
        caret_width: f64,
    ) -> Result<Rect, RichLayoutError> {
        let (line, stop) = find_stop(&self.lines, self.length, offset, at_line_end)?;
        Ok(caret_box(line, stop, caret_width))
    }

    /// [`Self::caret_rect_at`] for a [`RichCaret`].
    pub fn caret_rect(&self, caret: RichCaret, caret_width: f64) -> Result<Rect, RichLayoutError> {
        self.caret_rect_at(caret.offset, caret.at_line_end, caret_width)
    }

    /// The line that shows the caret at `offset` (see [`Self::caret_rect_at`]).
    pub fn line_of(
        &self,
        offset: usize,
        at_line_end: bool,
    ) -> Result<&RichLayoutLine, RichLayoutError> {
        find_stop(&self.lines, self.length, offset, at_line_end).map(|(line, _)| line)
    }
}

impl RichBlockGeometry for RichBlockLayout {
    fn height(&self) -> f64 {
        self.height
    }

    fn hit_test(&self, point: Point) -> RichCaret {
        hit_test_lines(&self.lines, point)
    }

    fn caret_rect(&self, caret: RichCaret, caret_width: f64) -> Result<Rect, RichLayoutError> {
        RichBlockLayout::caret_rect(self, caret, caret_width)
    }
}

/// Breaks `items` into lines of `available` width and appends them to
/// `lines`, stacked top-down from `top` (y down; flip them with
/// [`flip_lines`] once the container's height is known).  Returns the y below
/// the last line.
#[allow(clippy::too_many_arguments)]
pub(crate) fn stack_lines(
    lines: &mut Vec<RichLayoutLine>,
    items: &[LayoutItem],
    source: &LineSource<'_>,
    top: f64,
    text_left: f64,
    available: f64,
    length: usize,
    wrap_at_spaces: bool,
    line_gap: f64,
) -> f64 {
    let line_starts = layout_items::break_lines(items, available, wrap_at_spaces);
    let mut y = top;
    for (i, &start) in line_starts.iter().enumerate() {
        let end = line_starts.get(i + 1).copied().unwrap_or(items.len());
        let is_last = i == line_starts.len() - 1;
        let mut line = build_line(
            source, items, start, end, is_last, text_left, available, length,
        );
        line.top = y;
        line.baseline = y + line_gap / 2.0 + line.ascent;
        y += line.ascent + line.descent + line_gap;
        line.bottom = y;
        lines.push(line);
    }
    y
}

/// Turns lines stacked y-down (see [`stack_lines`]) into y-up coordinates in
/// a box `height` tall.
pub(crate) fn flip_lines(lines: &mut [RichLayoutLine], height: f64) {
    for line in lines {
        line.top = height - line.top;
        line.bottom = height - line.bottom;
        line.baseline = height - line.baseline;
        for fragment in &mut line.fragments {
            fragment.baseline = line.baseline;
        }
    }
}

/// [`RichBlockLayout::hit_test`] over any stack of lines (a block's, or one
/// table cell's).
pub(crate) fn hit_test_lines(lines: &[RichLayoutLine], point: Point) -> RichCaret {
    let mut line_index = lines.len() - 1;
    for (i, line) in lines.iter().enumerate() {
        if point.y >= line.bottom {
            line_index = i;
            break;
        }
    }

    // A text atom too wide for one line is split over several, with no caret
    // inside it: a click on any piece goes before or after the whole atom by
    // which half of it was clicked.  A line holding only a middle piece has no
    // stop at all, so a click anywhere on it is taken to that piece.
    let line = &lines[line_index];
    if let Some(piece) = split_atom_piece_at(line, point.x, line.caret_stops.is_empty()) {
        let piece_chars = piece.text.chars().count() as f64;
        let characters = if piece.width > 0.0 {
            ((point.x - piece.x) / piece.width).clamp(0.0, 1.0) * piece_chars
        } else {
            0.0
        };
        let atom_chars = piece
            .atom
            .as_ref()
            .map_or(0, |a| a.raw_markdown.chars().count());
        let second_half = piece.start_in_inline as f64 + characters >= atom_chars as f64 / 2.0;
        return RichCaret::new(piece.start + usize::from(second_half));
    }

    while line_index > 0 && lines[line_index].caret_stops.is_empty() {
        line_index -= 1;
    }

    let stops = &lines[line_index].caret_stops;
    let mut best = stops[0];
    for stop in stops {
        if (stop.x - point.x).abs() < (best.x - point.x).abs() {
            best = *stop;
        }
    }
    best.caret
}

fn split_atom_piece_at(
    line: &RichLayoutLine,
    x: f64,
    anywhere: bool,
) -> Option<&RichLayoutFragment> {
    line.fragments.iter().find(|fragment| {
        let split = match (&fragment.atom, &fragment.face) {
            (Some(atom), Some(_)) => {
                fragment.text.chars().count() < atom.raw_markdown.chars().count()
            }
            _ => false,
        };
        split && (anywhere || (x >= fragment.x && x <= fragment.x + fragment.width))
    })
}

/// The caret box for `stop` on `line`.
pub(crate) fn caret_box(line: &RichLayoutLine, stop: &RichCaretStop, caret_width: f64) -> Rect {
    rect_lbrt(
        stop.x - caret_width / 2.0,
        line.baseline - line.descent,
        stop.x + caret_width / 2.0,
        line.baseline + line.ascent,
    )
}

/// The line and stop that show `offset` among `lines`, whose text is
/// `length` offsets long.
pub(crate) fn find_stop(
    lines: &[RichLayoutLine],
    length: usize,
    offset: usize,
    at_line_end: bool,
) -> Result<(&RichLayoutLine, &RichCaretStop), RichLayoutError> {
    if offset > length {
        return Err(RichLayoutError::OffsetOutOfRange { offset, length });
    }

    let mut before = None;
    let mut plain = None;
    for line in lines {
        for stop in &line.caret_stops {
            if stop.caret.offset == offset {
                if stop.caret.at_line_end == at_line_end {
                    return Ok((line, stop));
                }
                if !stop.caret.at_line_end && plain.is_none() {
                    plain = Some((line, stop));
                }
            } else if stop.caret.offset < offset && !stop.caret.at_line_end {
                before = Some((line, stop));
            }
        }
    }
    Ok(plain
        .or(before)
        .unwrap_or((&lines[0], &lines[0].caret_stops[0])))
}

#[allow(clippy::too_many_arguments)]
fn build_line(
    source: &LineSource<'_>,
    items: &[LayoutItem],
    start: usize,
    end: usize,
    is_last: bool,
    text_left: f64,
    available: f64,
    length: usize,
) -> RichLayoutLine {
    let block_face = &source.block_face;
    let mut line = RichLayoutLine {
        start: items.get(start).map_or(length, |item| item.offset),
        end: items.get(end).map_or(length, |item| item.offset),
        is_last,
        ..RichLayoutLine::default()
    };

    // Every line is at least as tall as the block's face, so an empty line
    // still holds a caret.
    let mut ascent = block_face.ascent_px();
    let mut descent = block_face.descent_px();
    let mut relative_x = vec![0.0; end - start + 1];
    let mut content_end = start;
    for i in start..end {
        let item = &items[i];
        relative_x[i - start + 1] = relative_x[i - start] + item.advance_at(relative_x[i - start]);
        if item.is_content() {
            content_end = i + 1;
        }
        match &item.face {
            Some(face) => {
                ascent = ascent.max(face.ascent_px());
                descent = descent.max(face.descent_px());
            }
            None => ascent = ascent.max(item.box_height),
        }
    }

    line.ascent = ascent;
    line.descent = descent;
    line.width = relative_x[content_end - start];
    let align_offset = match source.alignment {
        RichAlignment::Center => ((available - line.width) / 2.0).max(0.0),
        RichAlignment::Right => (available - line.width).max(0.0),
        RichAlignment::Left => 0.0,
    };
    line.left = text_left + align_offset;

    // Trailing spaces can run past the column; their carets stay at its edge
    // instead of off the block.  Those stops then share one x, so a click
    // reaches only the first of them — deliberate: the spaces past the edge
    // are invisible, and the caret still reaches the rest by keyboard.
    let right = text_left + available;
    let left = line.left;
    let caret_x = |i: usize| (left + relative_x[i - start]).min(right);
    for (i, item) in items.iter().enumerate().take(end).skip(start) {
        if item.stop_before {
            line.caret_stops.push(RichCaretStop {
                caret: RichCaret::new(item.offset),
                x: caret_x(i),
            });
        }
    }

    if is_last {
        line.caret_stops.push(RichCaretStop {
            caret: RichCaret::new(length),
            x: caret_x(end),
        });
    } else if end > start
        && end < items.len()
        && items[end].stop_before
        && items[end - 1].is_content()
    {
        // Wrapped with no space: the line's visual end is a caret position of
        // its own.
        line.caret_stops.push(RichCaretStop {
            caret: RichCaret::at_line_end(items[end].offset),
            x: caret_x(end),
        });
    }

    add_fragments(source, items, start, end, length, &mut line, &|i| {
        left + relative_x[i - start]
    });
    line
}

// Consecutive characters of one inline become one fragment; each box and tab
// is its own; breaks draw nothing.
fn add_fragments(
    source: &LineSource<'_>,
    items: &[LayoutItem],
    start: usize,
    end: usize,
    length: usize,
    line: &mut RichLayoutLine,
    x: &dyn Fn(usize) -> f64,
) {
    let mut i = start;
    while i < end {
        let item = &items[i];
        if item.kind == LayoutItemKind::Break {
            i += 1;
            continue;
        }

        let mut run_end = i + 1;
        if item.kind != LayoutItemKind::Box && !item.is_tab {
            while run_end < end
                && matches!(
                    items[run_end].kind,
                    LayoutItemKind::Char | LayoutItemKind::Space
                )
                && !items[run_end].is_tab
                && items[run_end].inline_index == item.inline_index
            {
                run_end += 1;
            }
        }

        let inline = item.inline_index.map(|index| &source.inlines[index]);
        let atom = inline.and_then(|inline| inline.as_atom());
        let text_source: &str = if item.kind == LayoutItemKind::Box {
            ""
        } else if let Some(code_text) = source.code_text {
            code_text
        } else {
            match inline.map(|inline| &inline.content) {
                Some(RichInlineContent::Atom(atom)) => &atom.raw_markdown,
                Some(RichInlineContent::Run(run)) => &run.text,
                None => "",
            }
        };
        let offset_after = items.get(run_end).map_or(length, |item| item.offset);
        line.fragments.push(RichLayoutFragment {
            inline_index: item.inline_index,
            start_in_inline: item.start_in_inline,
            start: item.offset,
            // Every piece of an atom split over lines stands for the whole
            // atom, as its one offset.
            length: if atom.is_some() {
                1
            } else {
                offset_after - item.offset
            },
            text: text_source
                .chars()
                .skip(item.start_in_inline)
                .take(run_end - i)
                .collect(),
            face: item.face.clone(),
            atom: atom.cloned(),
            x: x(i),
            width: x(run_end) - x(i),
            baseline: 0.0,
            ascent: item
                .face
                .as_ref()
                .map_or(item.box_height, |face| face.ascent_px()),
            descent: item.face.as_ref().map_or(0.0, |face| face.descent_px()),
        });
        i = run_end;
    }
}
