//! Pixel geometry for [`RichTextEdit`](super::super::RichTextEdit): mapping
//! between caret [`DocPos`] byte offsets and on-screen coordinates using the
//! cached [`DocLayout`].
//!
//! The layout is expressed **top-down** (y grows downward from the document
//! top); these helpers flip it into the framework's Y-up local space and fold
//! in padding and the internal vertical scroll offset, mirroring the read-only
//! [`RichTextView`](super::super::view::RichTextView) painter and
//! `TextArea`'s geometry so paint, hit-testing and caret placement all agree.

use crate::geometry::Point;
use crate::text::measure_advance;

use super::super::layout::LineLayout;
use super::super::model::DocPos;
use super::RichTextEdit;

/// Where a caret sits, in widget-local Y-up coordinates.
pub(crate) struct CaretGeom {
    /// X of the caret.
    pub x: f64,
    /// Y of the caret's bottom edge (Y-up).
    pub y_bottom: f64,
    /// Line-box height.
    pub height: f64,
}

impl RichTextEdit {
    /// Y-up coordinate of the document's top edge (doc y = 0). A downward
    /// doc-space `y_top` maps to `doc_top_yup - y_top`.
    pub(crate) fn doc_top_yup(&self) -> f64 {
        self.bounds.height - self.padding + self.vbar.offset
    }

    /// Inner content width (widget width minus horizontal padding).
    pub(crate) fn inner_width(&self) -> f64 {
        (self.bounds.width - self.padding * 2.0).max(0.0)
    }

    /// Inner content height (widget height minus vertical padding).
    pub(crate) fn inner_height(&self) -> f64 {
        (self.bounds.height - self.padding * 2.0).max(0.0)
    }

    /// X offset (from a line's text start) of byte `byte` within `line`.
    pub(crate) fn x_in_line(&self, line: &LineLayout, byte: usize) -> f64 {
        let b = byte.clamp(line.start_byte, line.end_byte);
        for frag in &line.fragments {
            let fend = frag.start_byte + frag.text.len();
            if b <= fend {
                let local = b.saturating_sub(frag.start_byte).min(frag.text.len());
                return frag.x + measure_advance(&frag.font, &frag.text[..local], frag.font_size);
            }
        }
        line.fragments.last().map(|f| f.x + f.width).unwrap_or(0.0)
    }

    /// Byte offset (into the block's flattened text) nearest to `rel_x`, an X
    /// offset from `line`'s text start.
    fn byte_in_line(&self, line: &LineLayout, rel_x: f64) -> usize {
        if line.fragments.is_empty() {
            return line.start_byte;
        }
        if rel_x <= 0.0 {
            return line.start_byte;
        }
        let mut best_byte = line.start_byte;
        let mut best_delta = f64::INFINITY;
        for frag in &line.fragments {
            let txt = &frag.text;
            for (i, _) in txt
                .char_indices()
                .chain(std::iter::once((txt.len(), ' ')))
            {
                let x = frag.x + measure_advance(&frag.font, &txt[..i], frag.font_size);
                let d = (x - rel_x).abs();
                if d < best_delta {
                    best_delta = d;
                    best_byte = frag.start_byte + i;
                }
            }
        }
        best_byte
    }

    /// A flat list of visual lines in document order: `(block, line_index,
    /// y_top_downward, height)`.
    pub(crate) fn visual_lines(&self) -> Vec<(usize, usize, f64, f64)> {
        let mut out = Vec::new();
        let Some(layout) = &self.layout else {
            return out;
        };
        let mut y_top = 0.0f64;
        for (bi, bl) in layout.blocks.iter().enumerate() {
            let mut ly = y_top;
            for (li, line) in bl.lines.iter().enumerate() {
                out.push((bi, li, ly, line.height));
                ly += line.height;
            }
            y_top += bl.height;
        }
        out
    }

    /// Index into [`visual_lines`](Self::visual_lines) of the line the caret
    /// sits on. The last line whose `(block, start_byte)` does not exceed the
    /// caret wins, so a caret in a dropped-space gap lands on the earlier line.
    pub(crate) fn caret_visual_line(&self, pos: DocPos) -> usize {
        let Some(layout) = &self.layout else {
            return 0;
        };
        let mut idx = 0usize;
        let mut best = 0usize;
        for (bi, bl) in layout.blocks.iter().enumerate() {
            for line in &bl.lines {
                if bi < pos.block || (bi == pos.block && line.start_byte <= pos.byte) {
                    best = idx;
                }
                idx += 1;
            }
        }
        best
    }

    /// Caret geometry (Y-up) for `pos`, or `None` before the first layout.
    pub(crate) fn caret_geometry(&self, pos: DocPos) -> Option<CaretGeom> {
        let layout = self.layout.as_ref()?;
        let vlines = self.visual_lines();
        let vi = self.caret_visual_line(pos);
        let &(bi, li, y_top, height) = vlines.get(vi)?;
        let bl = layout.blocks.get(bi)?;
        let line = bl.lines.get(li)?;
        let text_left = self.padding + bl.text_left + line.align_dx;
        let x = text_left + self.x_in_line(line, pos.byte);
        let y_bottom = self.doc_top_yup() - (y_top + height);
        Some(CaretGeom { x, y_bottom, height })
    }

    /// Hit-test a widget-local Y-up point to a caret [`DocPos`].
    pub(crate) fn hit_test_pos(&self, local: Point) -> DocPos {
        let Some(layout) = &self.layout else {
            return DocPos::new(0, 0);
        };
        let vlines = self.visual_lines();
        if vlines.is_empty() {
            return DocPos::new(0, 0);
        }
        // Downward distance from the document top to the pointer.
        let doc_y = self.doc_top_yup() - local.y;
        // Pick the visual line whose vertical band contains `doc_y`, clamping
        // above the first / below the last line.
        let mut chosen = 0usize;
        for (i, &(_, _, y_top, _)) in vlines.iter().enumerate() {
            if doc_y >= y_top {
                chosen = i;
            }
        }
        if doc_y < vlines[0].2 {
            chosen = 0;
        }
        let (bi, li, _, _) = vlines[chosen];
        let bl = &layout.blocks[bi];
        let line = &bl.lines[li];
        let text_left = self.padding + bl.text_left + line.align_dx;
        let rel_x = local.x - text_left;
        let byte = self.byte_in_line(line, rel_x);
        DocPos::new(bi, byte)
    }

    /// Start and end [`DocPos`] of the visual line the caret sits on — the
    /// targets for plain Home / End.
    pub(crate) fn caret_line_bounds(&self, caret: DocPos) -> (DocPos, DocPos) {
        let Some(layout) = &self.layout else {
            return (caret, caret);
        };
        let vlines = self.visual_lines();
        let vi = self.caret_visual_line(caret);
        let Some(&(bi, li, _, _)) = vlines.get(vi) else {
            return (caret, caret);
        };
        let line = &layout.blocks[bi].lines[li];
        (
            DocPos::new(bi, line.start_byte),
            DocPos::new(bi, line.end_byte),
        )
    }

    /// Number of visual lines that fit in the viewport (PageUp/PageDown step).
    /// Uses the caret line's height as a representative metric; always ≥ 1.
    pub(crate) fn page_lines(&self, caret: DocPos) -> usize {
        let vlines = self.visual_lines();
        let vi = self.caret_visual_line(caret);
        let line_h = vlines.get(vi).map(|v| v.3).unwrap_or(0.0);
        if line_h <= 0.0 {
            return 1;
        }
        ((self.inner_height() / line_h).floor() as usize).max(1)
    }

    /// The [`DocPos`] `delta` visual lines from `caret` (negative = up),
    /// preserving the caret's pixel column. Clamps to the first/last line.
    pub(crate) fn pos_by_visual_line(&self, caret: DocPos, delta: isize) -> DocPos {
        let Some(layout) = &self.layout else {
            return caret;
        };
        let vlines = self.visual_lines();
        if vlines.is_empty() {
            return caret;
        }
        let cur = self.caret_visual_line(caret) as isize;
        let last = vlines.len() as isize - 1;
        let target = (cur + delta).clamp(0, last);
        if target == cur {
            // Already at the extreme — move to line edge for a natural feel.
            let (bi, li, _, _) = vlines[cur as usize];
            let line = &layout.blocks[bi].lines[li];
            return DocPos::new(bi, if delta < 0 { line.start_byte } else { line.end_byte });
        }
        // Current caret's pixel column (x offset from its line's text start).
        let (cbi, cli, _, _) = vlines[cur as usize];
        let cur_line = &layout.blocks[cbi].lines[cli];
        let cur_x = self.x_in_line(cur_line, caret.byte);
        let (tbi, tli, _, _) = vlines[target as usize];
        let tline = &layout.blocks[tbi].lines[tli];
        let byte = self.byte_in_line(tline, cur_x);
        DocPos::new(tbi, byte)
    }
}
